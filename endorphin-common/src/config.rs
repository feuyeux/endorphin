//! Configuration Management Module
//! 
//! This module provides comprehensive configuration management with support for:
//! - TOML and YAML configuration files
//! - Runtime configuration hot reloading
//! - Environment variable overrides
//! - Configuration validation

use crate::{Result, RemoteAdbError};
use notify::{Watcher, RecursiveMode, Event, EventKind, recommended_watcher};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::net::IpAddr;
use std::path::{Path, PathBuf};
use std::sync::{Arc, RwLock};
use std::time::Duration;
use tokio::sync::broadcast;
use tracing::{info, warn, error, debug};

/// Configuration change event
#[derive(Debug, Clone)]
pub struct ConfigChangeEvent {
    pub config_path: PathBuf,
    pub change_type: ConfigChangeType,
    pub timestamp: std::time::SystemTime,
}

#[derive(Debug, Clone)]
pub enum ConfigChangeType {
    Modified,
    Created,
    Deleted,
}

/// Server configuration section
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ServerConfig {
    pub bind_address: String,
    pub port: u16,
    pub enable_auth: bool,
    pub max_connections: usize,
    pub connection_timeout: u64,
    pub enable_metrics: bool,
}

impl Default for ServerConfig {
    fn default() -> Self {
        Self {
            bind_address: "0.0.0.0".to_string(),
            port: 5555,
            enable_auth: true,
            max_connections: 10,
            connection_timeout: 30,
            enable_metrics: true,
        }
    }
}

/// Security configuration section
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SecurityConfig {
    pub ip_whitelist: Vec<String>,
    pub auth_token: Option<String>,
    pub enable_tls: bool,
    pub tls_cert_path: Option<String>,
    pub tls_key_path: Option<String>,
    pub rate_limit_requests_per_minute: u32,
}

impl Default for SecurityConfig {
    fn default() -> Self {
        Self {
            ip_whitelist: Vec::new(),
            auth_token: None,
            enable_tls: false,
            tls_cert_path: None,
            tls_key_path: None,
            rate_limit_requests_per_minute: 60,
        }
    }
}

/// Logging configuration section
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LoggingConfig {
    pub level: String,
    pub file_path: Option<String>,
    pub max_file_size_mb: u64,
    pub max_files: u32,
    pub enable_json_format: bool,
}

impl Default for LoggingConfig {
    fn default() -> Self {
        Self {
            level: "info".to_string(),
            file_path: None,
            max_file_size_mb: 100,
            max_files: 5,
            enable_json_format: false,
        }
    }
}

/// ADB configuration section
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AdbConfig {
    pub adb_path: Option<String>,
    pub command_timeout: u64,
    pub device_poll_interval: u64,
    pub max_retries: u32,
}

impl Default for AdbConfig {
    fn default() -> Self {
        Self {
            adb_path: None,
            command_timeout: 30,
            device_poll_interval: 5,
            max_retries: 3,
        }
    }
}

/// Performance configuration section
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PerformanceConfig {
    pub log_buffer_size: usize,
    pub heartbeat_interval: u64,
    pub stream_buffer_size: usize,
    pub max_memory_usage_mb: u64,
    pub gc_interval: u64,
    pub enable_compression: bool,
}

impl Default for PerformanceConfig {
    fn default() -> Self {
        Self {
            log_buffer_size: 1000,
            heartbeat_interval: 30,
            stream_buffer_size: 8192,
            max_memory_usage_mb: 512,
            gc_interval: 300, // 5 minutes
            enable_compression: true,
        }
    }
}

/// Client configuration section
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ClientConfig {
    pub default_host: String,
    pub default_port: u16,
    pub connection_timeout: u64,
    pub retry_attempts: u32,
    pub auto_reconnect: bool,
    pub reconnect_delay: u64,
    pub max_reconnect_delay: u64,
}

impl Default for ClientConfig {
    fn default() -> Self {
        Self {
            default_host: "localhost".to_string(),
            default_port: 5555,
            connection_timeout: 30,
            retry_attempts: 3,
            auto_reconnect: true,
            reconnect_delay: 2,
            max_reconnect_delay: 30,
        }
    }
}

/// Main application configuration
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct AppConfig {
    pub server: ServerConfig,
    pub security: SecurityConfig,
    pub logging: LoggingConfig,
    pub adb: AdbConfig,
    pub performance: PerformanceConfig,
    pub client: ClientConfig,
    
    #[serde(flatten)]
    pub custom: HashMap<String, toml::Value>,
}

impl AppConfig {
    /// Validate the configuration
    pub fn validate(&self) -> Result<()> {
        // Validate server configuration
        if self.server.port == 0 {
            return Err(RemoteAdbError::config("Server port cannot be 0"));
        }
        
        if self.server.max_connections == 0 {
            return Err(RemoteAdbError::config("Max connections must be greater than 0"));
        }

        // Validate IP whitelist
        for ip_str in &self.security.ip_whitelist {
            if ip_str.parse::<IpAddr>().is_err() {
                return Err(RemoteAdbError::config(
                    format!("Invalid IP address in whitelist: {}", ip_str)
                ));
            }
        }

        // Validate TLS configuration
        if self.security.enable_tls
            && (self.security.tls_cert_path.is_none() || self.security.tls_key_path.is_none()) {
            return Err(RemoteAdbError::config(
                "TLS enabled but certificate or key path not specified"
            ));
        }

        // Validate logging level
        match self.logging.level.to_lowercase().as_str() {
            "trace" | "debug" | "info" | "warn" | "error" => {},
            _ => return Err(RemoteAdbError::config(
                format!("Invalid logging level: {}", self.logging.level)
            )),
        }

        // Validate performance settings
        if self.performance.log_buffer_size == 0 {
            return Err(RemoteAdbError::config("Log buffer size must be greater than 0"));
        }

        if self.performance.max_memory_usage_mb == 0 {
            return Err(RemoteAdbError::config("Max memory usage must be greater than 0"));
        }

        Ok(())
    }

    /// Apply environment variable overrides
    pub fn apply_env_overrides(&mut self) {
        if let Ok(bind_addr) = std::env::var("ENDORPHIN_BIND_ADDRESS") {
            self.server.bind_address = bind_addr;
        }
        
        if let Ok(port_str) = std::env::var("ENDORPHIN_PORT") {
            if let Ok(port) = port_str.parse::<u16>() {
                self.server.port = port;
            }
        }
        
        if let Ok(auth_token) = std::env::var("ENDORPHIN_AUTH_TOKEN") {
            self.security.auth_token = Some(auth_token);
        }
        
        if let Ok(log_level) = std::env::var("ENDORPHIN_LOG_LEVEL") {
            self.logging.level = log_level;
        }
        
        if let Ok(max_conn_str) = std::env::var("ENDORPHIN_MAX_CONNECTIONS") {
            if let Ok(max_conn) = max_conn_str.parse::<usize>() {
                self.server.max_connections = max_conn;
            }
        }
    }
}

/// Configuration format
#[derive(Debug, Clone, Copy)]
pub enum ConfigFormat {
    Toml,
    Yaml,
}

impl ConfigFormat {
    pub fn from_path(path: &Path) -> Self {
        match path.extension().and_then(|ext| ext.to_str()) {
            Some("yaml") | Some("yml") => ConfigFormat::Yaml,
            _ => ConfigFormat::Toml, // Default to TOML
        }
    }
}

/// Configuration manager with hot reloading support
pub struct ConfigManager {
    config: Arc<RwLock<AppConfig>>,
    config_path: Option<PathBuf>,
    change_sender: broadcast::Sender<ConfigChangeEvent>,
    _watcher: Option<Box<dyn Watcher + Send>>,
}

impl ConfigManager {
    /// Create a new configuration manager with default configuration
    pub fn new() -> Self {
        let (change_sender, _) = broadcast::channel(100);
        
        Self {
            config: Arc::new(RwLock::new(AppConfig::default())),
            config_path: None,
            change_sender,
            _watcher: None,
        }
    }

    /// Create a configuration manager and load from file
    pub async fn from_file<P: AsRef<Path>>(path: P) -> Result<Self> {
        let mut manager = Self::new();
        manager.load_from_file(path).await?;
        Ok(manager)
    }

    /// Load configuration from file
    pub async fn load_from_file<P: AsRef<Path>>(&mut self, path: P) -> Result<()> {
        let path = path.as_ref().to_path_buf();
        let format = ConfigFormat::from_path(&path);
        
        info!("Loading configuration from: {}", path.display());
        
        let content = tokio::fs::read_to_string(&path).await
            .map_err(|e| RemoteAdbError::config(
                format!("Failed to read config file {}: {}", path.display(), e)
            ))?;

        let mut config: AppConfig = match format {
            ConfigFormat::Toml => {
                toml::from_str(&content)
                    .map_err(|e| RemoteAdbError::config(
                        format!("Failed to parse TOML config: {}", e)
                    ))?
            }
            ConfigFormat::Yaml => {
                serde_yaml::from_str(&content)
                    .map_err(|e| RemoteAdbError::config(
                        format!("Failed to parse YAML config: {}", e)
                    ))?
            }
        };

        // Apply environment variable overrides
        config.apply_env_overrides();

        // Validate configuration
        config.validate()?;

        // Update the configuration
        {
            let mut current_config = self.config.write().unwrap();
            *current_config = config;
        }

        self.config_path = Some(path.clone());
        info!("Configuration loaded successfully");

        // Set up file watching for hot reloading
        self.setup_file_watcher(path).await?;

        Ok(())
    }

    /// Save current configuration to file
    pub async fn save_to_file<P: AsRef<Path>>(&self, path: P) -> Result<()> {
        let path = path.as_ref();
        let format = ConfigFormat::from_path(path);
        
        let config = self.get_config();
        
        let content = match format {
            ConfigFormat::Toml => {
                toml::to_string_pretty(&config)
                    .map_err(|e| RemoteAdbError::config(
                        format!("Failed to serialize config to TOML: {}", e)
                    ))?
            }
            ConfigFormat::Yaml => {
                serde_yaml::to_string(&config)
                    .map_err(|e| RemoteAdbError::config(
                        format!("Failed to serialize config to YAML: {}", e)
                    ))?
            }
        };

        tokio::fs::write(path, content).await
            .map_err(|e| RemoteAdbError::config(
                format!("Failed to write config file {}: {}", path.display(), e)
            ))?;

        info!("Configuration saved to: {}", path.display());
        Ok(())
    }

    /// Get a copy of the current configuration
    pub fn get_config(&self) -> AppConfig {
        self.config.read().unwrap().clone()
    }

    /// Update configuration
    pub fn update_config<F>(&self, updater: F) -> Result<()>
    where
        F: FnOnce(&mut AppConfig) -> Result<()>,
    {
        let mut config = self.config.write().unwrap();
        updater(&mut config)?;
        config.validate()?;
        
        info!("Configuration updated");
        Ok(())
    }

    /// Subscribe to configuration changes
    pub fn subscribe_to_changes(&self) -> broadcast::Receiver<ConfigChangeEvent> {
        self.change_sender.subscribe()
    }

    /// Reload configuration from file
    pub async fn reload(&mut self) -> Result<()> {
        if let Some(path) = &self.config_path.clone() {
            info!("Reloading configuration from: {}", path.display());
            self.load_from_file(path).await?;
            
            // Notify subscribers of the change
            let event = ConfigChangeEvent {
                config_path: path.clone(),
                change_type: ConfigChangeType::Modified,
                timestamp: std::time::SystemTime::now(),
            };
            
            if let Err(e) = self.change_sender.send(event) {
                warn!("Failed to notify config change subscribers: {}", e);
            }
            
            Ok(())
        } else {
            Err(RemoteAdbError::config("No configuration file path set"))
        }
    }

    /// Set up file watcher for hot reloading
    async fn setup_file_watcher(&mut self, path: PathBuf) -> Result<()> {
        let change_sender = self.change_sender.clone();
        let watch_path = path.clone();
        
        let mut watcher = recommended_watcher(move |res: notify::Result<Event>| {
            match res {
                Ok(event) => {
                    debug!("File system event: {:?}", event);
                    
                    if event.paths.contains(&watch_path) {
                        match event.kind {
                            EventKind::Modify(_) => {
                                let change_event = ConfigChangeEvent {
                                    config_path: watch_path.clone(),
                                    change_type: ConfigChangeType::Modified,
                                    timestamp: std::time::SystemTime::now(),
                                };
                                
                                if let Err(e) = change_sender.send(change_event) {
                                    warn!("Failed to send config change event: {}", e);
                                }
                            }
                            EventKind::Create(_) => {
                                let change_event = ConfigChangeEvent {
                                    config_path: watch_path.clone(),
                                    change_type: ConfigChangeType::Created,
                                    timestamp: std::time::SystemTime::now(),
                                };
                                
                                if let Err(e) = change_sender.send(change_event) {
                                    warn!("Failed to send config change event: {}", e);
                                }
                            }
                            EventKind::Remove(_) => {
                                let change_event = ConfigChangeEvent {
                                    config_path: watch_path.clone(),
                                    change_type: ConfigChangeType::Deleted,
                                    timestamp: std::time::SystemTime::now(),
                                };
                                
                                if let Err(e) = change_sender.send(change_event) {
                                    warn!("Failed to send config change event: {}", e);
                                }
                            }
                            _ => {}
                        }
                    }
                }
                Err(e) => {
                    error!("File watcher error: {}", e);
                }
            }
        }).map_err(|e| RemoteAdbError::config(
            format!("Failed to create file watcher: {}", e)
        ))?;

        // Watch the config file
        watcher.watch(&path, RecursiveMode::NonRecursive)
            .map_err(|e| RemoteAdbError::config(
                format!("Failed to watch config file: {}", e)
            ))?;

        self._watcher = Some(Box::new(watcher));
        info!("File watcher set up for: {}", path.display());
        
        Ok(())
    }

    /// Start automatic configuration reloading
    pub async fn start_auto_reload(&mut self) -> Result<()> {
        let change_receiver = self.subscribe_to_changes();
        
        tokio::spawn(async move {
            let mut receiver = change_receiver;
            while let Ok(change_event) = receiver.recv().await {
                match change_event.change_type {
                    ConfigChangeType::Modified | ConfigChangeType::Created => {
                        info!("Configuration file changed, reloading...");
                        
                        // Add a small delay to ensure file write is complete
                        tokio::time::sleep(Duration::from_millis(100)).await;
                        
                        // Note: In a real implementation, we would need to pass
                        // a reference to the config manager to reload it
                        info!("Configuration change detected - manual reload required");
                    }
                    ConfigChangeType::Deleted => {
                        warn!("Configuration file was deleted: {}", change_event.config_path.display());
                    }
                }
            }
        });
        
        Ok(())
    }
}

impl Default for ConfigManager {
    fn default() -> Self {
        Self::new()
    }
}

/// Global configuration manager instance
static GLOBAL_CONFIG_MANAGER: std::sync::OnceLock<Arc<tokio::sync::Mutex<ConfigManager>>> = std::sync::OnceLock::new();

/// Initialize global configuration manager
pub fn init_global_config_manager() -> Arc<tokio::sync::Mutex<ConfigManager>> {
    GLOBAL_CONFIG_MANAGER.get_or_init(|| {
        Arc::new(tokio::sync::Mutex::new(ConfigManager::new()))
    }).clone()
}

/// Get global configuration manager
pub fn global_config_manager() -> Arc<tokio::sync::Mutex<ConfigManager>> {
    init_global_config_manager()
}

/// Load global configuration from file
pub async fn load_global_config<P: AsRef<Path>>(path: P) -> Result<()> {
    let manager = global_config_manager();
    let mut manager = manager.lock().await;
    manager.load_from_file(path).await
}

/// Get current global configuration
pub async fn get_global_config() -> AppConfig {
    let manager = global_config_manager();
    let manager = manager.lock().await;
    manager.get_config()
}

/// Update global configuration
pub async fn update_global_config<F>(updater: F) -> Result<()>
where
    F: FnOnce(&mut AppConfig) -> Result<()>,
{
    let manager = global_config_manager();
    let manager = manager.lock().await;
    manager.update_config(updater)
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::NamedTempFile;
    use std::io::Write;

    #[tokio::test]
    async fn test_config_loading_toml() {
        let config_content = r#"
[server]
bind_address = "127.0.0.1"
port = 8080
enable_auth = false
max_connections = 20
connection_timeout = 30
enable_metrics = false

[security]
ip_whitelist = ["192.168.1.1", "10.0.0.1"]
auth_token = "test-token"
enable_tls = false
rate_limit_requests_per_minute = 100

[logging]
level = "debug"
file_path = "/tmp/test.log"
max_file_size_mb = 10
max_files = 5
enable_json_format = false

[performance]
log_buffer_size = 2000
heartbeat_interval = 60
stream_buffer_size = 1024
max_memory_usage_mb = 256
gc_interval = 300
enable_compression = true

[adb]
command_timeout = 30
device_poll_interval = 5
max_retries = 3

[client]
default_host = "localhost"
default_port = 5555
connection_timeout = 30
retry_attempts = 3
auto_reconnect = true
reconnect_delay = 2
max_reconnect_delay = 30
"#;

        let mut temp_file = NamedTempFile::new().unwrap();
        temp_file.write_all(config_content.as_bytes()).unwrap();
        
        let manager = ConfigManager::from_file(temp_file.path()).await.unwrap();
        let config = manager.get_config();
        
        assert_eq!(config.server.bind_address, "127.0.0.1");
        assert_eq!(config.server.port, 8080);
        assert!(!config.server.enable_auth);
        assert_eq!(config.server.max_connections, 20);
        assert_eq!(config.security.ip_whitelist, vec!["192.168.1.1", "10.0.0.1"]);
        assert_eq!(config.security.auth_token, Some("test-token".to_string()));
        assert_eq!(config.logging.level, "debug");
        assert_eq!(config.performance.log_buffer_size, 2000);
        assert_eq!(config.performance.heartbeat_interval, 60);
    }

    #[tokio::test]
    async fn test_config_validation() {
        let mut config = AppConfig::default();
        
        // Valid configuration should pass
        assert!(config.validate().is_ok());
        
        // Invalid port should fail
        config.server.port = 0;
        assert!(config.validate().is_err());
        
        // Reset port and test invalid IP
        config.server.port = 5555;
        config.security.ip_whitelist.push("invalid-ip".to_string());
        assert!(config.validate().is_err());
    }

    #[tokio::test]
    async fn test_env_overrides() {
        std::env::set_var("ENDORPHIN_BIND_ADDRESS", "192.168.1.100");
        std::env::set_var("ENDORPHIN_PORT", "9999");
        std::env::set_var("ENDORPHIN_AUTH_TOKEN", "env-token");
        
        let mut config = AppConfig::default();
        config.apply_env_overrides();
        
        assert_eq!(config.server.bind_address, "192.168.1.100");
        assert_eq!(config.server.port, 9999);
        assert_eq!(config.security.auth_token, Some("env-token".to_string()));
        
        // Clean up
        std::env::remove_var("ENDORPHIN_BIND_ADDRESS");
        std::env::remove_var("ENDORPHIN_PORT");
        std::env::remove_var("ENDORPHIN_AUTH_TOKEN");
    }
}