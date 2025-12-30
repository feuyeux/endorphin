//! Logging configuration for the remote ADB system

use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt, EnvFilter};

/// Initialize logging for the application
/// 
/// This sets up structured logging with tracing, using environment variables
/// for configuration. The default log level is INFO, but can be overridden
/// with the RUST_LOG environment variable.
pub fn init_logging() -> anyhow::Result<()> {
    let filter = EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| EnvFilter::new("info"));

    tracing_subscriber::registry()
        .with(filter)
        .with(
            tracing_subscriber::fmt::layer()
                .with_target(true)
                .with_thread_ids(true)
                .with_file(true)
                .with_line_number(true)
        )
        .try_init()
        .map_err(|e| anyhow::anyhow!("Failed to initialize logging: {}", e))?;

    tracing::info!("Logging initialized");
    Ok(())
}

/// Initialize logging with a specific log level
pub fn init_logging_with_level(level: &str) -> anyhow::Result<()> {
    let filter = EnvFilter::new(level);

    tracing_subscriber::registry()
        .with(filter)
        .with(
            tracing_subscriber::fmt::layer()
                .with_target(true)
                .with_thread_ids(true)
                .with_file(true)
                .with_line_number(true)
        )
        .try_init()
        .map_err(|e| anyhow::anyhow!("Failed to initialize logging: {}", e))?;

    tracing::info!("Logging initialized with level: {}", level);
    Ok(())
}