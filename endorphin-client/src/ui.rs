//! Terminal UI for log display
//!
//! This module provides a terminal-based user interface for displaying
//! Android device logs in real-time with filtering and search capabilities.

use endorphin_common::{LogEntry, LogLevel, DeviceInfo, StreamHandle};
use std::collections::VecDeque;
use std::io::{self, Write};
use std::time::UNIX_EPOCH;
use tokio::sync::mpsc;
use tracing::info;

/// Maximum number of log entries to keep in memory
const MAX_LOG_ENTRIES: usize = 10000;

/// Color codes for different log levels
const COLOR_VERBOSE: &str = "\x1b[37m"; // White
const COLOR_DEBUG: &str = "\x1b[36m";   // Cyan
const COLOR_INFO: &str = "\x1b[32m";    // Green
const COLOR_WARN: &str = "\x1b[33m";    // Yellow
const COLOR_ERROR: &str = "\x1b[31m";   // Red
const COLOR_FATAL: &str = "\x1b[35m";   // Magenta
const COLOR_RESET: &str = "\x1b[0m";    // Reset

/// Terminal UI configuration
#[derive(Debug, Clone)]
pub struct UiConfig {
    /// Whether to use colors in output
    pub use_colors: bool,
    /// Whether to show timestamps
    pub show_timestamps: bool,
    /// Whether to show process/thread IDs
    pub show_pids: bool,
    /// Maximum line length before truncation
    pub max_line_length: Option<usize>,
    /// Whether to auto-scroll to new entries
    pub auto_scroll: bool,
}

impl Default for UiConfig {
    fn default() -> Self {
        Self {
            use_colors: true,
            show_timestamps: true,
            show_pids: true,
            max_line_length: Some(120),
            auto_scroll: true,
        }
    }
}

/// Log display filter for the UI
#[derive(Debug, Clone, Default)]
pub struct DisplayFilter {
    /// Minimum log level to display
    pub min_level: Option<LogLevel>,
    /// Tag filter (substring match)
    pub tag_filter: Option<String>,
    /// Message filter (substring match)
    pub message_filter: Option<String>,
    /// Process ID filter
    pub pid_filter: Option<u32>,
}

impl DisplayFilter {
    /// Check if a log entry matches this filter
    pub fn matches(&self, entry: &LogEntry) -> bool {
        // Check minimum log level
        if let Some(min_level) = &self.min_level {
            if entry.level < *min_level {
                return false;
            }
        }

        // Check tag filter
        if let Some(tag_filter) = &self.tag_filter {
            if !entry.tag.to_lowercase().contains(&tag_filter.to_lowercase()) {
                return false;
            }
        }

        // Check message filter
        if let Some(message_filter) = &self.message_filter {
            if !entry.message.to_lowercase().contains(&message_filter.to_lowercase()) {
                return false;
            }
        }

        // Check PID filter
        if let Some(pid_filter) = &self.pid_filter {
            if entry.process_id != *pid_filter {
                return false;
            }
        }

        true
    }
}

/// Statistics about displayed logs
#[derive(Debug, Clone, Default)]
pub struct LogStats {
    pub total_entries: usize,
    pub filtered_entries: usize,
    pub verbose_count: usize,
    pub debug_count: usize,
    pub info_count: usize,
    pub warn_count: usize,
    pub error_count: usize,
    pub fatal_count: usize,
}

impl LogStats {
    /// Update statistics with a new log entry
    pub fn update(&mut self, entry: &LogEntry, is_filtered: bool) {
        self.total_entries += 1;
        
        if !is_filtered {
            self.filtered_entries += 1;
            
            match entry.level {
                LogLevel::Verbose => self.verbose_count += 1,
                LogLevel::Debug => self.debug_count += 1,
                LogLevel::Info => self.info_count += 1,
                LogLevel::Warn => self.warn_count += 1,
                LogLevel::Error => self.error_count += 1,
                LogLevel::Fatal => self.fatal_count += 1,
            }
        }
    }
}

/// Terminal UI for displaying logs
pub struct TerminalUi {
    config: UiConfig,
    filter: DisplayFilter,
    log_buffer: VecDeque<LogEntry>,
    stats: LogStats,
    current_device: Option<DeviceInfo>,
    current_stream: Option<StreamHandle>,
    command_tx: mpsc::Sender<UiCommand>,
    command_rx: Option<mpsc::Receiver<UiCommand>>,
}

/// Commands that can be sent to the UI
#[derive(Debug, Clone)]
pub enum UiCommand {
    /// Add new log entries
    AddLogEntries(Vec<LogEntry>),
    /// Update current device info
    SetDevice(DeviceInfo),
    /// Update current stream handle
    SetStream(StreamHandle),
    /// Update display filter
    SetFilter(DisplayFilter),
    /// Update UI configuration
    SetConfig(UiConfig),
    /// Clear log buffer
    Clear,
    /// Export logs to file
    Export(String),
    /// Show help
    ShowHelp,
    /// Show statistics
    ShowStats,
    /// Quit the UI
    Quit,
}

impl TerminalUi {
    /// Create a new terminal UI
    pub fn new(config: UiConfig) -> Self {
        let (command_tx, command_rx) = mpsc::channel(1000);
        
        Self {
            config,
            filter: DisplayFilter::default(),
            log_buffer: VecDeque::with_capacity(MAX_LOG_ENTRIES),
            stats: LogStats::default(),
            current_device: None,
            current_stream: None,
            command_tx,
            command_rx: Some(command_rx),
        }
    }

    /// Get a sender for UI commands
    pub fn get_command_sender(&self) -> mpsc::Sender<UiCommand> {
        self.command_tx.clone()
    }

    /// Run the terminal UI
    pub async fn run(&mut self) -> io::Result<()> {
        let mut command_rx = self.command_rx.take()
            .ok_or_else(|| io::Error::other("Command receiver not available"))?;

        info!("Starting terminal UI");
        self.print_header()?;
        self.print_help()?;

        while let Some(command) = command_rx.recv().await {
            match command {
                UiCommand::AddLogEntries(entries) => {
                    self.add_log_entries(entries)?;
                }
                UiCommand::SetDevice(device) => {
                    self.current_device = Some(device);
                    self.print_device_info()?;
                }
                UiCommand::SetStream(stream) => {
                    self.current_stream = Some(stream);
                    self.print_stream_info()?;
                }
                UiCommand::SetFilter(filter) => {
                    self.filter = filter;
                    self.refresh_display()?;
                }
                UiCommand::SetConfig(config) => {
                    self.config = config;
                    self.refresh_display()?;
                }
                UiCommand::Clear => {
                    self.clear_logs()?;
                }
                UiCommand::Export(filename) => {
                    self.export_logs(&filename)?;
                }
                UiCommand::ShowHelp => {
                    self.print_help()?;
                }
                UiCommand::ShowStats => {
                    self.print_stats()?;
                }
                UiCommand::Quit => {
                    info!("Shutting down terminal UI");
                    break;
                }
            }
        }

        Ok(())
    }

    /// Add new log entries to the display
    fn add_log_entries(&mut self, entries: Vec<LogEntry>) -> io::Result<()> {
        for entry in entries {
            let is_filtered = !self.filter.matches(&entry);
            self.stats.update(&entry, is_filtered);

            if !is_filtered {
                self.print_log_entry(&entry)?;
            }

            // Add to buffer (with size limit)
            if self.log_buffer.len() >= MAX_LOG_ENTRIES {
                self.log_buffer.pop_front();
            }
            self.log_buffer.push_back(entry);
        }

        io::stdout().flush()?;
        Ok(())
    }

    /// Print a single log entry
    fn print_log_entry(&self, entry: &LogEntry) -> io::Result<()> {
        let mut output = String::new();

        // Add timestamp if enabled
        if self.config.show_timestamps {
            let timestamp = entry.timestamp
                .duration_since(UNIX_EPOCH)
                .unwrap_or_default();
            let secs = timestamp.as_secs();
            let millis = timestamp.subsec_millis();
            output.push_str(&format!("{:02}:{:02}:{:02}.{:03} ", 
                (secs / 3600) % 24, (secs / 60) % 60, secs % 60, millis));
        }

        // Add log level with color
        let (level_str, color) = if self.config.use_colors {
            match entry.level {
                LogLevel::Verbose => ("V", COLOR_VERBOSE),
                LogLevel::Debug => ("D", COLOR_DEBUG),
                LogLevel::Info => ("I", COLOR_INFO),
                LogLevel::Warn => ("W", COLOR_WARN),
                LogLevel::Error => ("E", COLOR_ERROR),
                LogLevel::Fatal => ("F", COLOR_FATAL),
            }
        } else {
            match entry.level {
                LogLevel::Verbose => ("V", ""),
                LogLevel::Debug => ("D", ""),
                LogLevel::Info => ("I", ""),
                LogLevel::Warn => ("W", ""),
                LogLevel::Error => ("E", ""),
                LogLevel::Fatal => ("F", ""),
            }
        };

        output.push_str(&format!("{}{}{} ", color, level_str, COLOR_RESET));

        // Add PID/TID if enabled
        if self.config.show_pids {
            output.push_str(&format!("{:5}/{:5} ", entry.process_id, entry.thread_id));
        }

        // Add tag
        output.push_str(&format!("{:15} ", 
            if entry.tag.len() > 15 {
                &entry.tag[..15]
            } else {
                &entry.tag
            }
        ));

        // Add message (with optional truncation)
        let message = if let Some(max_len) = self.config.max_line_length {
            if entry.message.len() > max_len {
                format!("{}...", &entry.message[..max_len - 3])
            } else {
                entry.message.clone()
            }
        } else {
            entry.message.clone()
        };

        output.push_str(&message);
        println!("{}", output);

        Ok(())
    }

    /// Print the UI header
    fn print_header(&self) -> io::Result<()> {
        println!("=== Remote ADB Log Monitor ===");
        println!("Commands: 'h' for help, 'q' to quit, 's' for stats, 'c' to clear");
        println!("{}",  "=".repeat(60));
        Ok(())
    }

    /// Print device information
    fn print_device_info(&self) -> io::Result<()> {
        if let Some(device) = &self.current_device {
            println!("Device: {} ({}) - Android {} API {}", 
                device.device_id, device.device_name, 
                device.android_version, device.api_level);
        }
        Ok(())
    }

    /// Print stream information
    fn print_stream_info(&self) -> io::Result<()> {
        if let Some(stream) = &self.current_stream {
            println!("Stream: {} for device {}", stream.stream_id, stream.device_id);
        }
        Ok(())
    }

    /// Print help information
    fn print_help(&self) -> io::Result<()> {
        println!("Help:");
        println!("  h - Show this help");
        println!("  s - Show statistics");
        println!("  c - Clear log buffer");
        println!("  q - Quit");
        println!("  Ctrl+C - Force quit");
        println!("{}",  "-".repeat(40));
        Ok(())
    }

    /// Print log statistics
    fn print_stats(&self) -> io::Result<()> {
        println!("Log Statistics:");
        println!("  Total entries: {}", self.stats.total_entries);
        println!("  Filtered entries: {}", self.stats.filtered_entries);
        println!("  Verbose: {}", self.stats.verbose_count);
        println!("  Debug: {}", self.stats.debug_count);
        println!("  Info: {}", self.stats.info_count);
        println!("  Warn: {}", self.stats.warn_count);
        println!("  Error: {}", self.stats.error_count);
        println!("  Fatal: {}", self.stats.fatal_count);
        println!("{}",  "-".repeat(40));
        Ok(())
    }

    /// Clear the log buffer
    fn clear_logs(&mut self) -> io::Result<()> {
        self.log_buffer.clear();
        self.stats = LogStats::default();
        
        // Clear terminal screen
        print!("\x1b[2J\x1b[H");
        io::stdout().flush()?;
        
        self.print_header()?;
        println!("Log buffer cleared.");
        Ok(())
    }

    /// Export logs to a file
    fn export_logs(&self, filename: &str) -> io::Result<()> {
        use std::fs::File;
        use std::io::Write;

        let mut file = File::create(filename)?;
        
        writeln!(file, "# Remote ADB Log Export")?;
        if let Some(device) = &self.current_device {
            writeln!(file, "# Device: {} ({})", device.device_id, device.device_name)?;
        }
        writeln!(file, "# Total entries: {}", self.log_buffer.len())?;
        writeln!(file)?;

        for entry in &self.log_buffer {
            if self.filter.matches(entry) {
                let timestamp = entry.timestamp
                    .duration_since(UNIX_EPOCH)
                    .unwrap_or_default();
                let secs = timestamp.as_secs();
                let millis = timestamp.subsec_millis();
                
                writeln!(file, "{:02}:{:02}:{:02}.{:03} {:?} {:5}/{:5} {} {}", 
                    (secs / 3600) % 24, (secs / 60) % 60, secs % 60, millis,
                    entry.level,
                    entry.process_id, entry.thread_id,
                    entry.tag,
                    entry.message)?;
            }
        }

        println!("Exported {} log entries to {}", self.log_buffer.len(), filename);
        Ok(())
    }

    /// Refresh the entire display
    fn refresh_display(&self) -> io::Result<()> {
        // Clear terminal screen
        print!("\x1b[2J\x1b[H");
        io::stdout().flush()?;
        
        self.print_header()?;
        self.print_device_info()?;
        self.print_stream_info()?;

        // Redisplay filtered log entries
        for entry in &self.log_buffer {
            if self.filter.matches(entry) {
                self.print_log_entry(entry)?;
            }
        }

        io::stdout().flush()?;
        Ok(())
    }
}

/// Simple log viewer that displays logs in a scrolling format
pub struct SimpleLogViewer {
    config: UiConfig,
    filter: DisplayFilter,
}

impl SimpleLogViewer {
    /// Create a new simple log viewer
    pub fn new(config: UiConfig) -> Self {
        Self {
            config,
            filter: DisplayFilter::default(),
        }
    }

    /// Set display filter
    pub fn set_filter(&mut self, filter: DisplayFilter) {
        self.filter = filter;
    }

    /// Display a batch of log entries
    pub fn display_entries(&self, entries: &[LogEntry]) -> io::Result<()> {
        for entry in entries {
            if self.filter.matches(entry) {
                self.print_log_entry(entry)?;
            }
        }
        io::stdout().flush()?;
        Ok(())
    }

    /// Print a single log entry (simplified version)
    fn print_log_entry(&self, entry: &LogEntry) -> io::Result<()> {
        let timestamp = entry.timestamp
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default();
        let secs = timestamp.as_secs();
        let millis = timestamp.subsec_millis();

        let level_char = match entry.level {
            LogLevel::Verbose => 'V',
            LogLevel::Debug => 'D',
            LogLevel::Info => 'I',
            LogLevel::Warn => 'W',
            LogLevel::Error => 'E',
            LogLevel::Fatal => 'F',
        };

        let color = if self.config.use_colors {
            match entry.level {
                LogLevel::Verbose => COLOR_VERBOSE,
                LogLevel::Debug => COLOR_DEBUG,
                LogLevel::Info => COLOR_INFO,
                LogLevel::Warn => COLOR_WARN,
                LogLevel::Error => COLOR_ERROR,
                LogLevel::Fatal => COLOR_FATAL,
            }
        } else {
            ""
        };

        println!("{:02}:{:02}:{:02}.{:03} {}{}{} {:5} {} {}", 
            (secs / 3600) % 24, (secs / 60) % 60, secs % 60, millis,
            color, level_char, COLOR_RESET,
            entry.process_id,
            entry.tag,
            entry.message);

        Ok(())
    }

    /// Print device information header
    pub fn print_device_header(&self, device: &DeviceInfo) -> io::Result<()> {
        println!("=== Monitoring Device: {} ===", device.device_id);
        println!("Name: {}", device.device_name);
        println!("Android: {} (API {})", device.android_version, device.api_level);
        println!("Status: {:?}", device.status);
        println!("{}",  "=".repeat(50));
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::SystemTime;

    fn create_test_log_entry(level: LogLevel, tag: &str, message: &str, pid: u32) -> LogEntry {
        LogEntry {
            timestamp: SystemTime::now(),
            level,
            tag: tag.to_string(),
            process_id: pid,
            thread_id: 1234,
            message: message.to_string(),
        }
    }

    #[test]
    fn test_display_filter_level() {
        let filter = DisplayFilter {
            min_level: Some(LogLevel::Warn),
            ..Default::default()
        };

        let verbose_entry = create_test_log_entry(LogLevel::Verbose, "Test", "message", 100);
        let warn_entry = create_test_log_entry(LogLevel::Warn, "Test", "message", 100);
        let error_entry = create_test_log_entry(LogLevel::Error, "Test", "message", 100);

        assert!(!filter.matches(&verbose_entry));
        assert!(filter.matches(&warn_entry));
        assert!(filter.matches(&error_entry));
    }

    #[test]
    fn test_display_filter_tag() {
        let filter = DisplayFilter {
            tag_filter: Some("MainActivity".to_string()),
            ..Default::default()
        };

        let matching_entry = create_test_log_entry(LogLevel::Info, "MainActivity", "message", 100);
        let non_matching_entry = create_test_log_entry(LogLevel::Info, "Service", "message", 100);

        assert!(filter.matches(&matching_entry));
        assert!(!filter.matches(&non_matching_entry));
    }

    #[test]
    fn test_display_filter_pid() {
        let filter = DisplayFilter {
            pid_filter: Some(1234),
            ..Default::default()
        };

        let matching_entry = create_test_log_entry(LogLevel::Info, "Test", "message", 1234);
        let non_matching_entry = create_test_log_entry(LogLevel::Info, "Test", "message", 5678);

        assert!(filter.matches(&matching_entry));
        assert!(!filter.matches(&non_matching_entry));
    }

    #[test]
    fn test_log_stats_update() {
        let mut stats = LogStats::default();
        let info_entry = create_test_log_entry(LogLevel::Info, "Test", "message", 100);
        let error_entry = create_test_log_entry(LogLevel::Error, "Test", "message", 100);

        stats.update(&info_entry, false);
        stats.update(&error_entry, false);
        stats.update(&info_entry, true); // filtered

        assert_eq!(stats.total_entries, 3);
        assert_eq!(stats.filtered_entries, 2);
        assert_eq!(stats.info_count, 1);
        assert_eq!(stats.error_count, 1);
    }

    #[test]
    fn test_ui_config_default() {
        let config = UiConfig::default();
        assert!(config.use_colors);
        assert!(config.show_timestamps);
        assert!(config.show_pids);
        assert!(config.auto_scroll);
        assert_eq!(config.max_line_length, Some(120));
    }
}