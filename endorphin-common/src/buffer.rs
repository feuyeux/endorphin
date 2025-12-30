//! Buffer management for log entries
//! 
//! This module provides circular buffer implementation and intelligent cache
//! cleaning strategies for managing log entries in memory efficiently.

use crate::{LogEntry, LogLevel, Result, RemoteAdbError};
use std::collections::VecDeque;
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use std::sync::{Arc, RwLock};
use std::time::{Duration, SystemTime};
use tracing::{debug, info, warn};

/// Configuration for buffer management
#[derive(Debug, Clone)]
pub struct BufferConfig {
    /// Maximum number of log entries to keep in memory
    pub max_entries: usize,
    /// Maximum memory usage in bytes (approximate)
    pub max_memory_bytes: usize,
    /// Time-to-live for log entries (entries older than this are eligible for cleanup)
    pub entry_ttl: Duration,
    /// Cleanup threshold - when to trigger cleanup (as percentage of max_entries)
    pub cleanup_threshold: f32,
    /// How many entries to remove during cleanup (as percentage of max_entries)
    pub cleanup_batch_size: f32,
    /// Minimum log level to keep in buffer (lower levels are discarded first)
    pub min_log_level: Option<LogLevel>,
}

impl Default for BufferConfig {
    fn default() -> Self {
        Self {
            max_entries: 10_000,
            max_memory_bytes: 50 * 1024 * 1024, // 50MB
            entry_ttl: Duration::from_secs(3600), // 1 hour
            cleanup_threshold: 0.9, // Cleanup when 90% full
            cleanup_batch_size: 0.2, // Remove 20% of entries during cleanup
            min_log_level: Some(LogLevel::Debug), // Keep Debug and above by default
        }
    }
}

/// Statistics about buffer usage
#[derive(Debug, Clone)]
pub struct BufferStats {
    pub total_entries: usize,
    pub memory_usage_bytes: usize,
    pub oldest_entry_age: Option<Duration>,
    pub newest_entry_age: Option<Duration>,
    pub entries_by_level: std::collections::HashMap<LogLevel, usize>,
    pub cleanup_count: u64,
    pub entries_discarded: u64,
}

/// Circular buffer for log entries with intelligent cleanup
#[derive(Debug, Clone)]
pub struct LogBuffer {
    /// Configuration for the buffer
    config: BufferConfig,
    /// The actual buffer storing log entries
    entries: Arc<RwLock<VecDeque<LogEntry>>>,
    /// Approximate memory usage tracking
    memory_usage: Arc<AtomicUsize>,
    /// Statistics counters
    cleanup_count: Arc<AtomicU64>,
    entries_discarded: Arc<AtomicU64>,
    /// Last cleanup time
    last_cleanup: Arc<RwLock<SystemTime>>,
}

impl LogBuffer {
    /// Create a new log buffer with default configuration
    pub fn new() -> Self {
        Self::with_config(BufferConfig::default())
    }

    /// Create a new log buffer with custom configuration
    pub fn with_config(config: BufferConfig) -> Self {
        Self {
            config,
            entries: Arc::new(RwLock::new(VecDeque::new())),
            memory_usage: Arc::new(AtomicUsize::new(0)),
            cleanup_count: Arc::new(AtomicU64::new(0)),
            entries_discarded: Arc::new(AtomicU64::new(0)),
            last_cleanup: Arc::new(RwLock::new(SystemTime::now())),
        }
    }

    /// Add a log entry to the buffer
    pub fn push(&self, entry: LogEntry) -> Result<()> {
        // Check if entry meets minimum log level requirement
        if let Some(min_level) = &self.config.min_log_level {
            if entry.level < *min_level {
                // Discard entry that doesn't meet minimum level
                self.entries_discarded.fetch_add(1, Ordering::Relaxed);
                return Ok(());
            }
        }

        let entry_size = self.estimate_entry_size(&entry);
        
        {
            let mut entries = self.entries.write().map_err(|_| {
                RemoteAdbError::InvalidRequest("Failed to acquire buffer write lock".to_string())
            })?;

            entries.push_back(entry);
            self.memory_usage.fetch_add(entry_size, Ordering::Relaxed);
        }

        // Check if cleanup is needed
        self.check_and_cleanup()?;

        Ok(())
    }

    /// Get all entries from the buffer (returns a copy)
    pub fn get_all(&self) -> Result<Vec<LogEntry>> {
        let entries = self.entries.read().map_err(|_| {
            RemoteAdbError::InvalidRequest("Failed to acquire buffer read lock".to_string())
        })?;

        Ok(entries.iter().cloned().collect())
    }

    /// Get the most recent N entries
    pub fn get_recent(&self, count: usize) -> Result<Vec<LogEntry>> {
        let entries = self.entries.read().map_err(|_| {
            RemoteAdbError::InvalidRequest("Failed to acquire buffer read lock".to_string())
        })?;

        let start_index = if entries.len() > count {
            entries.len() - count
        } else {
            0
        };

        Ok(entries.range(start_index..).cloned().collect())
    }

    /// Get entries within a time range
    pub fn get_range(&self, start_time: SystemTime, end_time: SystemTime) -> Result<Vec<LogEntry>> {
        let entries = self.entries.read().map_err(|_| {
            RemoteAdbError::InvalidRequest("Failed to acquire buffer read lock".to_string())
        })?;

        let filtered: Vec<LogEntry> = entries
            .iter()
            .filter(|entry| entry.timestamp >= start_time && entry.timestamp <= end_time)
            .cloned()
            .collect();

        Ok(filtered)
    }

    /// Get entries matching a specific log level
    pub fn get_by_level(&self, level: LogLevel) -> Result<Vec<LogEntry>> {
        let entries = self.entries.read().map_err(|_| {
            RemoteAdbError::InvalidRequest("Failed to acquire buffer read lock".to_string())
        })?;

        let filtered: Vec<LogEntry> = entries
            .iter()
            .filter(|entry| entry.level == level)
            .cloned()
            .collect();

        Ok(filtered)
    }

    /// Clear all entries from the buffer
    pub fn clear(&self) -> Result<()> {
        {
            let mut entries = self.entries.write().map_err(|_| {
                RemoteAdbError::InvalidRequest("Failed to acquire buffer write lock".to_string())
            })?;

            entries.clear();
            self.memory_usage.store(0, Ordering::Relaxed);
        }
        
        info!("Buffer cleared");
        Ok(())
    }

    /// Get buffer statistics
    pub fn get_stats(&self) -> Result<BufferStats> {
        let entries = self.entries.read().map_err(|_| {
            RemoteAdbError::InvalidRequest("Failed to acquire buffer read lock".to_string())
        })?;

        let now = SystemTime::now();
        let mut oldest_age = None;
        let mut newest_age = None;
        let mut entries_by_level = std::collections::HashMap::new();

        if !entries.is_empty() {
            // Calculate ages
            if let Ok(duration) = now.duration_since(entries.front().unwrap().timestamp) {
                oldest_age = Some(duration);
            }
            if let Ok(duration) = now.duration_since(entries.back().unwrap().timestamp) {
                newest_age = Some(duration);
            }

            // Count entries by level
            for entry in entries.iter() {
                *entries_by_level.entry(entry.level.clone()).or_insert(0) += 1;
            }
        }

        Ok(BufferStats {
            total_entries: entries.len(),
            memory_usage_bytes: self.memory_usage.load(Ordering::Relaxed),
            oldest_entry_age: oldest_age,
            newest_entry_age: newest_age,
            entries_by_level,
            cleanup_count: self.cleanup_count.load(Ordering::Relaxed),
            entries_discarded: self.entries_discarded.load(Ordering::Relaxed),
        })
    }

    /// Force a cleanup operation
    pub fn force_cleanup(&self) -> Result<usize> {
        self.perform_cleanup(true)
    }

    /// Check if cleanup is needed and perform it if necessary
    fn check_and_cleanup(&self) -> Result<()> {
        let current_entries = {
            let entries = self.entries.read().map_err(|_| {
                RemoteAdbError::InvalidRequest("Failed to acquire buffer read lock".to_string())
            })?;
            entries.len()
        };

        let cleanup_threshold = (self.config.max_entries as f32 * self.config.cleanup_threshold) as usize;
        let memory_threshold = (self.config.max_memory_bytes as f32 * self.config.cleanup_threshold) as usize;
        let current_memory = self.memory_usage.load(Ordering::Relaxed);

        if current_entries >= cleanup_threshold || current_memory >= memory_threshold {
            self.perform_cleanup(false)?;
        }

        Ok(())
    }

    /// Perform the actual cleanup operation
    fn perform_cleanup(&self, force: bool) -> Result<usize> {
        let mut entries = self.entries.write().map_err(|_| {
            RemoteAdbError::InvalidRequest("Failed to acquire buffer write lock".to_string())
        })?;

        let initial_count = entries.len();
        let now = SystemTime::now();
        let mut removed_count = 0;
        let mut memory_freed = 0;

        // Strategy 1: Remove entries older than TTL
        while let Some(entry) = entries.front() {
            if let Ok(age) = now.duration_since(entry.timestamp) {
                if age > self.config.entry_ttl {
                    if let Some(removed_entry) = entries.pop_front() {
                        memory_freed += self.estimate_entry_size(&removed_entry);
                        removed_count += 1;
                    }
                } else {
                    break; // Entries are ordered by time, so we can stop here
                }
            } else {
                break;
            }
        }

        // Strategy 2: If still over threshold, remove entries by priority
        if force || entries.len() > self.config.max_entries || 
           self.memory_usage.load(Ordering::Relaxed) > self.config.max_memory_bytes {
            
            let target_removal = if force {
                (entries.len() as f32 * self.config.cleanup_batch_size) as usize
            } else {
                let entries_to_remove = entries.len().saturating_sub(self.config.max_entries);
                let memory_to_free = self.memory_usage.load(Ordering::Relaxed)
                    .saturating_sub(self.config.max_memory_bytes);
                
                // Estimate entries to remove based on memory pressure
                let avg_entry_size = if !entries.is_empty() {
                    self.memory_usage.load(Ordering::Relaxed) / entries.len()
                } else {
                    1000 // Default estimate
                };
                
                let memory_based_removal = memory_to_free / avg_entry_size;
                entries_to_remove.max(memory_based_removal)
            };

            // Remove entries with lower priority first (lower log levels, older entries)
            let mut entries_vec: Vec<LogEntry> = entries.drain(..).collect();
            
            // Sort by priority (higher priority = keep longer)
            entries_vec.sort_by(|a, b| {
                // Primary: log level (higher levels have higher priority)
                let level_cmp = b.level.cmp(&a.level);
                if level_cmp != std::cmp::Ordering::Equal {
                    return level_cmp;
                }
                
                // Secondary: timestamp (newer entries have higher priority)
                b.timestamp.cmp(&a.timestamp)
            });

            // Remove the lowest priority entries
            let keep_count = entries_vec.len().saturating_sub(target_removal);
            entries_vec.truncate(keep_count);
            removed_count += initial_count - keep_count;

            // Calculate memory freed from removed entries
            for _ in keep_count..initial_count {
                memory_freed += 1000; // Estimate, since we don't have the actual entries
            }

            // Restore the remaining entries (they're already sorted by priority)
            entries_vec.sort_by(|a, b| a.timestamp.cmp(&b.timestamp)); // Restore time order
            *entries = entries_vec.into();
        }

        // Update memory usage
        self.memory_usage.fetch_sub(memory_freed, Ordering::Relaxed);
        self.entries_discarded.fetch_add(removed_count as u64, Ordering::Relaxed);
        self.cleanup_count.fetch_add(1, Ordering::Relaxed);

        // Update last cleanup time
        {
            let mut last_cleanup = self.last_cleanup.write().map_err(|_| {
                RemoteAdbError::InvalidRequest("Failed to acquire cleanup time write lock".to_string())
            })?;
            *last_cleanup = now;
        }

        if removed_count > 0 {
            info!(
                "Buffer cleanup completed: removed {} entries, freed ~{} bytes, {} entries remaining",
                removed_count,
                memory_freed,
                entries.len()
            );
        }

        Ok(removed_count)
    }

    /// Estimate the memory size of a log entry
    fn estimate_entry_size(&self, entry: &LogEntry) -> usize {
        // Rough estimation of memory usage
        std::mem::size_of::<LogEntry>() +
        entry.tag.len() +
        entry.message.len() +
        std::mem::size_of::<SystemTime>()
    }

    /// Get buffer configuration
    pub fn config(&self) -> &BufferConfig {
        &self.config
    }

    /// Update buffer configuration (some changes require restart)
    pub fn update_config(&mut self, new_config: BufferConfig) -> Result<()> {
        self.config = new_config;
        
        // Trigger cleanup if new limits are lower
        self.check_and_cleanup()?;
        
        info!("Buffer configuration updated");
        Ok(())
    }
}

impl Default for LogBuffer {
    fn default() -> Self {
        Self::new()
    }
}

/// Multi-buffer manager for handling multiple log streams
#[derive(Debug)]
pub struct MultiBufferManager {
    /// Individual buffers for each stream
    buffers: Arc<RwLock<std::collections::HashMap<String, LogBuffer>>>,
    /// Default configuration for new buffers
    default_config: BufferConfig,
}

impl MultiBufferManager {
    /// Create a new multi-buffer manager
    pub fn new() -> Self {
        Self {
            buffers: Arc::new(RwLock::new(std::collections::HashMap::new())),
            default_config: BufferConfig::default(),
        }
    }

    /// Create a new multi-buffer manager with custom default configuration
    pub fn with_default_config(config: BufferConfig) -> Self {
        Self {
            buffers: Arc::new(RwLock::new(std::collections::HashMap::new())),
            default_config: config,
        }
    }

    /// Get or create a buffer for a stream
    pub fn get_or_create_buffer(&self, stream_id: &str) -> Result<LogBuffer> {
        let mut buffers = self.buffers.write().map_err(|_| {
            RemoteAdbError::InvalidRequest("Failed to acquire buffers write lock".to_string())
        })?;

        if let Some(buffer) = buffers.get(stream_id) {
            // Return a clone of the existing buffer (Arc makes this cheap)
            Ok(buffer.clone())
        } else {
            // Create a new buffer
            let buffer = LogBuffer::with_config(self.default_config.clone());
            buffers.insert(stream_id.to_string(), buffer.clone());
            Ok(buffer)
        }
    }

    /// Remove a buffer for a stream
    pub fn remove_buffer(&self, stream_id: &str) -> Result<bool> {
        let mut buffers = self.buffers.write().map_err(|_| {
            RemoteAdbError::InvalidRequest("Failed to acquire buffers write lock".to_string())
        })?;

        Ok(buffers.remove(stream_id).is_some())
    }

    /// Get statistics for all buffers
    pub fn get_all_stats(&self) -> Result<std::collections::HashMap<String, BufferStats>> {
        let buffers = self.buffers.read().map_err(|_| {
            RemoteAdbError::InvalidRequest("Failed to acquire buffers read lock".to_string())
        })?;

        let mut all_stats = std::collections::HashMap::new();
        
        for (stream_id, buffer) in buffers.iter() {
            match buffer.get_stats() {
                Ok(stats) => {
                    all_stats.insert(stream_id.clone(), stats);
                }
                Err(e) => {
                    warn!("Failed to get stats for buffer {}: {}", stream_id, e);
                }
            }
        }

        Ok(all_stats)
    }

    /// Force cleanup on all buffers
    pub fn cleanup_all(&self) -> Result<usize> {
        let buffers = self.buffers.read().map_err(|_| {
            RemoteAdbError::InvalidRequest("Failed to acquire buffers read lock".to_string())
        })?;

        let mut total_removed = 0;
        
        for (stream_id, buffer) in buffers.iter() {
            match buffer.force_cleanup() {
                Ok(removed) => {
                    total_removed += removed;
                    if removed > 0 {
                        debug!("Cleaned up {} entries from buffer {}", removed, stream_id);
                    }
                }
                Err(e) => {
                    warn!("Failed to cleanup buffer {}: {}", stream_id, e);
                }
            }
        }

        info!("Global cleanup completed: removed {} total entries", total_removed);
        Ok(total_removed)
    }

    /// Get the number of active buffers
    pub fn buffer_count(&self) -> Result<usize> {
        let buffers = self.buffers.read().map_err(|_| {
            RemoteAdbError::InvalidRequest("Failed to acquire buffers read lock".to_string())
        })?;

        Ok(buffers.len())
    }
}

impl Default for MultiBufferManager {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::thread;
    use std::time::Duration;

    fn create_test_entry(level: LogLevel, tag: &str, message: &str) -> LogEntry {
        LogEntry {
            timestamp: SystemTime::now(),
            level,
            tag: tag.to_string(),
            process_id: 1234,
            thread_id: 5678,
            message: message.to_string(),
        }
    }

    #[test]
    fn test_buffer_creation() {
        let buffer = LogBuffer::new();
        let stats = buffer.get_stats().unwrap();
        
        assert_eq!(stats.total_entries, 0);
        assert_eq!(stats.memory_usage_bytes, 0);
    }

    #[test]
    fn test_buffer_push_and_get() {
        let buffer = LogBuffer::new();
        let entry = create_test_entry(LogLevel::Info, "TestTag", "Test message");
        
        buffer.push(entry.clone()).unwrap();
        
        let entries = buffer.get_all().unwrap();
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].message, "Test message");
        
        let stats = buffer.get_stats().unwrap();
        assert_eq!(stats.total_entries, 1);
        assert!(stats.memory_usage_bytes > 0);
    }

    #[test]
    fn test_buffer_get_recent() {
        let buffer = LogBuffer::new();
        
        // Add multiple entries
        for i in 0..5 {
            let entry = create_test_entry(LogLevel::Info, "TestTag", &format!("Message {}", i));
            buffer.push(entry).unwrap();
        }
        
        let recent = buffer.get_recent(3).unwrap();
        assert_eq!(recent.len(), 3);
        assert_eq!(recent[0].message, "Message 2");
        assert_eq!(recent[2].message, "Message 4");
    }

    #[test]
    fn test_buffer_get_by_level() {
        let buffer = LogBuffer::new();
        
        buffer.push(create_test_entry(LogLevel::Info, "Tag1", "Info message")).unwrap();
        buffer.push(create_test_entry(LogLevel::Error, "Tag2", "Error message")).unwrap();
        buffer.push(create_test_entry(LogLevel::Info, "Tag3", "Another info")).unwrap();
        
        let info_entries = buffer.get_by_level(LogLevel::Info).unwrap();
        assert_eq!(info_entries.len(), 2);
        
        let error_entries = buffer.get_by_level(LogLevel::Error).unwrap();
        assert_eq!(error_entries.len(), 1);
        assert_eq!(error_entries[0].message, "Error message");
    }

    #[test]
    fn test_buffer_clear() {
        let buffer = LogBuffer::new();
        
        buffer.push(create_test_entry(LogLevel::Info, "TestTag", "Test message")).unwrap();
        assert_eq!(buffer.get_stats().unwrap().total_entries, 1);
        
        buffer.clear().unwrap();
        let stats = buffer.get_stats().unwrap();
        assert_eq!(stats.total_entries, 0);
        assert_eq!(stats.memory_usage_bytes, 0);
    }

    #[test]
    fn test_buffer_min_log_level_filtering() {
        let config = BufferConfig {
            min_log_level: Some(LogLevel::Warn),
            ..Default::default()
        };
        let buffer = LogBuffer::with_config(config);
        
        // These should be discarded
        buffer.push(create_test_entry(LogLevel::Debug, "Tag1", "Debug message")).unwrap();
        buffer.push(create_test_entry(LogLevel::Info, "Tag2", "Info message")).unwrap();
        
        // These should be kept
        buffer.push(create_test_entry(LogLevel::Warn, "Tag3", "Warn message")).unwrap();
        buffer.push(create_test_entry(LogLevel::Error, "Tag4", "Error message")).unwrap();
        
        let entries = buffer.get_all().unwrap();
        assert_eq!(entries.len(), 2);
        
        let stats = buffer.get_stats().unwrap();
        assert_eq!(stats.entries_discarded, 2);
    }

    #[test]
    fn test_buffer_cleanup_by_count() {
        let config = BufferConfig {
            max_entries: 5,
            cleanup_threshold: 0.8, // Cleanup at 4 entries (80% of 5)
            cleanup_batch_size: 0.4, // Remove 2 entries (40% of 5)
            ..Default::default()
        };
        let buffer = LogBuffer::with_config(config);
        
        // Add entries up to cleanup threshold
        for i in 0..6 {
            let entry = create_test_entry(LogLevel::Info, "TestTag", &format!("Message {}", i));
            buffer.push(entry).unwrap();
        }
        
        // Should have triggered cleanup
        let stats = buffer.get_stats().unwrap();
        assert!(stats.total_entries < 6);
        assert!(stats.cleanup_count > 0);
    }

    #[test]
    fn test_multi_buffer_manager() {
        let manager = MultiBufferManager::new();
        
        // Create buffers for different streams
        let buffer1 = manager.get_or_create_buffer("stream1").unwrap();
        let buffer2 = manager.get_or_create_buffer("stream2").unwrap();
        
        // Add entries to each buffer
        buffer1.push(create_test_entry(LogLevel::Info, "Tag1", "Stream 1 message")).unwrap();
        buffer2.push(create_test_entry(LogLevel::Error, "Tag2", "Stream 2 message")).unwrap();
        
        // Check buffer count
        assert_eq!(manager.buffer_count().unwrap(), 2);
        
        // Get stats for all buffers
        let all_stats = manager.get_all_stats().unwrap();
        assert_eq!(all_stats.len(), 2);
        assert!(all_stats.contains_key("stream1"));
        assert!(all_stats.contains_key("stream2"));
        
        // Remove a buffer
        assert!(manager.remove_buffer("stream1").unwrap());
        assert_eq!(manager.buffer_count().unwrap(), 1);
        
        // Try to remove non-existent buffer
        assert!(!manager.remove_buffer("nonexistent").unwrap());
    }

    #[test]
    fn test_buffer_time_range_filtering() {
        let buffer = LogBuffer::new();
        
        let start_time = SystemTime::now();
        
        // Add an entry
        buffer.push(create_test_entry(LogLevel::Info, "Tag1", "Message 1")).unwrap();
        
        // Wait a bit
        thread::sleep(Duration::from_millis(10));
        let mid_time = SystemTime::now();
        
        // Add another entry
        buffer.push(create_test_entry(LogLevel::Info, "Tag2", "Message 2")).unwrap();
        
        let end_time = SystemTime::now();
        
        // Get entries in different ranges
        let all_entries = buffer.get_range(start_time, end_time).unwrap();
        assert_eq!(all_entries.len(), 2);
        
        let mid_to_end = buffer.get_range(mid_time, end_time).unwrap();
        assert_eq!(mid_to_end.len(), 1);
        assert_eq!(mid_to_end[0].message, "Message 2");
    }

    #[test]
    fn test_buffer_stats() {
        let buffer = LogBuffer::new();
        
        // Add entries with different levels
        buffer.push(create_test_entry(LogLevel::Info, "Tag1", "Info message")).unwrap();
        buffer.push(create_test_entry(LogLevel::Error, "Tag2", "Error message")).unwrap();
        buffer.push(create_test_entry(LogLevel::Info, "Tag3", "Another info")).unwrap();
        
        let stats = buffer.get_stats().unwrap();
        
        assert_eq!(stats.total_entries, 3);
        assert!(stats.memory_usage_bytes > 0);
        assert_eq!(stats.entries_by_level.get(&LogLevel::Info), Some(&2));
        assert_eq!(stats.entries_by_level.get(&LogLevel::Error), Some(&1));
        assert!(stats.oldest_entry_age.is_some());
        assert!(stats.newest_entry_age.is_some());
    }

    #[test]
    fn test_force_cleanup() {
        let buffer = LogBuffer::new();
        
        // Add several entries
        for i in 0..10 {
            let entry = create_test_entry(LogLevel::Info, "TestTag", &format!("Message {}", i));
            buffer.push(entry).unwrap();
        }
        
        assert_eq!(buffer.get_stats().unwrap().total_entries, 10);
        
        // Force cleanup
        let removed = buffer.force_cleanup().unwrap();
        assert!(removed > 0);
        
        let stats = buffer.get_stats().unwrap();
        assert!(stats.total_entries < 10);
        assert!(stats.cleanup_count > 0);
    }
}