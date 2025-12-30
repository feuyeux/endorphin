//! Remote ADB Client
//! 
//! This is the client application that connects to a remote ADB server
//! to monitor Android device logs.

use clap::{Parser, Subcommand};
use endorphin_common::{
    Result, ConnectionConfig, LogFilters, LogLevel, RemoteAdbError, 
    init_global_status_monitor,
    load_global_config, get_global_config,
    PerformanceThresholds, init_global_performance_monitor, start_global_performance_monitoring
};
use endorphin_client::{RemoteConnectionManager, ReconnectPolicy, SimpleLogViewer, UiConfig, DisplayFilter};
use std::time::Duration;
use tracing::{info, error, warn};

#[derive(Parser)]
#[command(name = "endorphin-client")]
#[command(about = "Remote ADB client for monitoring Android device logs")]
#[command(version = "0.1.0")]
struct Cli {
    /// Verbose logging
    #[arg(short, long)]
    verbose: bool,

    /// Server host to connect to
    #[arg(short = 'H', long)]
    host: Option<String>,

    /// Server port to connect to
    #[arg(short, long)]
    port: Option<u16>,

    /// Authentication token
    #[arg(short, long)]
    auth_token: Option<String>,

    /// Configuration file path
    #[arg(short, long, default_value = "config.toml")]
    config: String,

    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Connect to remote ADB server
    Connect {
        /// Device ID to monitor (optional)
        device_id: Option<String>,
    },
    /// List available devices
    Devices,
    /// Monitor logs from a specific device
    Logs {
        /// Device ID to monitor
        device_id: String,
        /// Log level filter
        #[arg(short, long)]
        level: Option<String>,
        /// Tag filter
        #[arg(short, long)]
        tag: Option<String>,
    },
}

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();

    // Initialize logging with basic level first
    let initial_log_level = if cli.verbose { "debug" } else { "info" };
    init_logging_with_level(initial_log_level)?;

    info!("Starting Remote ADB Client v0.1.0");

    // Load configuration from file
    if std::path::Path::new(&cli.config).exists() {
        info!("Loading configuration from: {}", cli.config);
        if let Err(e) = load_global_config(&cli.config).await {
            error!("Failed to load configuration: {}", e);
            warn!("Using default configuration");
        }
    } else {
        warn!("Configuration file '{}' not found, using defaults", cli.config);
    }

    // Get the loaded configuration
    let config = get_global_config().await;

    // Re-initialize logging with the configuration level
    init_logging_with_level(&config.logging.level)?;

    // Initialize global status monitoring
    init_global_status_monitor(Duration::from_secs(60)); // 1-minute health checks

    // Initialize performance monitoring for client
    let perf_thresholds = PerformanceThresholds {
        cpu_warning_percent: 60.0,  // Lower thresholds for client
        cpu_critical_percent: 80.0,
        memory_warning_percent: 70.0,
        memory_critical_percent: 90.0,
        response_time_warning_ms: 2000.0,
        response_time_critical_ms: 10000.0,
        error_rate_warning_per_minute: 5,
        error_rate_critical_per_minute: 20,
    };
    
    init_global_performance_monitor(perf_thresholds);
    
    // Start performance monitoring
    if let Err(e) = start_global_performance_monitoring(Duration::from_secs(60)).await {
        warn!("Failed to start performance monitoring: {}", e);
    }

    // Determine connection parameters (CLI overrides config)
    let host = cli.host.unwrap_or(config.client.default_host);
    let port = cli.port.unwrap_or(config.client.default_port);
    let auth_token = cli.auth_token.or(config.security.auth_token)
        .unwrap_or_else(|| "default-token".to_string());

    info!("Connecting to {}:{}", host, port);

    // Create connection configuration
    let connection_config = ConnectionConfig {
        host: host.clone(),
        port,
        auth_token,
        timeout_seconds: config.client.connection_timeout,
        retry_attempts: config.client.retry_attempts,
    };

    // Create connection manager
    let mut connection_manager = RemoteConnectionManager::new(connection_config);
    
    // Set up reconnection policy from configuration
    let reconnect_policy = ReconnectPolicy {
        max_attempts: config.client.retry_attempts,
        initial_delay: Duration::from_secs(config.client.reconnect_delay),
        max_delay: Duration::from_secs(config.client.max_reconnect_delay),
        backoff_multiplier: 2.0,
        auto_reconnect: config.client.auto_reconnect,
    };
    connection_manager.set_reconnect_policy(reconnect_policy).await;

    match cli.command {
        Commands::Connect { device_id } => {
            info!("Connecting to remote ADB server...");
            
            match connection_manager.connect().await {
                Ok(()) => {
                    info!("Successfully connected to remote ADB server");
                    
                    // Start auto-reconnect monitoring if enabled
                    if config.client.auto_reconnect {
                        connection_manager.start_auto_reconnect().await;
                    }
                    
                    if let Some(device) = device_id {
                        info!("Target device: {}", device);
                        // TODO: Start monitoring specific device
                    } else {
                        // List available devices
                        match connection_manager.list_devices().await {
                            Ok(devices) => {
                                info!("Available devices:");
                                for device in devices {
                                    info!("  - {} ({}): {} API {}", 
                                          device.device_id, 
                                          device.device_name,
                                          device.android_version,
                                          device.api_level);
                                }
                            }
                            Err(e) => {
                                error!("Failed to list devices: {}", e);
                            }
                        }
                    }
                    
                    // Keep connection alive
                    info!("Connection established. Press Ctrl+C to exit.");
                    tokio::signal::ctrl_c().await.expect("Failed to listen for ctrl+c");
                    info!("Shutting down...");
                }
                Err(e) => {
                    error!("Failed to connect: {}", e);
                    return Err(e);
                }
            }
        }
        Commands::Devices => {
            info!("Listing available devices...");
            
            match connection_manager.connect().await {
                Ok(()) => {
                    match connection_manager.list_devices().await {
                        Ok(devices) => {
                            println!("Available devices:");
                            for device in devices {
                                println!("  {} - {} ({}) - API {} - Status: {:?}", 
                                        device.device_id,
                                        device.device_name,
                                        device.android_version,
                                        device.api_level,
                                        device.status);
                            }
                        }
                        Err(e) => {
                            error!("Failed to list devices: {}", e);
                            return Err(e);
                        }
                    }
                }
                Err(e) => {
                    error!("Failed to connect: {}", e);
                    return Err(e);
                }
            }
        }
        Commands::Logs { device_id, level, tag } => {
            info!("Starting log monitoring for device: {}", device_id);
            
            // Parse log level
            let log_level = if let Some(level_str) = level {
                match level_str.to_lowercase().as_str() {
                    "verbose" | "v" => Some(LogLevel::Verbose),
                    "debug" | "d" => Some(LogLevel::Debug),
                    "info" | "i" => Some(LogLevel::Info),
                    "warn" | "w" => Some(LogLevel::Warn),
                    "error" | "e" => Some(LogLevel::Error),
                    "fatal" | "f" => Some(LogLevel::Fatal),
                    _ => {
                        warn!("Unknown log level '{}', using default", level_str);
                        None
                    }
                }
            } else {
                None
            };

            // Create log filters
            let filters = LogFilters {
                log_level: log_level.clone(),
                tag_filter: tag.clone(),
                package_filter: None,
                regex_pattern: None,
                max_lines: Some(config.performance.log_buffer_size), // Use config buffer size
            };

            // Create UI configuration
            let ui_config = UiConfig {
                use_colors: true,
                show_timestamps: true,
                show_pids: true,
                max_line_length: Some(120),
                auto_scroll: true,
            };

            // Create display filter
            let display_filter = DisplayFilter {
                min_level: log_level,
                tag_filter: tag,
                message_filter: None,
                pid_filter: None,
            };

            // Create simple log viewer
            let mut log_viewer = SimpleLogViewer::new(ui_config);
            log_viewer.set_filter(display_filter);

            match connection_manager.connect().await {
                Ok(()) => {
                    info!("Connected to server, starting log monitoring...");
                    
                    // Get device info first
                    let devices = connection_manager.list_devices().await?;
                    let target_device = devices.iter()
                        .find(|d| d.device_id == device_id)
                        .ok_or_else(|| RemoteAdbError::client(format!("Device {} not found", device_id)))?;

                    // Print device header
                    log_viewer.print_device_header(target_device)?;
                    
                    match connection_manager.start_log_monitoring(&device_id, filters).await {
                        Ok(stream_handle) => {
                            info!("Started log monitoring with stream ID: {}", stream_handle.stream_id);
                            println!("Monitoring logs for device {}. Press Ctrl+C to stop.", device_id);
                            println!("{}",  "=".repeat(50));
                            
                            // Monitor logs until interrupted
                            loop {
                                tokio::select! {
                                    result = connection_manager.receive_log_entries() => {
                                        match result {
                                            Ok(log_entries) => {
                                                if !log_entries.is_empty() {
                                                    if let Err(e) = log_viewer.display_entries(&log_entries) {
                                                        error!("Error displaying log entries: {}", e);
                                                    }
                                                }
                                            }
                                            Err(e) => {
                                                error!("Error receiving log entries: {}", e);
                                                break;
                                            }
                                        }
                                    }
                                    _ = tokio::signal::ctrl_c() => {
                                        info!("Stopping log monitoring...");
                                        if let Err(e) = connection_manager.stop_log_monitoring(&stream_handle).await {
                                            warn!("Error stopping log monitoring: {}", e);
                                        }
                                        break;
                                    }
                                }
                            }
                        }
                        Err(e) => {
                            error!("Failed to start log monitoring: {}", e);
                            return Err(e);
                        }
                    }
                }
                Err(e) => {
                    error!("Failed to connect: {}", e);
                    return Err(e);
                }
            }
        }
    }

    // Shutdown connection manager
    connection_manager.shutdown().await?;
    Ok(())
}

// Re-export init_logging_with_level from common for use in main
use endorphin_common::logging::init_logging_with_level;