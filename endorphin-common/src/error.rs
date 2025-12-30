//! Error handling for the remote ADB system

use thiserror::Error;
use tracing::{error, warn, info};
use serde::{Serialize, Deserialize};
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::{SystemTime, UNIX_EPOCH};

/// Result type alias for the remote ADB system
pub type Result<T> = std::result::Result<T, RemoteAdbError>;

/// Main error type for the remote ADB system
#[derive(Error, Debug, Clone, Serialize, Deserialize)]
pub enum RemoteAdbError {
    #[error("Network connection error: {0}")]
    NetworkError(String),

    #[error("ADB command failed: {0}")]
    AdbError(String),

    #[error("Authentication failed: {0}")]
    AuthenticationError(String),

    #[error("Device not found: {device_id}")]
    DeviceNotFound { device_id: String },

    #[error("Configuration error: {0}")]
    ConfigError(String),

    #[error("IO error: {0}")]
    IoError(String),

    #[error("Serialization error: {0}")]
    SerializationError(String),

    #[error("Connection timeout")]
    Timeout,

    #[error("Invalid request: {0}")]
    InvalidRequest(String),

    #[error("Server error: {0}")]
    ServerError(String),

    #[error("Client error: {0}")]
    ClientError(String),

    #[error("Resource exhaustion: {0}")]
    ResourceExhaustion(String),

    #[error("Service unavailable: {0}")]
    ServiceUnavailable(String),

    #[error("Recovery failed: {0}")]
    RecoveryFailed(String),

    #[error("General error: {0}")]
    GeneralError(String),
}

impl RemoteAdbError {
    /// Create a new network error
    pub fn network<S: Into<String>>(msg: S) -> Self {
        Self::NetworkError(msg.into())
    }

    /// Create a new ADB error
    pub fn adb<S: Into<String>>(msg: S) -> Self {
        Self::AdbError(msg.into())
    }

    /// Create a new authentication error
    pub fn auth<S: Into<String>>(msg: S) -> Self {
        Self::AuthenticationError(msg.into())
    }

    /// Create a new configuration error
    pub fn config<S: Into<String>>(msg: S) -> Self {
        Self::ConfigError(msg.into())
    }

    /// Create a new server error
    pub fn server<S: Into<String>>(msg: S) -> Self {
        Self::ServerError(msg.into())
    }

    /// Create a new client error
    pub fn client<S: Into<String>>(msg: S) -> Self {
        Self::ClientError(msg.into())
    }

    /// Create a new resource exhaustion error
    pub fn resource_exhaustion<S: Into<String>>(msg: S) -> Self {
        Self::ResourceExhaustion(msg.into())
    }

    /// Create a new service unavailable error
    pub fn service_unavailable<S: Into<String>>(msg: S) -> Self {
        Self::ServiceUnavailable(msg.into())
    }

    /// Create a new recovery failed error
    pub fn recovery_failed<S: Into<String>>(msg: S) -> Self {
        Self::RecoveryFailed(msg.into())
    }

    /// Get the error category for classification
    pub fn category(&self) -> ErrorCategory {
        match self {
            Self::NetworkError(_) | Self::Timeout => ErrorCategory::Network,
            Self::AdbError(_) | Self::DeviceNotFound { .. } => ErrorCategory::Device,
            Self::AuthenticationError(_) => ErrorCategory::Security,
            Self::ConfigError(_) => ErrorCategory::Configuration,
            Self::IoError(_) | Self::SerializationError(_) => ErrorCategory::System,
            Self::ResourceExhaustion(_) => ErrorCategory::Resource,
            Self::ServiceUnavailable(_) => ErrorCategory::Service,
            Self::RecoveryFailed(_) => ErrorCategory::Recovery,
            Self::InvalidRequest(_) | Self::ServerError(_) | Self::ClientError(_) => ErrorCategory::Application,
            Self::GeneralError(_) => ErrorCategory::Unknown,
        }
    }

    /// Check if the error is recoverable
    pub fn is_recoverable(&self) -> bool {
        match self {
            Self::NetworkError(_) | Self::Timeout | Self::ServiceUnavailable(_) => true,
            Self::DeviceNotFound { .. } => true, // Device might come back online
            Self::ResourceExhaustion(_) => true, // Resources might be freed
            Self::AdbError(_) => false, // Usually indicates a permanent issue
            Self::AuthenticationError(_) => false, // Requires user intervention
            Self::ConfigError(_) => false, // Requires configuration fix
            Self::IoError(_) | Self::SerializationError(_) => false, // System issues
            Self::InvalidRequest(_) | Self::ServerError(_) | Self::ClientError(_) => false,
            Self::RecoveryFailed(_) => false, // Recovery already attempted
            Self::GeneralError(_) => false,
        }
    }

    /// Get suggested recovery action
    pub fn recovery_suggestion(&self) -> Option<String> {
        match self {
            Self::NetworkError(_) | Self::Timeout => {
                Some("Check network connectivity and retry connection".to_string())
            }
            Self::DeviceNotFound { device_id } => {
                Some(format!("Ensure device {} is connected and ADB is enabled", device_id))
            }
            Self::ServiceUnavailable(_) => {
                Some("Wait for service to become available and retry".to_string())
            }
            Self::ResourceExhaustion(_) => {
                Some("Free up system resources and retry operation".to_string())
            }
            Self::AuthenticationError(_) => {
                Some("Check authentication credentials and permissions".to_string())
            }
            Self::ConfigError(_) => {
                Some("Review and correct configuration settings".to_string())
            }
            Self::AdbError(_) => {
                Some("Check ADB server status and device connection".to_string())
            }
            _ => None,
        }
    }
}

// Convert from std::io::Error
impl From<std::io::Error> for RemoteAdbError {
    fn from(err: std::io::Error) -> Self {
        Self::IoError(err.to_string())
    }
}

// Convert from serde_json::Error
impl From<serde_json::Error> for RemoteAdbError {
    fn from(err: serde_json::Error) -> Self {
        Self::SerializationError(err.to_string())
    }
}

// Convert from anyhow::Error
impl From<anyhow::Error> for RemoteAdbError {
    fn from(err: anyhow::Error) -> Self {
        Self::GeneralError(err.to_string())
    }
}

/// Error categories for classification and handling
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum ErrorCategory {
    Network,
    Device,
    Security,
    Configuration,
    System,
    Resource,
    Service,
    Recovery,
    Application,
    Unknown,
}

/// Error severity levels
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub enum ErrorSeverity {
    Low,
    Medium,
    High,
    Critical,
}

/// Error report for logging and monitoring
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ErrorReport {
    pub error: RemoteAdbError,
    pub category: ErrorCategory,
    pub severity: ErrorSeverity,
    pub timestamp: u64,
    pub context: HashMap<String, String>,
    pub recovery_attempted: bool,
    pub recovery_successful: Option<bool>,
}

impl ErrorReport {
    pub fn new(error: RemoteAdbError) -> Self {
        let severity = match error.category() {
            ErrorCategory::Security | ErrorCategory::Configuration => ErrorSeverity::High,
            ErrorCategory::Network | ErrorCategory::Device => ErrorSeverity::Medium,
            ErrorCategory::Resource | ErrorCategory::Service => ErrorSeverity::Medium,
            ErrorCategory::Recovery => ErrorSeverity::Critical,
            _ => ErrorSeverity::Low,
        };

        Self {
            category: error.category(),
            error,
            severity,
            timestamp: SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs(),
            context: HashMap::new(),
            recovery_attempted: false,
            recovery_successful: None,
        }
    }

    pub fn with_context<K: Into<String>, V: Into<String>>(mut self, key: K, value: V) -> Self {
        self.context.insert(key.into(), value.into());
        self
    }

    pub fn mark_recovery_attempted(mut self) -> Self {
        self.recovery_attempted = true;
        self
    }

    pub fn mark_recovery_result(mut self, successful: bool) -> Self {
        self.recovery_successful = Some(successful);
        self
    }
}

/// Global error handler for centralized error processing
#[derive(Debug)]
pub struct GlobalErrorHandler {
    error_stats: Arc<Mutex<HashMap<ErrorCategory, u64>>>,
    recent_errors: Arc<Mutex<Vec<ErrorReport>>>,
    max_recent_errors: usize,
}

impl Default for GlobalErrorHandler {
    fn default() -> Self {
        Self::new(100)
    }
}

impl GlobalErrorHandler {
    pub fn new(max_recent_errors: usize) -> Self {
        Self {
            error_stats: Arc::new(Mutex::new(HashMap::new())),
            recent_errors: Arc::new(Mutex::new(Vec::new())),
            max_recent_errors,
        }
    }

    /// Handle an error with full reporting and logging
    pub fn handle_error(&self, error: RemoteAdbError) -> ErrorReport {
        let report = ErrorReport::new(error);
        
        // Log the error based on severity
        match report.severity {
            ErrorSeverity::Critical => {
                error!(
                    error = %report.error,
                    category = ?report.category,
                    "Critical error occurred"
                );
            }
            ErrorSeverity::High => {
                error!(
                    error = %report.error,
                    category = ?report.category,
                    "High severity error occurred"
                );
            }
            ErrorSeverity::Medium => {
                warn!(
                    error = %report.error,
                    category = ?report.category,
                    "Medium severity error occurred"
                );
            }
            ErrorSeverity::Low => {
                info!(
                    error = %report.error,
                    category = ?report.category,
                    "Low severity error occurred"
                );
            }
        }

        // Update statistics
        if let Ok(mut stats) = self.error_stats.lock() {
            *stats.entry(report.category).or_insert(0) += 1;
        }

        // Store in recent errors
        if let Ok(mut recent) = self.recent_errors.lock() {
            recent.push(report.clone());
            if recent.len() > self.max_recent_errors {
                recent.remove(0);
            }
        }

        report
    }

    /// Handle an error with additional context
    pub fn handle_error_with_context(
        &self,
        error: RemoteAdbError,
        context: HashMap<String, String>,
    ) -> ErrorReport {
        let mut report = self.handle_error(error);
        report.context = context;
        report
    }

    /// Get error statistics by category
    pub fn get_error_stats(&self) -> HashMap<ErrorCategory, u64> {
        match self.error_stats.lock() {
            Ok(stats) => stats.clone(),
            Err(_) => {
                error!("Failed to acquire error stats lock");
                HashMap::new()
            }
        }
    }

    /// Get recent error reports
    pub fn get_recent_errors(&self) -> Vec<ErrorReport> {
        match self.recent_errors.lock() {
            Ok(recent) => recent.clone(),
            Err(_) => {
                error!("Failed to acquire recent errors lock");
                Vec::new()
            }
        }
    }

    /// Clear error statistics
    pub fn clear_stats(&self) {
        if let Ok(mut stats) = self.error_stats.lock() {
            stats.clear();
        }
        if let Ok(mut recent) = self.recent_errors.lock() {
            recent.clear();
        }
    }
}

/// Global error handler instance
static GLOBAL_ERROR_HANDLER: std::sync::OnceLock<GlobalErrorHandler> = std::sync::OnceLock::new();

/// Get the global error handler instance
pub fn global_error_handler() -> &'static GlobalErrorHandler {
    GLOBAL_ERROR_HANDLER.get_or_init(GlobalErrorHandler::default)
}

/// Initialize the global error handler with custom configuration
pub fn init_global_error_handler(max_recent_errors: usize) {
    let _ = GLOBAL_ERROR_HANDLER.set(GlobalErrorHandler::new(max_recent_errors));
}

/// Convenience function to handle errors globally
pub fn handle_error(error: RemoteAdbError) -> ErrorReport {
    global_error_handler().handle_error(error)
}

/// Convenience function to handle errors with context
pub fn handle_error_with_context(
    error: RemoteAdbError,
    context: HashMap<String, String>,
) -> ErrorReport {
    global_error_handler().handle_error_with_context(error, context)
}