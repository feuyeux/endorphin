//! Common types for the remote ADB system

use serde::{Deserialize, Serialize};
use std::time::SystemTime;

/// Configuration for establishing a remote connection
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConnectionConfig {
    pub host: String,
    pub port: u16,
    pub auth_token: String,
    pub timeout_seconds: u64,
    pub retry_attempts: u32,
}

impl Default for ConnectionConfig {
    fn default() -> Self {
        Self {
            host: "localhost".to_string(),
            port: 5555,
            auth_token: String::new(),
            timeout_seconds: 30,
            retry_attempts: 3,
        }
    }
}

/// Status of a connection
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ConnectionStatus {
    pub is_connected: bool,
    pub connection_time: Option<SystemTime>,
    pub last_heartbeat: Option<SystemTime>,
    pub error_message: Option<String>,
}

/// Information about a connected client
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ClientConnection {
    pub client_ip: String,
    pub connection_id: String,
    pub connected_at: SystemTime,
    pub is_authenticated: bool,
    pub active_streams: Vec<String>,
}

/// Information about an Android device/emulator
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeviceInfo {
    pub device_id: String,
    pub device_name: String,
    pub android_version: String,
    pub api_level: u32,
    pub status: DeviceStatus,
    pub emulator_port: Option<u16>,
}

/// Status of an Android device
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum DeviceStatus {
    Online,
    Offline,
    Unauthorized,
    Unknown,
}

/// Log entry from Android logcat
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LogEntry {
    pub timestamp: SystemTime,
    pub level: LogLevel,
    pub tag: String,
    pub process_id: u32,
    pub thread_id: u32,
    pub message: String,
}

/// Android log levels
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum LogLevel {
    Verbose,
    Debug,
    Info,
    Warn,
    Error,
    Fatal,
}

/// Filters for log entries
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct LogFilters {
    pub log_level: Option<LogLevel>,
    pub tag_filter: Option<String>,
    pub package_filter: Option<String>,
    pub regex_pattern: Option<String>,
    pub max_lines: Option<usize>,
}

/// Handle for a log stream
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StreamHandle {
    pub stream_id: String,
    pub device_id: String,
    pub created_at: SystemTime,
    pub filters: LogFilters,
}

/// Status of a log stream
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StreamStatus {
    pub is_active: bool,
    pub lines_captured: u64,
    pub bytes_transferred: u64,
    pub last_activity: Option<SystemTime>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_connection_config_serialization() {
        let config = ConnectionConfig::default();
        let json = serde_json::to_string(&config).unwrap();
        let deserialized: ConnectionConfig = serde_json::from_str(&json).unwrap();
        assert_eq!(config.host, deserialized.host);
        assert_eq!(config.port, deserialized.port);
    }

    #[test]
    fn test_device_info_serialization() {
        let device = DeviceInfo {
            device_id: "emulator-5554".to_string(),
            device_name: "Android Emulator".to_string(),
            android_version: "13".to_string(),
            api_level: 33,
            status: DeviceStatus::Online,
            emulator_port: Some(5554),
        };
        let json = serde_json::to_string(&device).unwrap();
        let deserialized: DeviceInfo = serde_json::from_str(&json).unwrap();
        assert_eq!(device.device_id, deserialized.device_id);
        assert_eq!(device.api_level, deserialized.api_level);
    }

    #[test]
    fn test_log_entry_serialization() {
        let log_entry = LogEntry {
            timestamp: SystemTime::now(),
            level: LogLevel::Info,
            tag: "MainActivity".to_string(),
            process_id: 1234,
            thread_id: 5678,
            message: "Application started".to_string(),
        };
        let json = serde_json::to_string(&log_entry).unwrap();
        let deserialized: LogEntry = serde_json::from_str(&json).unwrap();
        assert_eq!(log_entry.level, deserialized.level);
        assert_eq!(log_entry.tag, deserialized.tag);
        assert_eq!(log_entry.message, deserialized.message);
    }

    #[test]
    fn test_log_level_ordering() {
        assert!(LogLevel::Verbose < LogLevel::Debug);
        assert!(LogLevel::Debug < LogLevel::Info);
        assert!(LogLevel::Info < LogLevel::Warn);
        assert!(LogLevel::Warn < LogLevel::Error);
        assert!(LogLevel::Error < LogLevel::Fatal);
    }
}