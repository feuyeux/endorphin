//! Logcat stream processing and log parsing
//! 
//! This module provides async logcat stream capture and real-time log parsing functionality.

use crate::{AdbExecutor, LogEntry, LogFilters, LogLevel, Result, RemoteAdbError, StreamHandle, StreamStatus, LogFilterEngine, AdvancedLogFilters, LogBuffer, BufferConfig};
use futures::stream::Stream;
use regex::Regex;
use std::collections::HashMap;
use std::pin::Pin;
use std::process::Stdio;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;
use std::task::{Context, Poll};
use std::time::{Duration, SystemTime};
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::process::{Child, Command};
use tokio::sync::{broadcast, RwLock};
use tokio::time::timeout;
use tracing::{debug, error, info, warn};
use uuid::Uuid;

/// Logcat stream manager for capturing and processing Android logs
#[derive(Debug)]
pub struct LogcatManager {
    adb_executor: AdbExecutor,
    active_streams: Arc<RwLock<HashMap<String, LogcatStream>>>,
    stream_counter: AtomicU64,
    filter_engine: LogFilterEngine,
}

/// Individual logcat stream for a specific device
#[derive(Debug)]
pub struct LogcatStream {
    handle: StreamHandle,
    status: Arc<RwLock<StreamStatus>>,
    process: Option<Child>,
    sender: broadcast::Sender<LogEntry>,
    is_active: Arc<AtomicBool>,
    #[allow(dead_code)]
    lines_captured: Arc<AtomicU64>,
    #[allow(dead_code)]
    bytes_transferred: Arc<AtomicU64>,
    buffer: LogBuffer,
}

/// Stream of log entries from logcat
pub struct LogEntryStream {
    receiver: broadcast::Receiver<LogEntry>,
    filters: LogFilters,
    filter_engine: LogFilterEngine,
}

impl LogcatManager {
    /// Create a new logcat manager
    pub fn new(adb_executor: AdbExecutor) -> Self {
        Self {
            adb_executor,
            active_streams: Arc::new(RwLock::new(HashMap::new())),
            stream_counter: AtomicU64::new(0),
            filter_engine: LogFilterEngine::new(),
        }
    }

    /// Create a new logcat manager with custom filter engine
    pub fn with_filter_engine(adb_executor: AdbExecutor, filter_engine: LogFilterEngine) -> Self {
        Self {
            adb_executor,
            active_streams: Arc::new(RwLock::new(HashMap::new())),
            stream_counter: AtomicU64::new(0),
            filter_engine,
        }
    }

    /// Start capturing logs from a device
    pub async fn start_log_capture(
        &self,
        device_id: String,
        filters: LogFilters,
    ) -> Result<StreamHandle> {
        info!("Starting log capture for device: {}", device_id);

        // Check if device is online
        if !self.adb_executor.is_device_online(&device_id).await? {
            return Err(RemoteAdbError::DeviceNotFound { device_id });
        }

        let stream_id = Uuid::new_v4().to_string();
        let handle = StreamHandle {
            stream_id: stream_id.clone(),
            device_id: device_id.clone(),
            created_at: SystemTime::now(),
            filters: filters.clone(),
        };

        let status = Arc::new(RwLock::new(StreamStatus {
            is_active: true,
            lines_captured: 0,
            bytes_transferred: 0,
            last_activity: Some(SystemTime::now()),
        }));

        let (sender, _) = broadcast::channel(1000); // Buffer up to 1000 log entries
        let is_active = Arc::new(AtomicBool::new(true));
        let lines_captured = Arc::new(AtomicU64::new(0));
        let bytes_transferred = Arc::new(AtomicU64::new(0));

        // Start logcat process
        let process = self.start_logcat_process(&device_id, &filters).await?;

        // Create buffer for this stream
        let buffer_config = BufferConfig {
            max_entries: filters.max_lines.unwrap_or(10_000),
            ..Default::default()
        };
        let buffer = LogBuffer::with_config(buffer_config);

        let stream = LogcatStream {
            handle: handle.clone(),
            status: status.clone(),
            process: Some(process),
            sender: sender.clone(),
            is_active: is_active.clone(),
            lines_captured: lines_captured.clone(),
            bytes_transferred: bytes_transferred.clone(),
            buffer,
        };

        // Store the stream
        {
            let mut streams = self.active_streams.write().await;
            streams.insert(stream_id.clone(), stream);
        }

        // Start processing logs in background
        let _adb_executor = self.adb_executor.clone();
        let stream_buffer = {
            let streams = self.active_streams.read().await;
            if let Some(stream) = streams.get(&stream_id) {
                stream.buffer.clone()
            } else {
                return Err(RemoteAdbError::InvalidRequest("Stream not found after creation".to_string()));
            }
        };
        
        tokio::spawn(async move {
            if let Err(e) = Self::process_logcat_output(
                stream_id.clone(),
                device_id,
                sender,
                status,
                is_active,
                lines_captured,
                bytes_transferred,
                stream_buffer,
            ).await {
                error!("Error processing logcat output for stream {}: {}", stream_id, e);
            }
        });

        self.stream_counter.fetch_add(1, Ordering::Relaxed);
        info!("Started log capture with stream ID: {}", handle.stream_id);
        Ok(handle)
    }

    /// Stop log capture for a stream
    pub async fn stop_log_capture(&self, handle: &StreamHandle) -> Result<()> {
        info!("Stopping log capture for stream: {}", handle.stream_id);

        let mut streams = self.active_streams.write().await;
        if let Some(mut stream) = streams.remove(&handle.stream_id) {
            // Mark as inactive
            stream.is_active.store(false, Ordering::Relaxed);

            // Kill the logcat process
            if let Some(mut process) = stream.process.take() {
                if let Err(e) = process.kill().await {
                    warn!("Failed to kill logcat process: {}", e);
                }
            }

            // Update status
            {
                let mut status = stream.status.write().await;
                status.is_active = false;
                status.last_activity = Some(SystemTime::now());
            }

            info!("Stopped log capture for stream: {}", handle.stream_id);
            Ok(())
        } else {
            Err(RemoteAdbError::InvalidRequest(format!(
                "Stream not found: {}",
                handle.stream_id
            )))
        }
    }

    /// Get stream status
    pub async fn get_stream_status(&self, handle: &StreamHandle) -> Result<StreamStatus> {
        let streams = self.active_streams.read().await;
        if let Some(stream) = streams.get(&handle.stream_id) {
            let status = stream.status.read().await;
            Ok(status.clone())
        } else {
            Err(RemoteAdbError::InvalidRequest(format!(
                "Stream not found: {}",
                handle.stream_id
            )))
        }
    }

    /// Get a stream of log entries for a handle
    pub async fn get_log_stream(&self, handle: &StreamHandle) -> Result<LogEntryStream> {
        let streams = self.active_streams.read().await;
        if let Some(stream) = streams.get(&handle.stream_id) {
            let receiver = stream.sender.subscribe();
            Ok(LogEntryStream {
                receiver,
                filters: handle.filters.clone(),
                filter_engine: self.filter_engine.clone(),
            })
        } else {
            Err(RemoteAdbError::InvalidRequest(format!(
                "Stream not found: {}",
                handle.stream_id
            )))
        }
    }

    /// Get buffered log entries for a stream
    pub async fn get_buffered_logs(&self, handle: &StreamHandle, count: Option<usize>) -> Result<Vec<LogEntry>> {
        let streams = self.active_streams.read().await;
        if let Some(stream) = streams.get(&handle.stream_id) {
            if let Some(count) = count {
                stream.buffer.get_recent(count)
            } else {
                stream.buffer.get_all()
            }
        } else {
            Err(RemoteAdbError::InvalidRequest(format!(
                "Stream not found: {}",
                handle.stream_id
            )))
        }
    }

    /// Get buffer statistics for a stream
    pub async fn get_buffer_stats(&self, handle: &StreamHandle) -> Result<crate::BufferStats> {
        let streams = self.active_streams.read().await;
        if let Some(stream) = streams.get(&handle.stream_id) {
            stream.buffer.get_stats()
        } else {
            Err(RemoteAdbError::InvalidRequest(format!(
                "Stream not found: {}",
                handle.stream_id
            )))
        }
    }

    /// Clear buffer for a stream
    pub async fn clear_buffer(&self, handle: &StreamHandle) -> Result<()> {
        let streams = self.active_streams.read().await;
        if let Some(stream) = streams.get(&handle.stream_id) {
            stream.buffer.clear()
        } else {
            Err(RemoteAdbError::InvalidRequest(format!(
                "Stream not found: {}",
                handle.stream_id
            )))
        }
    }

    /// Search log entries across all active streams
    pub async fn search_logs(&self, query: &str, case_sensitive: bool) -> Result<Vec<LogEntry>> {
        let streams = self.active_streams.read().await;
        let mut all_entries = Vec::new();
        
        for stream in streams.values() {
            match stream.buffer.get_all() {
                Ok(entries) => {
                    let search_results = self.filter_engine.search_entries(&entries, query, case_sensitive)?;
                    all_entries.extend(search_results);
                }
                Err(e) => {
                    warn!("Failed to get entries from stream buffer: {}", e);
                }
            }
        }
        
        // Sort by timestamp
        all_entries.sort_by(|a, b| a.timestamp.cmp(&b.timestamp));
        
        debug!("Search for '{}' found {} results across {} streams", query, all_entries.len(), streams.len());
        Ok(all_entries)
    }

    /// Apply advanced filters to log entries
    pub fn filter_entries(&self, entries: &[LogEntry], filters: &AdvancedLogFilters) -> Result<Vec<LogEntry>> {
        self.filter_engine.filter_entries(entries, filters)
    }

    /// Get filter engine for advanced filtering operations
    pub fn filter_engine(&self) -> &LogFilterEngine {
        &self.filter_engine
    }

    /// Get all active streams
    pub async fn get_active_streams(&self) -> Vec<StreamHandle> {
        let streams = self.active_streams.read().await;
        streams.values().map(|s| s.handle.clone()).collect()
    }

    /// Start logcat process for a device
    async fn start_logcat_process(&self, device_id: &str, filters: &LogFilters) -> Result<Child> {
        let mut args = vec!["-s", device_id, "logcat"];

        // Add format for structured parsing
        args.extend_from_slice(&["-v", "threadtime"]);

        // Add log level filter if specified
        if let Some(ref level) = filters.log_level {
            let level_str = match level {
                LogLevel::Verbose => "*:V",
                LogLevel::Debug => "*:D",
                LogLevel::Info => "*:I",
                LogLevel::Warn => "*:W",
                LogLevel::Error => "*:E",
                LogLevel::Fatal => "*:F",
            };
            args.push(level_str);
        }

        // Add tag filter if specified
        if let Some(ref tag) = filters.tag_filter {
            args.push(tag);
        }

        debug!("Starting logcat with args: {:?}", args);

        let mut cmd = Command::new(self.adb_executor.adb_path()); // Use the getter method
        let child = cmd
            .args(&args)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|e| RemoteAdbError::adb(format!("Failed to start logcat: {}", e)))?;

        Ok(child)
    }

    /// Process logcat output and parse log entries
    #[allow(clippy::too_many_arguments)]
    async fn process_logcat_output(
        stream_id: String,
        device_id: String,
        sender: broadcast::Sender<LogEntry>,
        status: Arc<RwLock<StreamStatus>>,
        is_active: Arc<AtomicBool>,
        lines_captured: Arc<AtomicU64>,
        bytes_transferred: Arc<AtomicU64>,
        buffer: LogBuffer,
    ) -> Result<()> {
        // For now, we'll start a new logcat process here
        // In a real implementation, we'd need to restructure to avoid this duplication
        let mut cmd = Command::new("adb");
        let mut child = cmd
            .args(["-s", &device_id, "logcat", "-v", "threadtime"])
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|e| RemoteAdbError::adb(format!("Failed to start logcat: {}", e)))?;

        let stdout = child.stdout.take().ok_or_else(|| {
            RemoteAdbError::adb("Failed to get logcat stdout".to_string())
        })?;

        let reader = BufReader::new(stdout);
        let mut lines = reader.lines();

        while is_active.load(Ordering::Relaxed) {
            match timeout(Duration::from_secs(1), lines.next_line()).await {
                Ok(Ok(Some(line))) => {
                    let line_bytes = line.len() as u64;
                    bytes_transferred.fetch_add(line_bytes, Ordering::Relaxed);
                    lines_captured.fetch_add(1, Ordering::Relaxed);

                    // Parse the log entry
                    if let Ok(log_entry) = Self::parse_logcat_line(&line) {
                        // Store in buffer
                        if let Err(e) = buffer.push(log_entry.clone()) {
                            warn!("Failed to store log entry in buffer: {}", e);
                        }

                        // Send to subscribers (ignore if no receivers)
                        let _ = sender.send(log_entry);

                        // Update status
                        {
                            let mut status_guard = status.write().await;
                            status_guard.lines_captured = lines_captured.load(Ordering::Relaxed);
                            status_guard.bytes_transferred = bytes_transferred.load(Ordering::Relaxed);
                            status_guard.last_activity = Some(SystemTime::now());
                        }
                    }
                }
                Ok(Ok(None)) => {
                    // End of stream
                    debug!("Logcat stream ended for device: {}", device_id);
                    break;
                }
                Ok(Err(e)) => {
                    error!("Error reading logcat line: {}", e);
                    break;
                }
                Err(_) => {
                    // Timeout - continue if still active
                    continue;
                }
            }
        }

        // Clean up
        if let Err(e) = child.kill().await {
            warn!("Failed to kill logcat process: {}", e);
        }

        // Update final status
        {
            let mut status_guard = status.write().await;
            status_guard.is_active = false;
            status_guard.last_activity = Some(SystemTime::now());
        }

        info!("Logcat processing ended for stream: {}", stream_id);
        Ok(())
    }

    /// Parse a logcat line in threadtime format
    fn parse_logcat_line(line: &str) -> Result<LogEntry> {
        // Threadtime format: MM-DD HH:MM:SS.mmm PID TID LEVEL TAG: MESSAGE
        // Example: 12-28 15:30:45.123  1234  5678 I MainActivity: Application started
        
        let re = Regex::new(r"^(\d{2}-\d{2} \d{2}:\d{2}:\d{2}\.\d{3})\s+(\d+)\s+(\d+)\s+([VDIWEF])\s+([^:]+):\s*(.*)$")
            .map_err(|e| RemoteAdbError::adb(format!("Regex compilation failed: {}", e)))?;

        if let Some(captures) = re.captures(line) {
            let _timestamp_str = captures.get(1).unwrap().as_str();
            let process_id: u32 = captures.get(2).unwrap().as_str().parse()
                .map_err(|_| RemoteAdbError::adb("Invalid process ID".to_string()))?;
            let thread_id: u32 = captures.get(3).unwrap().as_str().parse()
                .map_err(|_| RemoteAdbError::adb("Invalid thread ID".to_string()))?;
            let level_char = captures.get(4).unwrap().as_str();
            let tag = captures.get(5).unwrap().as_str().trim().to_string();
            let message = captures.get(6).unwrap().as_str().to_string();

            let level = match level_char {
                "V" => LogLevel::Verbose,
                "D" => LogLevel::Debug,
                "I" => LogLevel::Info,
                "W" => LogLevel::Warn,
                "E" => LogLevel::Error,
                "F" => LogLevel::Fatal,
                _ => LogLevel::Debug, // Default fallback
            };

            // For simplicity, use current time as timestamp
            // In a real implementation, you'd parse the timestamp_str properly
            let timestamp = SystemTime::now();

            Ok(LogEntry {
                timestamp,
                level,
                tag,
                process_id,
                thread_id,
                message,
            })
        } else {
            Err(RemoteAdbError::adb(format!("Failed to parse logcat line: {}", line)))
        }
    }
}

impl Stream for LogEntryStream {
    type Item = LogEntry;

    fn poll_next(mut self: Pin<&mut Self>, _cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        loop {
            match self.receiver.try_recv() {
                Ok(entry) => {
                    // Apply filters
                    if self.should_include_entry(&entry) {
                        return Poll::Ready(Some(entry));
                    }
                    // Continue to next entry if filtered out
                }
                Err(broadcast::error::TryRecvError::Empty) => {
                    // No more entries available right now
                    return Poll::Pending;
                }
                Err(broadcast::error::TryRecvError::Closed) => {
                    // Stream is closed
                    return Poll::Ready(None);
                }
                Err(broadcast::error::TryRecvError::Lagged(_)) => {
                    // We've fallen behind, continue to get the latest entries
                    continue;
                }
            }
        }
    }
}

impl LogEntryStream {
    /// Check if a log entry should be included based on filters
    fn should_include_entry(&self, entry: &LogEntry) -> bool {
        // Use the new filter engine for more comprehensive filtering
        match self.filter_engine.should_include_basic(entry, &self.filters) {
            Ok(result) => result,
            Err(e) => {
                warn!("Error applying filters: {}", e);
                // Fallback to basic filtering on error
                self.should_include_entry_basic(entry)
            }
        }
    }

    /// Basic filtering logic (fallback)
    fn should_include_entry_basic(&self, entry: &LogEntry) -> bool {
        // Check log level filter
        if let Some(ref min_level) = self.filters.log_level {
            if entry.level < *min_level {
                return false;
            }
        }

        // Check tag filter
        if let Some(ref tag_filter) = self.filters.tag_filter {
            if !entry.tag.contains(tag_filter) {
                return false;
            }
        }

        // Check regex pattern
        if let Some(ref pattern) = self.filters.regex_pattern {
            if let Ok(regex) = Regex::new(pattern) {
                if !regex.is_match(&entry.message) {
                    return false;
                }
            }
        }

        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_logcat_line() {
        let line = "12-28 15:30:45.123  1234  5678 I MainActivity: Application started";
        let result = LogcatManager::parse_logcat_line(line);
        
        assert!(result.is_ok());
        let entry = result.unwrap();
        assert_eq!(entry.process_id, 1234);
        assert_eq!(entry.thread_id, 5678);
        assert_eq!(entry.level, LogLevel::Info);
        assert_eq!(entry.tag, "MainActivity");
        assert_eq!(entry.message, "Application started");
    }

    #[test]
    fn test_parse_logcat_line_with_spaces_in_message() {
        let line = "12-28 15:30:45.123  1234  5678 E SystemServer: Error occurred in system service";
        let result = LogcatManager::parse_logcat_line(line);
        
        assert!(result.is_ok());
        let entry = result.unwrap();
        assert_eq!(entry.level, LogLevel::Error);
        assert_eq!(entry.tag, "SystemServer");
        assert_eq!(entry.message, "Error occurred in system service");
    }

    #[test]
    fn test_parse_invalid_logcat_line() {
        let line = "Invalid logcat line format";
        let result = LogcatManager::parse_logcat_line(line);
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_logcat_manager_creation() {
        let adb_executor = AdbExecutor::new();
        let manager = LogcatManager::new(adb_executor);
        
        let streams = manager.get_active_streams().await;
        assert!(streams.is_empty());
    }

    #[test]
    fn test_log_entry_stream_filtering() {
        let (_sender, receiver) = broadcast::channel(10);
        let filters = LogFilters {
            log_level: Some(LogLevel::Warn),
            tag_filter: Some("Test".to_string()),
            ..Default::default()
        };
        
        let stream = LogEntryStream { 
            receiver, 
            filters,
            filter_engine: LogFilterEngine::new(),
        };
        
        // Test entry that should be included
        let entry = LogEntry {
            timestamp: SystemTime::now(),
            level: LogLevel::Error,
            tag: "TestTag".to_string(),
            process_id: 1234,
            thread_id: 5678,
            message: "Test message".to_string(),
        };
        
        assert!(stream.should_include_entry(&entry));
        
        // Test entry that should be filtered out (wrong level)
        let entry_filtered = LogEntry {
            level: LogLevel::Debug,
            ..entry.clone()
        };
        
        assert!(!stream.should_include_entry(&entry_filtered));
    }
}