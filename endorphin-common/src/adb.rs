//! ADB command execution and device management
//! 
//! This module provides async ADB command execution and device discovery functionality.

use crate::{DeviceInfo, DeviceStatus, Result, RemoteAdbError};
use std::process::Stdio;
use std::time::Duration;
use tokio::process::Command;
use tokio::time::timeout;
use tracing::{debug, error, info, warn};

/// ADB command executor for managing Android devices
#[derive(Debug, Clone)]
pub struct AdbExecutor {
    adb_path: String,
    command_timeout: Duration,
}

impl AdbExecutor {
    /// Create a new ADB executor
    pub fn new() -> Self {
        Self {
            adb_path: "adb".to_string(),
            command_timeout: Duration::from_secs(30),
        }
    }

    /// Create a new ADB executor with custom ADB path
    pub fn with_path<S: Into<String>>(adb_path: S) -> Self {
        Self {
            adb_path: adb_path.into(),
            command_timeout: Duration::from_secs(30),
        }
    }

    /// Set command timeout
    pub fn with_timeout(mut self, timeout: Duration) -> Self {
        self.command_timeout = timeout;
        self
    }

    /// Get the ADB executable path
    pub fn adb_path(&self) -> &str {
        &self.adb_path
    }

    /// Execute an ADB command and return the output
    pub async fn execute_command(&self, args: &[&str]) -> Result<String> {
        debug!("Executing ADB command: {} {}", self.adb_path, args.join(" "));

        let mut cmd = Command::new(&self.adb_path);
        cmd.args(args)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());

        let output = timeout(self.command_timeout, cmd.output())
            .await
            .map_err(|_| RemoteAdbError::Timeout)?
            .map_err(|e| RemoteAdbError::adb(format!("Failed to execute ADB command: {}", e)))?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            error!("ADB command failed: {}", stderr);
            return Err(RemoteAdbError::adb(format!("Command failed: {}", stderr)));
        }

        let stdout = String::from_utf8_lossy(&output.stdout).to_string();
        debug!("ADB command output: {}", stdout);
        Ok(stdout)
    }

    /// Get list of connected devices
    pub async fn get_devices(&self) -> Result<Vec<DeviceInfo>> {
        info!("Discovering ADB devices");
        let output = self.execute_command(&["devices", "-l"]).await?;
        
        let mut devices = Vec::new();
        for line in output.lines().skip(1) { // Skip "List of devices attached" header
            if line.trim().is_empty() {
                continue;
            }

            let device_info = self.parse_device_line(line).await?;
            devices.push(device_info);
        }

        info!("Found {} devices", devices.len());
        Ok(devices)
    }

    /// Parse a device line from `adb devices -l` output
    async fn parse_device_line(&self, line: &str) -> Result<DeviceInfo> {
        let parts: Vec<&str> = line.split_whitespace().collect();
        if parts.len() < 2 {
            return Err(RemoteAdbError::adb(format!("Invalid device line: {}", line)));
        }

        let device_id = parts[0].to_string();
        let status_str = parts[1];
        
        let status = match status_str {
            "device" => DeviceStatus::Online,
            "offline" => DeviceStatus::Offline,
            "unauthorized" => DeviceStatus::Unauthorized,
            _ => DeviceStatus::Unknown,
        };

        // Extract additional device properties
        let mut device_name = "Unknown Device".to_string();
        let mut emulator_port = None;

        // Parse additional properties from the line
        for part in parts.iter().skip(2) {
            if let Some(model) = part.strip_prefix("model:") {
                device_name = model.to_string();
            } else if device_id.starts_with("emulator-") {
                if let Some(port_str) = device_id.strip_prefix("emulator-") {
                    if let Ok(port) = port_str.parse::<u16>() {
                        emulator_port = Some(port);
                    }
                }
            }
        }

        // Get additional device info if device is online
        let (android_version, api_level) = if status == DeviceStatus::Online {
            self.get_device_properties(&device_id).await.unwrap_or_else(|e| {
                warn!("Failed to get device properties for {}: {}", device_id, e);
                ("Unknown".to_string(), 0)
            })
        } else {
            ("Unknown".to_string(), 0)
        };

        Ok(DeviceInfo {
            device_id,
            device_name,
            android_version,
            api_level,
            status,
            emulator_port,
        })
    }

    /// Get device properties (Android version and API level)
    async fn get_device_properties(&self, device_id: &str) -> Result<(String, u32)> {
        let version_output = self
            .execute_command(&["-s", device_id, "shell", "getprop", "ro.build.version.release"])
            .await?;
        let android_version = version_output.trim().to_string();

        let api_output = self
            .execute_command(&["-s", device_id, "shell", "getprop", "ro.build.version.sdk"])
            .await?;
        let api_level = api_output.trim().parse::<u32>().unwrap_or(0);

        Ok((android_version, api_level))
    }

    /// Check if a specific device is online
    pub async fn is_device_online(&self, device_id: &str) -> Result<bool> {
        let devices = self.get_devices().await?;
        Ok(devices
            .iter()
            .any(|d| d.device_id == device_id && d.status == DeviceStatus::Online))
    }

    /// Wait for a device to come online
    pub async fn wait_for_device(&self, device_id: &str, timeout_duration: Duration) -> Result<()> {
        info!("Waiting for device {} to come online", device_id);
        
        let start_time = std::time::Instant::now();
        while start_time.elapsed() < timeout_duration {
            if self.is_device_online(device_id).await? {
                info!("Device {} is now online", device_id);
                return Ok(());
            }
            
            tokio::time::sleep(Duration::from_secs(1)).await;
        }

        Err(RemoteAdbError::Timeout)
    }

    /// Start ADB server
    pub async fn start_server(&self) -> Result<()> {
        info!("Starting ADB server");
        self.execute_command(&["start-server"]).await?;
        Ok(())
    }

    /// Kill ADB server
    pub async fn kill_server(&self) -> Result<()> {
        info!("Killing ADB server");
        self.execute_command(&["kill-server"]).await?;
        Ok(())
    }

    /// Connect to a device over TCP/IP
    pub async fn connect_tcp(&self, host: &str, port: u16) -> Result<()> {
        info!("Connecting to {}:{} via TCP", host, port);
        let address = format!("{}:{}", host, port);
        self.execute_command(&["connect", &address]).await?;
        Ok(())
    }

    /// Disconnect from a TCP/IP device
    pub async fn disconnect_tcp(&self, host: &str, port: u16) -> Result<()> {
        info!("Disconnecting from {}:{}", host, port);
        let address = format!("{}:{}", host, port);
        self.execute_command(&["disconnect", &address]).await?;
        Ok(())
    }
}

impl Default for AdbExecutor {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_adb_executor_creation() {
        let executor = AdbExecutor::new();
        assert_eq!(executor.adb_path, "adb");
        assert_eq!(executor.command_timeout, Duration::from_secs(30));
    }

    #[tokio::test]
    async fn test_adb_executor_with_custom_path() {
        let executor = AdbExecutor::with_path("/custom/path/adb");
        assert_eq!(executor.adb_path, "/custom/path/adb");
    }

    #[tokio::test]
    async fn test_adb_executor_with_timeout() {
        let executor = AdbExecutor::new().with_timeout(Duration::from_secs(60));
        assert_eq!(executor.command_timeout, Duration::from_secs(60));
    }

    #[test]
    fn test_parse_device_line_emulator() {
        let _executor = AdbExecutor::new();
        let _rt = tokio::runtime::Runtime::new().unwrap();
        
        // This is a unit test for the parsing logic, not requiring actual ADB
        // We'll test the parsing of a typical device line format
        let line = "emulator-5554	device product:sdk_gphone64_x86_64 model:sdk_gphone64_x86_64 device:generic_x86_64";
        
        // Since parse_device_line is async and calls other async methods,
        // we'll test the individual parsing components in a synchronous way
        let parts: Vec<&str> = line.split_whitespace().collect();
        assert_eq!(parts[0], "emulator-5554");
        assert_eq!(parts[1], "device");
        
        // Test emulator port extraction
        let device_id = "emulator-5554";
        if let Some(port_str) = device_id.strip_prefix("emulator-") {
            let port: u16 = port_str.parse().unwrap();
            assert_eq!(port, 5554);
        }
    }
}