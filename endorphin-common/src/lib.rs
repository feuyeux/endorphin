//! Remote ADB Common Library
//! 
//! This library contains shared types, utilities, and error handling
//! for the remote ADB logging system.

pub mod adb;
pub mod buffer;
pub mod config;
pub mod error;
pub mod filter;
pub mod logcat;
pub mod logging;
pub mod monitoring;
pub mod network;
pub mod optimization;
pub mod performance;
pub mod security;
pub mod types;

pub use adb::AdbExecutor;
pub use buffer::{LogBuffer, BufferConfig, BufferStats, MultiBufferManager};
pub use config::{
    AppConfig, ServerConfig, SecurityConfig, LoggingConfig, AdbConfig, PerformanceConfig, ClientConfig,
    ConfigManager, ConfigChangeEvent, ConfigChangeType, ConfigFormat,
    init_global_config_manager, global_config_manager, load_global_config, 
    get_global_config, update_global_config
};
pub use error::{Result, RemoteAdbError, ErrorCategory, ErrorSeverity, ErrorReport, GlobalErrorHandler, global_error_handler, init_global_error_handler, handle_error, handle_error_with_context};
pub use filter::{LogFilterEngine, AdvancedLogFilters};
pub use logcat::{LogcatManager, LogEntryStream};
pub use logging::init_logging;
pub use monitoring::{
    ComponentType, SystemStatus, ComponentStatus, StatusChangeEvent, NotificationType,
    Notification, NotificationSeverity, SystemStatusMonitor, global_status_monitor,
    init_global_status_monitor, update_component_status, send_notification, get_system_status,
    helpers,
};
pub use optimization::{
    MemoryOptimizer, CpuOptimizer, NetworkOptimizer, SystemOptimizer
};
pub use performance::{
    PerformanceMetrics, PerformanceAlert, PerformanceAlertEvent, AlertSeverity,
    PerformanceThresholds, PerformanceCounter, PerformanceMonitor, PerformanceReport,
    init_global_performance_monitor, global_performance_monitor, global_performance_counter,
    start_global_performance_monitoring, get_global_performance_metrics
};
pub use types::*;