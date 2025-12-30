//! Remote ADB Server
//! 
//! This is the server application that provides remote access to local ADB
//! and streams Android device logs to connected clients.

mod bridge;
mod stream_manager;
mod emulator;

use bridge::{NetworkBridge, BridgeConfig};
use emulator::EmulatorManager;
use clap::Parser;
use endorphin_common::{
    Result, init_global_status_monitor, helpers, SystemStatus,
    global_config_manager, load_global_config, get_global_config, AppConfig,
    PerformanceThresholds, init_global_performance_monitor, start_global_performance_monitoring
};
use std::net::IpAddr;
use std::time::Duration;
use tracing::{error, info, warn};

#[derive(Parser)]
#[command(name = "remote-adb-server")]
#[command(about = "Remote ADB server for sharing Android device access")]
#[command(version = "0.1.0")]
struct Cli {
    /// Verbose logging
    #[arg(short, long)]
    verbose: bool,

    /// Bind address for the server
    #[arg(short = 'b', long)]
    bind: Option<String>,

    /// Port to listen on
    #[arg(short, long)]
    port: Option<u16>,

    /// Configuration file path
    #[arg(short, long, default_value = "config.toml")]
    config: String,

    /// Enable authentication
    #[arg(long)]
    auth: Option<bool>,

    /// Authentication token
    #[arg(long)]
    token: Option<String>,

    /// IP whitelist (comma-separated)
    #[arg(long)]
    whitelist: Option<String>,

    /// Maximum number of concurrent connections
    #[arg(long)]
    max_connections: Option<usize>,

    /// Enable hot reloading of configuration
    #[arg(long, default_value = "true")]
    hot_reload: bool,

    /// Skip emulator check and startup
    #[arg(long)]
    skip_emulator: bool,

    /// Specific AVD name to start (if not running)
    #[arg(long)]
    avd_name: Option<String>,

    /// Emulator startup timeout in seconds
    #[arg(long, default_value = "120")]
    emulator_timeout: u64,
}

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();

    // Initialize logging with basic level first
    let initial_log_level = if cli.verbose { "debug" } else { "info" };
    init_logging_with_level(initial_log_level)?;

    info!("Starting Remote ADB Server v0.1.0");

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
    let mut config = get_global_config().await;

    // Apply CLI overrides
    apply_cli_overrides(&mut config, &cli);

    // Re-initialize logging with the configuration level
    init_logging_with_level(&config.logging.level)?;

    // Initialize global status monitoring
    init_global_status_monitor(Duration::from_secs(config.performance.heartbeat_interval));

    // Check and start Android emulator if needed
    if !cli.skip_emulator {
        info!("🔍 Checking Android emulator status...");
        let emulator_manager = EmulatorManager::with_timeout(Duration::from_secs(cli.emulator_timeout));
        
        match emulator_manager.ensure_emulator_running(cli.avd_name.clone()).await {
            Ok(status) => {
                info!("✅ Android emulator is ready");
                if let Some(device_id) = &status.device_id {
                    info!("Connected to device: {}", device_id);
                }
                if let Some(android_version) = &status.android_version {
                    info!("Android version: {}", android_version);
                }
            }
            Err(e) => {
                error!("❌ Failed to ensure emulator is running: {}", e);
                error!("💡 Suggestions:");
                error!("  1. Install Android SDK and set ANDROID_HOME environment variable");
                error!("  2. Create an AVD using Android Studio or avdmanager");
                error!("  3. Use --skip-emulator flag to bypass emulator check");
                error!("  4. Use --avd-name <name> to specify a specific AVD");
                return Err(e);
            }
        }
    } else {
        warn!("⚠️  Skipping emulator check (--skip-emulator flag used)");
        warn!("Make sure an Android device or emulator is connected manually");
    }

    // Initialize performance monitoring
    let perf_thresholds = PerformanceThresholds {
        cpu_warning_percent: 70.0,
        cpu_critical_percent: 90.0,
        memory_warning_percent: 80.0,
        memory_critical_percent: 95.0,
        response_time_warning_ms: 1000.0,
        response_time_critical_ms: 5000.0,
        error_rate_warning_per_minute: 10,
        error_rate_critical_per_minute: 50,
    };
    
    init_global_performance_monitor(perf_thresholds);
    
    // Start performance monitoring if enabled
    if config.server.enable_metrics {
        info!("Starting performance monitoring");
        if let Err(e) = start_global_performance_monitoring(Duration::from_secs(30)).await {
            warn!("Failed to start performance monitoring: {}", e);
        } else {
            info!("Performance monitoring started");
        }
    }

    info!("Binding to {}:{}", config.server.bind_address, config.server.port);

    // Report server startup
    helpers::report_network_bridge_status(
        SystemStatus::Healthy,
        "Remote ADB Server starting up".to_string(),
    )?;

    // Parse IP whitelist from configuration
    let mut ip_whitelist = Vec::new();
    for ip_str in &config.security.ip_whitelist {
        match ip_str.parse::<IpAddr>() {
            Ok(ip) => {
                ip_whitelist.push(ip);
                info!("Added {} to IP whitelist", ip);
            }
            Err(e) => {
                error!("Invalid IP address '{}': {}", ip_str, e);
                return Err(endorphin_common::RemoteAdbError::config(
                    format!("Invalid IP address in whitelist: {}", ip_str)
                ));
            }
        }
    }

    // Create bridge configuration from app config
    let bridge_config = BridgeConfig {
        bind_address: config.server.bind_address.clone(),
        port: config.server.port,
        max_connections: config.server.max_connections,
        enable_auth: config.server.enable_auth,
        auth_token: config.security.auth_token.clone(),
        ip_whitelist,
    };

    if bridge_config.enable_auth {
        if bridge_config.auth_token.is_some() {
            info!("Authentication enabled");
        } else {
            error!("Authentication enabled but no token provided in configuration");
            return Err(endorphin_common::RemoteAdbError::config(
                "Authentication enabled but no token provided"
            ));
        }
    }

    // Set up configuration hot reloading if enabled
    if cli.hot_reload {
        info!("Setting up configuration hot reloading");
        let config_manager = global_config_manager();
        let mut manager = config_manager.lock().await;
        if let Err(e) = manager.start_auto_reload().await {
            warn!("Failed to set up configuration hot reloading: {}", e);
        } else {
            info!("Configuration hot reloading enabled");
        }
    }

    // Create and start the network bridge
    let mut bridge = NetworkBridge::new(bridge_config);
    
    info!("Network Bridge Server initialized");
    info!("Maximum connections: {}", config.server.max_connections);
    info!("Performance settings:");
    info!("  - Log buffer size: {}", config.performance.log_buffer_size);
    info!("  - Stream buffer size: {}", config.performance.stream_buffer_size);
    info!("  - Max memory usage: {} MB", config.performance.max_memory_usage_mb);
    info!("  - Compression enabled: {}", config.performance.enable_compression);
    
    // Report server ready
    helpers::report_network_bridge_status(
        SystemStatus::Healthy,
        "Remote ADB Server ready to accept connections".to_string(),
    )?;

    // Start health monitoring
    endorphin_common::global_status_monitor().start_health_monitoring().await?;
    
    // Set up graceful shutdown
    let shutdown_result = tokio::select! {
        result = bridge.start() => {
            match result {
                Ok(()) => {
                    info!("Bridge server completed normally");
                    Ok(())
                }
                Err(e) => {
                    error!("Bridge server error: {}", e);
                    Err(e)
                }
            }
        }
        _ = tokio::signal::ctrl_c() => {
            info!("Received shutdown signal");
            helpers::report_network_bridge_status(
                SystemStatus::Offline,
                "Remote ADB Server shutting down".to_string(),
            ).unwrap_or_else(|e| error!("Failed to report shutdown status: {}", e));
            bridge.stop().await?;
            Ok(())
        }
    };

    info!("Remote ADB Server shutdown complete");
    shutdown_result
}

/// Apply CLI argument overrides to configuration
fn apply_cli_overrides(config: &mut AppConfig, cli: &Cli) {
    if let Some(bind) = &cli.bind {
        config.server.bind_address = bind.clone();
    }
    
    if let Some(port) = cli.port {
        config.server.port = port;
    }
    
    if let Some(auth) = cli.auth {
        config.server.enable_auth = auth;
    }
    
    if let Some(token) = &cli.token {
        config.security.auth_token = Some(token.clone());
    }
    
    if let Some(max_conn) = cli.max_connections {
        config.server.max_connections = max_conn;
    }
    
    if let Some(whitelist) = &cli.whitelist {
        config.security.ip_whitelist = whitelist
            .split(',')
            .map(|s| s.trim().to_string())
            .collect();
    }
}

// Re-export init_logging_with_level from common for use in main
use endorphin_common::logging::init_logging_with_level;