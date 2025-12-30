//! Advanced log filtering engine
//! 
//! This module provides comprehensive log filtering capabilities including
//! level filtering, tag matching, regex patterns, and real-time search functionality.

use crate::{LogEntry, LogFilters, Result, RemoteAdbError};
use regex::Regex;
use std::collections::HashSet;
use std::sync::Arc;
use tracing::{debug, warn};

/// Advanced log filtering engine with multiple filter types
#[derive(Debug, Clone)]
pub struct LogFilterEngine {
    /// Compiled regex patterns for efficient matching
    regex_cache: Arc<std::sync::RwLock<std::collections::HashMap<String, Regex>>>,
    /// Case sensitivity setting for text filters
    case_sensitive: bool,
}

/// Extended filter criteria for advanced filtering
#[derive(Debug, Clone, Default)]
pub struct AdvancedLogFilters {
    /// Base filters from LogFilters
    pub base: LogFilters,
    /// Multiple tag filters (OR logic)
    pub tag_filters: Vec<String>,
    /// Excluded tags (blacklist)
    pub excluded_tags: HashSet<String>,
    /// Multiple regex patterns (OR logic)
    pub regex_patterns: Vec<String>,
    /// Process ID filters
    pub process_ids: Option<HashSet<u32>>,
    /// Thread ID filters  
    pub thread_ids: Option<HashSet<u32>>,
    /// Message content filters (substring matching)
    pub message_contains: Vec<String>,
    /// Message content exclusions
    pub message_excludes: Vec<String>,
    /// Time range filter (start time)
    pub time_start: Option<std::time::SystemTime>,
    /// Time range filter (end time)
    pub time_end: Option<std::time::SystemTime>,
    /// Case sensitivity for text matching
    pub case_sensitive: bool,
}

impl LogFilterEngine {
    /// Create a new log filter engine
    pub fn new() -> Self {
        Self {
            regex_cache: Arc::new(std::sync::RwLock::new(std::collections::HashMap::new())),
            case_sensitive: false,
        }
    }

    /// Create a new log filter engine with case sensitivity setting
    pub fn with_case_sensitivity(case_sensitive: bool) -> Self {
        Self {
            regex_cache: Arc::new(std::sync::RwLock::new(std::collections::HashMap::new())),
            case_sensitive,
        }
    }

    /// Apply filters to a log entry and determine if it should be included
    pub fn should_include(&self, entry: &LogEntry, filters: &AdvancedLogFilters) -> Result<bool> {
        // Check log level filter
        if !self.check_log_level(entry, filters)? {
            return Ok(false);
        }

        // Check tag filters
        if !self.check_tag_filters(entry, filters)? {
            return Ok(false);
        }

        // Check excluded tags
        if !self.check_excluded_tags(entry, filters)? {
            return Ok(false);
        }

        // Check process ID filters
        if !self.check_process_id_filters(entry, filters)? {
            return Ok(false);
        }

        // Check thread ID filters
        if !self.check_thread_id_filters(entry, filters)? {
            return Ok(false);
        }

        // Check message content filters
        if !self.check_message_content_filters(entry, filters)? {
            return Ok(false);
        }

        // Check message exclusions
        if !self.check_message_exclusions(entry, filters)? {
            return Ok(false);
        }

        // Check regex patterns
        if !self.check_regex_patterns(entry, filters)? {
            return Ok(false);
        }

        // Check time range
        if !self.check_time_range(entry, filters)? {
            return Ok(false);
        }

        Ok(true)
    }

    /// Apply basic LogFilters (for backward compatibility)
    pub fn should_include_basic(&self, entry: &LogEntry, filters: &LogFilters) -> Result<bool> {
        let advanced_filters = AdvancedLogFilters {
            base: filters.clone(),
            case_sensitive: self.case_sensitive,
            ..Default::default()
        };
        self.should_include(entry, &advanced_filters)
    }

    /// Filter a batch of log entries
    pub fn filter_entries(&self, entries: &[LogEntry], filters: &AdvancedLogFilters) -> Result<Vec<LogEntry>> {
        let mut filtered = Vec::new();
        
        for entry in entries {
            if self.should_include(entry, filters)? {
                filtered.push(entry.clone());
            }
        }

        // Apply max_lines limit if specified
        if let Some(max_lines) = filters.base.max_lines {
            if filtered.len() > max_lines {
                // Keep the most recent entries
                filtered = filtered.into_iter().rev().take(max_lines).rev().collect();
            }
        }

        debug!("Filtered {} entries to {} entries", entries.len(), filtered.len());
        Ok(filtered)
    }

    /// Search for entries matching a query string
    pub fn search_entries(&self, entries: &[LogEntry], query: &str, case_sensitive: bool) -> Result<Vec<LogEntry>> {
        let search_query = if case_sensitive {
            query.to_string()
        } else {
            query.to_lowercase()
        };

        let mut results = Vec::new();
        
        for entry in entries {
            let message = if case_sensitive {
                &entry.message
            } else {
                &entry.message.to_lowercase()
            };
            
            let tag = if case_sensitive {
                &entry.tag
            } else {
                &entry.tag.to_lowercase()
            };

            if message.contains(&search_query) || tag.contains(&search_query) {
                results.push(entry.clone());
            }
        }

        debug!("Search for '{}' found {} results", query, results.len());
        Ok(results)
    }

    /// Get or compile a regex pattern
    fn get_regex(&self, pattern: &str) -> Result<Regex> {
        // First try to get from cache
        {
            let cache = self.regex_cache.read().map_err(|_| {
                RemoteAdbError::InvalidRequest("Failed to acquire regex cache read lock".to_string())
            })?;
            
            if let Some(regex) = cache.get(pattern) {
                return Ok(regex.clone());
            }
        }

        // Compile new regex
        let regex = Regex::new(pattern).map_err(|e| {
            RemoteAdbError::InvalidRequest(format!("Invalid regex pattern '{}': {}", pattern, e))
        })?;

        // Store in cache
        {
            let mut cache = self.regex_cache.write().map_err(|_| {
                RemoteAdbError::InvalidRequest("Failed to acquire regex cache write lock".to_string())
            })?;
            
            cache.insert(pattern.to_string(), regex.clone());
        }

        Ok(regex)
    }

    /// Check log level filter
    fn check_log_level(&self, entry: &LogEntry, filters: &AdvancedLogFilters) -> Result<bool> {
        if let Some(ref min_level) = filters.base.log_level {
            Ok(entry.level >= *min_level)
        } else {
            Ok(true)
        }
    }

    /// Check tag filters (OR logic - entry must match at least one tag filter)
    fn check_tag_filters(&self, entry: &LogEntry, filters: &AdvancedLogFilters) -> Result<bool> {
        // Check base tag filter first
        if let Some(ref tag_filter) = filters.base.tag_filter {
            let entry_tag = if filters.case_sensitive {
                &entry.tag
            } else {
                &entry.tag.to_lowercase()
            };
            
            let filter_tag = if filters.case_sensitive {
                tag_filter.clone()
            } else {
                tag_filter.to_lowercase()
            };

            if !entry_tag.contains(&filter_tag) {
                return Ok(false);
            }
        }

        // Check additional tag filters (OR logic)
        if !filters.tag_filters.is_empty() {
            let entry_tag = if filters.case_sensitive {
                &entry.tag
            } else {
                &entry.tag.to_lowercase()
            };

            let matches = filters.tag_filters.iter().any(|filter| {
                let filter_tag = if filters.case_sensitive {
                    filter.clone()
                } else {
                    filter.to_lowercase()
                };
                entry_tag.contains(&filter_tag)
            });

            if !matches {
                return Ok(false);
            }
        }

        Ok(true)
    }

    /// Check excluded tags (blacklist)
    fn check_excluded_tags(&self, entry: &LogEntry, filters: &AdvancedLogFilters) -> Result<bool> {
        if filters.excluded_tags.is_empty() {
            return Ok(true);
        }

        let entry_tag = if filters.case_sensitive {
            &entry.tag
        } else {
            &entry.tag.to_lowercase()
        };

        for excluded_tag in &filters.excluded_tags {
            let excluded = if filters.case_sensitive {
                excluded_tag.clone()
            } else {
                excluded_tag.to_lowercase()
            };

            if entry_tag.contains(&excluded) {
                return Ok(false);
            }
        }

        Ok(true)
    }

    /// Check process ID filters
    fn check_process_id_filters(&self, entry: &LogEntry, filters: &AdvancedLogFilters) -> Result<bool> {
        if let Some(ref process_ids) = filters.process_ids {
            Ok(process_ids.contains(&entry.process_id))
        } else {
            Ok(true)
        }
    }

    /// Check thread ID filters
    fn check_thread_id_filters(&self, entry: &LogEntry, filters: &AdvancedLogFilters) -> Result<bool> {
        if let Some(ref thread_ids) = filters.thread_ids {
            Ok(thread_ids.contains(&entry.thread_id))
        } else {
            Ok(true)
        }
    }

    /// Check message content filters (AND logic - entry must contain all specified strings)
    fn check_message_content_filters(&self, entry: &LogEntry, filters: &AdvancedLogFilters) -> Result<bool> {
        if filters.message_contains.is_empty() {
            return Ok(true);
        }

        let entry_message = if filters.case_sensitive {
            &entry.message
        } else {
            &entry.message.to_lowercase()
        };

        for content_filter in &filters.message_contains {
            let filter_content = if filters.case_sensitive {
                content_filter.clone()
            } else {
                content_filter.to_lowercase()
            };

            if !entry_message.contains(&filter_content) {
                return Ok(false);
            }
        }

        Ok(true)
    }

    /// Check message exclusions (blacklist)
    fn check_message_exclusions(&self, entry: &LogEntry, filters: &AdvancedLogFilters) -> Result<bool> {
        if filters.message_excludes.is_empty() {
            return Ok(true);
        }

        let entry_message = if filters.case_sensitive {
            &entry.message
        } else {
            &entry.message.to_lowercase()
        };

        for exclusion in &filters.message_excludes {
            let excluded_content = if filters.case_sensitive {
                exclusion.clone()
            } else {
                exclusion.to_lowercase()
            };

            if entry_message.contains(&excluded_content) {
                return Ok(false);
            }
        }

        Ok(true)
    }

    /// Check regex patterns (OR logic - entry must match at least one pattern)
    fn check_regex_patterns(&self, entry: &LogEntry, filters: &AdvancedLogFilters) -> Result<bool> {
        // Check base regex pattern first
        if let Some(ref pattern) = filters.base.regex_pattern {
            let regex = self.get_regex(pattern)?;
            if !regex.is_match(&entry.message) {
                return Ok(false);
            }
        }

        // Check additional regex patterns (OR logic)
        if !filters.regex_patterns.is_empty() {
            let mut matches = false;
            
            for pattern in &filters.regex_patterns {
                match self.get_regex(pattern) {
                    Ok(regex) => {
                        if regex.is_match(&entry.message) || regex.is_match(&entry.tag) {
                            matches = true;
                            break;
                        }
                    }
                    Err(e) => {
                        warn!("Invalid regex pattern '{}': {}", pattern, e);
                        continue;
                    }
                }
            }

            if !matches {
                return Ok(false);
            }
        }

        Ok(true)
    }

    /// Check time range filter
    fn check_time_range(&self, entry: &LogEntry, filters: &AdvancedLogFilters) -> Result<bool> {
        if let Some(start_time) = filters.time_start {
            if entry.timestamp < start_time {
                return Ok(false);
            }
        }

        if let Some(end_time) = filters.time_end {
            if entry.timestamp > end_time {
                return Ok(false);
            }
        }

        Ok(true)
    }

    /// Clear the regex cache
    pub fn clear_regex_cache(&self) -> Result<()> {
        let mut cache = self.regex_cache.write().map_err(|_| {
            RemoteAdbError::InvalidRequest("Failed to acquire regex cache write lock".to_string())
        })?;
        
        cache.clear();
        debug!("Regex cache cleared");
        Ok(())
    }

    /// Get regex cache statistics
    pub fn get_cache_stats(&self) -> Result<(usize, Vec<String>)> {
        let cache = self.regex_cache.read().map_err(|_| {
            RemoteAdbError::InvalidRequest("Failed to acquire regex cache read lock".to_string())
        })?;
        
        let size = cache.len();
        let patterns: Vec<String> = cache.keys().cloned().collect();
        
        Ok((size, patterns))
    }
}

impl Default for LogFilterEngine {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{LogLevel, LogEntry};
    use std::time::SystemTime;

    fn create_test_log_entry(level: LogLevel, tag: &str, message: &str) -> LogEntry {
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
    fn test_log_level_filtering() {
        let engine = LogFilterEngine::new();
        let entry = create_test_log_entry(LogLevel::Info, "TestTag", "Test message");
        
        let mut filters = AdvancedLogFilters::default();
        filters.base.log_level = Some(LogLevel::Warn);
        
        // Should be filtered out (Info < Warn)
        assert!(!engine.should_include(&entry, &filters).unwrap());
        
        filters.base.log_level = Some(LogLevel::Debug);
        
        // Should be included (Info >= Debug)
        assert!(engine.should_include(&entry, &filters).unwrap());
    }

    #[test]
    fn test_tag_filtering() {
        let engine = LogFilterEngine::new();
        let entry = create_test_log_entry(LogLevel::Info, "MainActivity", "Test message");
        
        let mut filters = AdvancedLogFilters::default();
        filters.base.tag_filter = Some("Main".to_string());
        
        // Should be included (tag contains "Main")
        assert!(engine.should_include(&entry, &filters).unwrap());
        
        filters.base.tag_filter = Some("Service".to_string());
        
        // Should be filtered out (tag doesn't contain "Service")
        assert!(!engine.should_include(&entry, &filters).unwrap());
    }

    #[test]
    fn test_multiple_tag_filters() {
        let engine = LogFilterEngine::new();
        let entry = create_test_log_entry(LogLevel::Info, "MainActivity", "Test message");
        
        let mut filters = AdvancedLogFilters::default();
        filters.tag_filters = vec!["Service".to_string(), "Activity".to_string()];
        
        // Should be included (tag contains "Activity")
        assert!(engine.should_include(&entry, &filters).unwrap());
        
        filters.tag_filters = vec!["Service".to_string(), "Fragment".to_string()];
        
        // Should be filtered out (tag doesn't contain either)
        assert!(!engine.should_include(&entry, &filters).unwrap());
    }

    #[test]
    fn test_excluded_tags() {
        let engine = LogFilterEngine::new();
        let entry = create_test_log_entry(LogLevel::Info, "SystemServer", "Test message");
        
        let mut filters = AdvancedLogFilters::default();
        filters.excluded_tags.insert("System".to_string());
        
        // Should be filtered out (tag contains excluded "System")
        assert!(!engine.should_include(&entry, &filters).unwrap());
        
        filters.excluded_tags.clear();
        filters.excluded_tags.insert("Activity".to_string());
        
        // Should be included (tag doesn't contain "Activity")
        assert!(engine.should_include(&entry, &filters).unwrap());
    }

    #[test]
    fn test_message_content_filtering() {
        let engine = LogFilterEngine::new();
        let entry = create_test_log_entry(LogLevel::Info, "TestTag", "Application started successfully");
        
        let mut filters = AdvancedLogFilters::default();
        filters.message_contains = vec!["Application".to_string(), "started".to_string()];
        
        // Should be included (message contains both strings)
        assert!(engine.should_include(&entry, &filters).unwrap());
        
        filters.message_contains = vec!["Application".to_string(), "failed".to_string()];
        
        // Should be filtered out (message doesn't contain "failed")
        assert!(!engine.should_include(&entry, &filters).unwrap());
    }

    #[test]
    fn test_message_exclusions() {
        let engine = LogFilterEngine::new();
        let entry = create_test_log_entry(LogLevel::Info, "TestTag", "Debug information");
        
        let mut filters = AdvancedLogFilters::default();
        filters.message_excludes = vec!["Debug".to_string()];
        
        // Should be filtered out (message contains excluded "Debug")
        assert!(!engine.should_include(&entry, &filters).unwrap());
        
        filters.message_excludes = vec!["Error".to_string()];
        
        // Should be included (message doesn't contain "Error")
        assert!(engine.should_include(&entry, &filters).unwrap());
    }

    #[test]
    fn test_regex_filtering() {
        let engine = LogFilterEngine::new();
        let entry = create_test_log_entry(LogLevel::Info, "TestTag", "Error code: 404");
        
        let mut filters = AdvancedLogFilters::default();
        filters.base.regex_pattern = Some(r"Error code: \d+".to_string());
        
        // Should be included (message matches regex)
        assert!(engine.should_include(&entry, &filters).unwrap());
        
        filters.base.regex_pattern = Some(r"Success code: \d+".to_string());
        
        // Should be filtered out (message doesn't match regex)
        assert!(!engine.should_include(&entry, &filters).unwrap());
    }

    #[test]
    fn test_process_id_filtering() {
        let engine = LogFilterEngine::new();
        let entry = create_test_log_entry(LogLevel::Info, "TestTag", "Test message");
        
        let mut filters = AdvancedLogFilters::default();
        let mut process_ids = HashSet::new();
        process_ids.insert(1234);
        filters.process_ids = Some(process_ids);
        
        // Should be included (process ID matches)
        assert!(engine.should_include(&entry, &filters).unwrap());
        
        let mut process_ids = HashSet::new();
        process_ids.insert(9999);
        filters.process_ids = Some(process_ids);
        
        // Should be filtered out (process ID doesn't match)
        assert!(!engine.should_include(&entry, &filters).unwrap());
    }

    #[test]
    fn test_case_sensitivity() {
        let engine = LogFilterEngine::with_case_sensitivity(true);
        let entry = create_test_log_entry(LogLevel::Info, "MainActivity", "Test Message");
        
        let mut filters = AdvancedLogFilters::default();
        filters.case_sensitive = true;
        filters.base.tag_filter = Some("mainactivity".to_string());
        
        // Should be filtered out (case sensitive, "MainActivity" != "mainactivity")
        assert!(!engine.should_include(&entry, &filters).unwrap());
        
        filters.case_sensitive = false;
        
        // Should be included (case insensitive)
        assert!(engine.should_include(&entry, &filters).unwrap());
    }

    #[test]
    fn test_search_entries() {
        let engine = LogFilterEngine::new();
        let entries = vec![
            create_test_log_entry(LogLevel::Info, "MainActivity", "Application started"),
            create_test_log_entry(LogLevel::Error, "NetworkService", "Connection failed"),
            create_test_log_entry(LogLevel::Debug, "DatabaseHelper", "Query executed"),
        ];
        
        // Case insensitive search
        let results = engine.search_entries(&entries, "application", false).unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].message, "Application started");
        
        // Case sensitive search
        let results = engine.search_entries(&entries, "application", true).unwrap();
        assert_eq!(results.len(), 0);
        
        // Search in tags
        let results = engine.search_entries(&entries, "Network", false).unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].tag, "NetworkService");
    }

    #[test]
    fn test_filter_entries_with_max_lines() {
        let engine = LogFilterEngine::new();
        let entries = vec![
            create_test_log_entry(LogLevel::Info, "Tag1", "Message 1"),
            create_test_log_entry(LogLevel::Info, "Tag2", "Message 2"),
            create_test_log_entry(LogLevel::Info, "Tag3", "Message 3"),
        ];
        
        let mut filters = AdvancedLogFilters::default();
        filters.base.max_lines = Some(2);
        
        let results = engine.filter_entries(&entries, &filters).unwrap();
        assert_eq!(results.len(), 2);
        // Should keep the most recent entries (last 2)
        assert_eq!(results[0].message, "Message 2");
        assert_eq!(results[1].message, "Message 3");
    }

    #[test]
    fn test_regex_cache() {
        let engine = LogFilterEngine::new();
        
        // Test cache stats
        let (size, patterns) = engine.get_cache_stats().unwrap();
        assert_eq!(size, 0);
        assert!(patterns.is_empty());
        
        // Add a regex pattern
        let entry = create_test_log_entry(LogLevel::Info, "TestTag", "Error code: 404");
        let mut filters = AdvancedLogFilters::default();
        filters.base.regex_pattern = Some(r"Error code: \d+".to_string());
        
        engine.should_include(&entry, &filters).unwrap();
        
        // Check cache stats
        let (size, patterns) = engine.get_cache_stats().unwrap();
        assert_eq!(size, 1);
        assert_eq!(patterns[0], r"Error code: \d+");
        
        // Clear cache
        engine.clear_regex_cache().unwrap();
        let (size, patterns) = engine.get_cache_stats().unwrap();
        assert_eq!(size, 0);
        assert!(patterns.is_empty());
    }
}