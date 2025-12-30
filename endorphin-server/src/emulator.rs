//! Android Emulator Management Module
//! 
//! This module provides functionality to check, start, and manage Android emulators
//! before starting the Remote ADB Server.

use endorphin_common::{Result, RemoteAdbError};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};
use tokio::time::sleep;
use tracing::{debug, info};

/// Android emulator manager
pub struct EmulatorManager {
    android_home: Option<String>,
    emulator_timeout: Duration,
}

/// Emulator status information
#[derive(Debug, Clone)]
pub struct EmulatorStatus {
    pub is_running: bool,
    pub device_id: Option<String>,
    pub avd_name: Option<String>,
    pub android_version: Option<String>,
}

impl EmulatorManager {
    /// Create a new emulator manager
    pub fn new() -> Self {
        Self {
            android_home: Self::detect_android_home(),
            emulator_timeout: Duration::from_secs(120),
        }
    }

    /// Create emulator manager with custom timeout
    pub fn with_timeout(timeout: Duration) -> Self {
        Self {
            android_home: Self::detect_android_home(),
            emulator_timeout: timeout,
        }
    }

    /// Detect Android SDK home directory
    fn detect_android_home() -> Option<String> {
        // Check environment variable first
        if let Ok(android_home) = std::env::var("ANDROID_HOME") {
            if std::path::Path::new(&android_home).exists() {
                return Some(android_home);
            }
        }

        // Check common installation paths
        let home_dir = std::env::var("HOME").unwrap_or_default();
        let common_paths = vec![
            format!("{}/Library/Android/sdk", home_dir),     // macOS
            format!("{}/Android/Sdk", home_dir),             // Linux/macOS
            format!("{}/android-sdk", home_dir),             // Linux
            format!("{}/zoo/android-sdk", home_dir),         // Custom path
            "/opt/android-sdk".to_string(),                  // System-wide Linux
        ];

        common_paths.into_iter().find(|path| std::path::Path::new(path).exists())
    }

    /// Check if ADB is available
    pub fn check_adb_available(&self) -> Result<bool> {
        let adb_path = if let Some(android_home) = &self.android_home {
            format!("{}/platform-tools/adb", android_home)
        } else {
            "adb".to_string()
        };

        match Command::new(&adb_path)
            .arg("version")
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
        {
            Ok(status) => Ok(status.success()),
            Err(_) => Ok(false),
        }
    }

    /// Get current emulator status
    pub async fn get_emulator_status(&self) -> Result<EmulatorStatus> {
        let adb_path = if let Some(android_home) = &self.android_home {
            format!("{}/platform-tools/adb", android_home)
        } else {
            "adb".to_string()
        };

        // Check for running devices
        let output = Command::new(&adb_path)
            .arg("devices")
            .output()
            .map_err(|e| RemoteAdbError::adb(format!("Failed to run adb devices: {}", e)))?;

        let devices_output = String::from_utf8_lossy(&output.stdout);
        debug!("ADB devices output: {}", devices_output);

        // Look for emulator devices
        for line in devices_output.lines() {
            if line.contains("emulator-") && line.contains("device") {
                let device_id = line.split_whitespace().next().unwrap_or("").to_string();
                
                // Get additional device info
                let android_version = self.get_device_property(&device_id, "ro.build.version.release").await.ok();
                let avd_name = self.get_device_property(&device_id, "ro.kernel.qemu.avd_name").await.ok();

                return Ok(EmulatorStatus {
                    is_running: true,
                    device_id: Some(device_id),
                    avd_name,
                    android_version,
                });
            }
        }

        Ok(EmulatorStatus {
            is_running: false,
            device_id: None,
            avd_name: None,
            android_version: None,
        })
    }

    /// Get device property using adb
    async fn get_device_property(&self, device_id: &str, property: &str) -> Result<String> {
        let adb_path = if let Some(android_home) = &self.android_home {
            format!("{}/platform-tools/adb", android_home)
        } else {
            "adb".to_string()
        };

        let output = Command::new(&adb_path)
            .arg("-s")
            .arg(device_id)
            .arg("shell")
            .arg("getprop")
            .arg(property)
            .output()
            .map_err(|e| RemoteAdbError::adb(format!("Failed to get device property: {}", e)))?;

        let property_value = String::from_utf8_lossy(&output.stdout).trim().to_string();
        if property_value.is_empty() {
            Err(RemoteAdbError::adb(format!("Property {} not found", property)))
        } else {
            Ok(property_value)
        }
    }

    /// List available AVDs
    pub fn list_avds(&self) -> Result<Vec<String>> {
        let android_home = self.android_home.as_ref()
            .ok_or_else(|| RemoteAdbError::config("Android SDK not found".to_string()))?;

        let emulator_path = format!("{}/emulator/emulator", android_home);
        
        let output = Command::new(&emulator_path)
            .arg("-list-avds")
            .output()
            .map_err(|e| RemoteAdbError::adb(format!("Failed to list AVDs: {}", e)))?;

        let avds_output = String::from_utf8_lossy(&output.stdout);
        let avds: Vec<String> = avds_output
            .lines()
            .filter(|line| !line.trim().is_empty())
            .map(|line| line.trim().to_string())
            .collect();

        Ok(avds)
    }

    /// Start Android emulator using the reference script logic
    pub async fn start_emulator(&self, avd_name: Option<String>) -> Result<EmulatorStatus> {
        let android_home = self.android_home.as_ref()
            .ok_or_else(|| RemoteAdbError::config("Android SDK not found".to_string()))?;

        info!("🚀 Starting Android emulator...");
        info!("Android SDK: {}", android_home);

        // Check if emulator is already running
        let current_status = self.get_emulator_status().await?;
        if current_status.is_running {
            info!("✅ Emulator already running: {:?}", current_status.device_id);
            return Ok(current_status);
        }

        // Get available AVDs
        let avds = self.list_avds()?;
        if avds.is_empty() {
            return Err(RemoteAdbError::config(
                "No AVDs found. Please create an AVD first using Android Studio or avdmanager".to_string()
            ));
        }

        // Select AVD to start
        let selected_avd = if let Some(name) = avd_name {
            if !avds.contains(&name) {
                return Err(RemoteAdbError::config(
                    format!("AVD '{}' not found. Available AVDs: {:?}", name, avds)
                ));
            }
            name
        } else {
            avds[0].clone()
        };

        info!("📱 Available AVDs: {:?}", avds);
        info!("🎯 Starting AVD: {}", selected_avd);

        // Determine optimal emulator parameters based on system
        let emulator_path = format!("{}/emulator/emulator", android_home);
        let mut cmd = Command::new(&emulator_path);
        
        cmd.arg("-avd")
           .arg(&selected_avd)
           .arg("-memory")
           .arg("4096")
           .arg("-gpu")
           .arg("swiftshader_indirect") // Safe GPU mode for compatibility
           .arg("-no-snapshot-load")    // Avoid corrupted snapshot issues
           .stdout(Stdio::null())
           .stderr(Stdio::null());

        // Detect system architecture for optimization
        if cfg!(target_arch = "aarch64") && cfg!(target_os = "macos") {
            // Apple Silicon optimization
            cmd.arg("-cores").arg("4");
        } else if cfg!(target_os = "linux") {
            // Linux with KVM acceleration
            cmd.arg("-accel").arg("auto");
        }

        info!("⏳ Starting emulator process...");
        
        // Start emulator in background
        let mut child = cmd.spawn()
            .map_err(|e| RemoteAdbError::adb(format!("Failed to start emulator: {}", e)))?;

        let emulator_pid = child.id();
        info!("Emulator process ID: {}", emulator_pid);

        // Wait for emulator to be ready
        info!("⏳ Waiting for emulator to be ready...");
        let start_time = Instant::now();
        let mut last_log_time = Instant::now();

        loop {
            // Check if emulator process is still running
            match child.try_wait() {
                Ok(Some(status)) => {
                    return Err(RemoteAdbError::adb(
                        format!("Emulator process exited with status: {}", status)
                    ));
                }
                Ok(None) => {
                    // Process is still running, continue checking
                }
                Err(e) => {
                    return Err(RemoteAdbError::adb(
                        format!("Failed to check emulator process: {}", e)
                    ));
                }
            }

            // Check if emulator is ready
            let status = self.get_emulator_status().await?;
            if status.is_running {
                info!("✅ Emulator is ready!");
                
                // Display device information
                if let Some(device_id) = &status.device_id {
                    info!("📋 Device information:");
                    if let Some(version) = &status.android_version {
                        info!("  Android version: {}", version);
                    }
                    if let Some(avd) = &status.avd_name {
                        info!("  AVD name: {}", avd);
                    }
                    info!("  Device ID: {}", device_id);
                }

                info!("🎉 Emulator startup successful!");
                return Ok(status);
            }

            // Check timeout
            if start_time.elapsed() > self.emulator_timeout {
                // Try to kill the emulator process
                let _ = child.kill();
                return Err(RemoteAdbError::adb(
                    format!("Emulator startup timeout ({} seconds)", self.emulator_timeout.as_secs())
                ));
            }

            // Log progress every 15 seconds
            if last_log_time.elapsed() >= Duration::from_secs(15) {
                let elapsed = start_time.elapsed().as_secs();
                let timeout = self.emulator_timeout.as_secs();
                info!("Waiting for emulator... ({}/{} seconds)", elapsed, timeout);
                last_log_time = Instant::now();
            }

            sleep(Duration::from_secs(2)).await;
        }
    }

    /// Ensure emulator is running, start if necessary
    pub async fn ensure_emulator_running(&self, avd_name: Option<String>) -> Result<EmulatorStatus> {
        info!("🔍 Checking emulator status...");
        
        // Check if ADB is available
        if !self.check_adb_available()? {
            return Err(RemoteAdbError::config(
                "ADB not found. Please install Android SDK and set ANDROID_HOME".to_string()
            ));
        }

        // Check current status
        let status = self.get_emulator_status().await?;
        
        if status.is_running {
            info!("✅ Emulator already running");
            if let Some(device_id) = &status.device_id {
                info!("Device ID: {}", device_id);
            }
            Ok(status)
        } else {
            info!("📱 No emulator running, starting one...");
            self.start_emulator(avd_name).await
        }
    }

    /// Stop all running emulators
    #[allow(dead_code)]
    pub async fn stop_emulators(&self) -> Result<()> {
        let adb_path = if let Some(android_home) = &self.android_home {
            format!("{}/platform-tools/adb", android_home)
        } else {
            "adb".to_string()
        };

        info!("🛑 Stopping all emulators...");

        // Get list of emulator devices
        let output = Command::new(&adb_path)
            .arg("devices")
            .output()
            .map_err(|e| RemoteAdbError::adb(format!("Failed to run adb devices: {}", e)))?;

        let devices_output = String::from_utf8_lossy(&output.stdout);
        
        let mut stopped_any = false;
        for line in devices_output.lines() {
            if line.contains("emulator-") && line.contains("device") {
                let device_id = line.split_whitespace().next().unwrap_or("");
                info!("Stopping emulator: {}", device_id);
                
                let _ = Command::new(&adb_path)
                    .arg("-s")
                    .arg(device_id)
                    .arg("emu")
                    .arg("kill")
                    .output();
                
                stopped_any = true;
            }
        }

        if stopped_any {
            info!("✅ All emulators stopped");
        } else {
            info!("ℹ️  No running emulators found");
        }

        Ok(())
    }
}

impl Default for EmulatorManager {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_emulator_manager_creation() {
        let manager = EmulatorManager::new();
        // Should not panic and should have reasonable timeout
        assert!(manager.emulator_timeout.as_secs() > 0);
    }

    #[test]
    fn test_emulator_manager_with_timeout() {
        let timeout = Duration::from_secs(60);
        let manager = EmulatorManager::with_timeout(timeout);
        assert_eq!(manager.emulator_timeout, timeout);
    }

    #[tokio::test]
    async fn test_get_emulator_status() {
        let manager = EmulatorManager::new();
        // This should not panic even if no emulator is running
        let result = manager.get_emulator_status().await;
        // We can't assert the result since it depends on system state,
        // but we can ensure it doesn't panic
        assert!(result.is_ok() || result.is_err());
    }
}