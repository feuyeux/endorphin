//! Performance Monitoring and Optimization Module
//! 
//! This module provides comprehensive performance monitoring, metrics collection,
//! and optimization features for the remote ADB logging system.

use crate::Result;
use serde::{Deserialize, Serialize};
use std::sync::{Arc, RwLock, atomic::{AtomicU64, AtomicUsize, Ordering}};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};
use tokio::sync::broadcast;
use tracing::{info, warn, error, debug};

/// Performance metrics data structure
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PerformanceMetrics {
    pub timestamp: u64,
    pub cpu_usage_percent: f64,
    pub memory_usage_mb: u64,
    pub memory_usage_percent: f64,
    pub network_bytes_sent: u64,
    pub network_bytes_received: u64,
    pub active_connections: usize,
    pub active_streams: usize,
    pub log_entries_processed: u64,
    pub log_entries_per_second: f64,
    pub average_response_time_ms: f64,
    pub error_count: u64,
    pub uptime_seconds: u64,
}

impl Default for PerformanceMetrics {
    fn default() -> Self {
        Self {
            timestamp: SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_secs(),
            cpu_usage_percent: 0.0,
            memory_usage_mb: 0,
            memory_usage_percent: 0.0,
            network_bytes_sent: 0,
            network_bytes_received: 0,
            active_connections: 0,
            active_streams: 0,
            log_entries_processed: 0,
            log_entries_per_second: 0.0,
            average_response_time_ms: 0.0,
            error_count: 0,
            uptime_seconds: 0,
        }
    }
}

/// Performance alert types
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum PerformanceAlert {
    HighCpuUsage { usage_percent: f64, threshold: f64 },
    HighMemoryUsage { usage_mb: u64, usage_percent: f64, threshold_percent: f64 },
    HighErrorRate { error_count: u64, time_window_seconds: u64 },
    SlowResponseTime { avg_response_ms: f64, threshold_ms: f64 },
    ConnectionLimitReached { active_connections: usize, max_connections: usize },
    StreamLimitReached { active_streams: usize, max_streams: usize },
}

/// Performance alert event
#[derive(Debug, Clone)]
pub struct PerformanceAlertEvent {
    pub alert: PerformanceAlert,
    pub timestamp: SystemTime,
    pub severity: AlertSeverity,
}

#[derive(Debug, Clone, Copy)]
pub enum AlertSeverity {
    Warning,
    Critical,
}

/// Performance thresholds configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PerformanceThresholds {
    pub cpu_warning_percent: f64,
    pub cpu_critical_percent: f64,
    pub memory_warning_percent: f64,
    pub memory_critical_percent: f64,
    pub response_time_warning_ms: f64,
    pub response_time_critical_ms: f64,
    pub error_rate_warning_per_minute: u64,
    pub error_rate_critical_per_minute: u64,
}

impl Default for PerformanceThresholds {
    fn default() -> Self {
        Self {
            cpu_warning_percent: 70.0,
            cpu_critical_percent: 90.0,
            memory_warning_percent: 80.0,
            memory_critical_percent: 95.0,
            response_time_warning_ms: 1000.0,
            response_time_critical_ms: 5000.0,
            error_rate_warning_per_minute: 10,
            error_rate_critical_per_minute: 50,
        }
    }
}

/// Memory optimization strategies
#[derive(Debug, Clone)]
pub enum MemoryOptimization {
    CompactBuffers,
    ClearOldLogs,
    ReduceBufferSizes,
    ForceGarbageCollection,
}

/// CPU optimization strategies
#[derive(Debug, Clone)]
pub enum CpuOptimization {
    ReducePollingFrequency,
    OptimizeBatchSizes,
    EnableCompression,
    ThrottleConnections,
}

/// Performance counter for tracking various metrics
#[derive(Debug)]
pub struct PerformanceCounter {
    log_entries_processed: AtomicU64,
    bytes_sent: AtomicU64,
    bytes_received: AtomicU64,
    error_count: AtomicU64,
    active_connections: AtomicUsize,
    active_streams: AtomicUsize,
    response_times: Arc<RwLock<Vec<f64>>>,
    start_time: Instant,
}

impl PerformanceCounter {
    pub fn new() -> Self {
        Self {
            log_entries_processed: AtomicU64::new(0),
            bytes_sent: AtomicU64::new(0),
            bytes_received: AtomicU64::new(0),
            error_count: AtomicU64::new(0),
            active_connections: AtomicUsize::new(0),
            active_streams: AtomicUsize::new(0),
            response_times: Arc::new(RwLock::new(Vec::new())),
            start_time: Instant::now(),
        }
    }

    pub fn increment_log_entries(&self, count: u64) {
        self.log_entries_processed.fetch_add(count, Ordering::Relaxed);
    }

    pub fn add_bytes_sent(&self, bytes: u64) {
        self.bytes_sent.fetch_add(bytes, Ordering::Relaxed);
    }

    pub fn add_bytes_received(&self, bytes: u64) {
        self.bytes_received.fetch_add(bytes, Ordering::Relaxed);
    }

    pub fn increment_errors(&self) {
        self.error_count.fetch_add(1, Ordering::Relaxed);
    }

    pub fn set_active_connections(&self, count: usize) {
        self.active_connections.store(count, Ordering::Relaxed);
    }

    pub fn set_active_streams(&self, count: usize) {
        self.active_streams.store(count, Ordering::Relaxed);
    }

    pub fn record_response_time(&self, duration: Duration) {
        let mut times = self.response_times.write().unwrap();
        times.push(duration.as_millis() as f64);
        
        // Keep only the last 1000 response times to prevent memory growth
        if times.len() > 1000 {
            let len = times.len();
            times.drain(0..len - 1000);
        }
    }

    pub fn get_metrics(&self) -> PerformanceMetrics {
        let response_times = self.response_times.read().unwrap();
        let avg_response_time = if response_times.is_empty() {
            0.0
        } else {
            response_times.iter().sum::<f64>() / response_times.len() as f64
        };

        let uptime = self.start_time.elapsed().as_secs();
        let log_entries = self.log_entries_processed.load(Ordering::Relaxed);
        let log_entries_per_second = if uptime > 0 {
            log_entries as f64 / uptime as f64
        } else {
            0.0
        };

        PerformanceMetrics {
            timestamp: SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_secs(),
            cpu_usage_percent: get_cpu_usage(),
            memory_usage_mb: get_memory_usage_mb(),
            memory_usage_percent: get_memory_usage_percent(),
            network_bytes_sent: self.bytes_sent.load(Ordering::Relaxed),
            network_bytes_received: self.bytes_received.load(Ordering::Relaxed),
            active_connections: self.active_connections.load(Ordering::Relaxed),
            active_streams: self.active_streams.load(Ordering::Relaxed),
            log_entries_processed: log_entries,
            log_entries_per_second,
            average_response_time_ms: avg_response_time,
            error_count: self.error_count.load(Ordering::Relaxed),
            uptime_seconds: uptime,
        }
    }

    pub fn reset(&self) {
        self.log_entries_processed.store(0, Ordering::Relaxed);
        self.bytes_sent.store(0, Ordering::Relaxed);
        self.bytes_received.store(0, Ordering::Relaxed);
        self.error_count.store(0, Ordering::Relaxed);
        self.response_times.write().unwrap().clear();
    }
}

impl Default for PerformanceCounter {
    fn default() -> Self {
        Self::new()
    }
}

/// Performance monitor with alerting and optimization
pub struct PerformanceMonitor {
    counter: Arc<PerformanceCounter>,
    thresholds: PerformanceThresholds,
    alert_sender: broadcast::Sender<PerformanceAlertEvent>,
    metrics_history: Arc<RwLock<Vec<PerformanceMetrics>>>,
    optimization_enabled: bool,
    max_history_size: usize,
}

impl PerformanceMonitor {
    pub fn new(thresholds: PerformanceThresholds) -> Self {
        let (alert_sender, _) = broadcast::channel(100);
        
        Self {
            counter: Arc::new(PerformanceCounter::new()),
            thresholds,
            alert_sender,
            metrics_history: Arc::new(RwLock::new(Vec::new())),
            optimization_enabled: true,
            max_history_size: 1000, // Keep last 1000 metrics snapshots
        }
    }

    pub fn get_counter(&self) -> Arc<PerformanceCounter> {
        self.counter.clone()
    }

    pub fn subscribe_to_alerts(&self) -> broadcast::Receiver<PerformanceAlertEvent> {
        self.alert_sender.subscribe()
    }

    pub fn get_current_metrics(&self) -> PerformanceMetrics {
        self.counter.get_metrics()
    }

    pub fn get_metrics_history(&self) -> Vec<PerformanceMetrics> {
        self.metrics_history.read().unwrap().clone()
    }

    pub fn enable_optimization(&mut self, enabled: bool) {
        self.optimization_enabled = enabled;
        info!("Performance optimization {}", if enabled { "enabled" } else { "disabled" });
    }

    pub fn update_thresholds(&mut self, thresholds: PerformanceThresholds) {
        self.thresholds = thresholds;
        info!("Performance thresholds updated");
    }

    /// Start performance monitoring with specified interval
    pub async fn start_monitoring(&self, interval: Duration) -> Result<()> {
        let counter = self.counter.clone();
        let thresholds = self.thresholds.clone();
        let alert_sender = self.alert_sender.clone();
        let metrics_history = self.metrics_history.clone();
        let optimization_enabled = self.optimization_enabled;
        let max_history_size = self.max_history_size;

        tokio::spawn(async move {
            let mut interval_timer = tokio::time::interval(interval);
            
            loop {
                interval_timer.tick().await;
                
                let metrics = counter.get_metrics();
                
                // Store metrics in history
                {
                    let mut history = metrics_history.write().unwrap();
                    history.push(metrics.clone());
                    
                    // Limit history size
                    if history.len() > max_history_size {
                        let len = history.len();
                        history.drain(0..len - max_history_size);
                    }
                }
                
                // Check for performance alerts
                Self::check_and_send_alerts(&metrics, &thresholds, &alert_sender).await;
                
                // Apply optimizations if enabled
                if optimization_enabled {
                    Self::apply_optimizations(&metrics, &thresholds).await;
                }
                
                debug!("Performance metrics collected: CPU: {:.1}%, Memory: {} MB ({:.1}%), Connections: {}, Streams: {}", 
                       metrics.cpu_usage_percent, 
                       metrics.memory_usage_mb, 
                       metrics.memory_usage_percent,
                       metrics.active_connections,
                       metrics.active_streams);
            }
        });

        info!("Performance monitoring started with interval: {:?}", interval);
        Ok(())
    }

    async fn check_and_send_alerts(
        metrics: &PerformanceMetrics,
        thresholds: &PerformanceThresholds,
        alert_sender: &broadcast::Sender<PerformanceAlertEvent>,
    ) {
        let mut alerts = Vec::new();

        // Check CPU usage
        if metrics.cpu_usage_percent >= thresholds.cpu_critical_percent {
            alerts.push((
                PerformanceAlert::HighCpuUsage {
                    usage_percent: metrics.cpu_usage_percent,
                    threshold: thresholds.cpu_critical_percent,
                },
                AlertSeverity::Critical,
            ));
        } else if metrics.cpu_usage_percent >= thresholds.cpu_warning_percent {
            alerts.push((
                PerformanceAlert::HighCpuUsage {
                    usage_percent: metrics.cpu_usage_percent,
                    threshold: thresholds.cpu_warning_percent,
                },
                AlertSeverity::Warning,
            ));
        }

        // Check memory usage
        if metrics.memory_usage_percent >= thresholds.memory_critical_percent {
            alerts.push((
                PerformanceAlert::HighMemoryUsage {
                    usage_mb: metrics.memory_usage_mb,
                    usage_percent: metrics.memory_usage_percent,
                    threshold_percent: thresholds.memory_critical_percent,
                },
                AlertSeverity::Critical,
            ));
        } else if metrics.memory_usage_percent >= thresholds.memory_warning_percent {
            alerts.push((
                PerformanceAlert::HighMemoryUsage {
                    usage_mb: metrics.memory_usage_mb,
                    usage_percent: metrics.memory_usage_percent,
                    threshold_percent: thresholds.memory_warning_percent,
                },
                AlertSeverity::Warning,
            ));
        }

        // Check response time
        if metrics.average_response_time_ms >= thresholds.response_time_critical_ms {
            alerts.push((
                PerformanceAlert::SlowResponseTime {
                    avg_response_ms: metrics.average_response_time_ms,
                    threshold_ms: thresholds.response_time_critical_ms,
                },
                AlertSeverity::Critical,
            ));
        } else if metrics.average_response_time_ms >= thresholds.response_time_warning_ms {
            alerts.push((
                PerformanceAlert::SlowResponseTime {
                    avg_response_ms: metrics.average_response_time_ms,
                    threshold_ms: thresholds.response_time_warning_ms,
                },
                AlertSeverity::Warning,
            ));
        }

        // Send alerts
        for (alert, severity) in alerts {
            let event = PerformanceAlertEvent {
                alert: alert.clone(),
                timestamp: SystemTime::now(),
                severity,
            };

            if let Err(e) = alert_sender.send(event) {
                warn!("Failed to send performance alert: {}", e);
            } else {
                match severity {
                    AlertSeverity::Warning => warn!("Performance warning: {:?}", alert),
                    AlertSeverity::Critical => error!("Performance critical alert: {:?}", alert),
                }
            }
        }
    }

    async fn apply_optimizations(metrics: &PerformanceMetrics, thresholds: &PerformanceThresholds) {
        let mut memory_optimizations = Vec::new();
        let mut cpu_optimizations = Vec::new();

        // Memory optimizations
        if metrics.memory_usage_percent >= thresholds.memory_warning_percent {
            memory_optimizations.push(MemoryOptimization::CompactBuffers);
            
            if metrics.memory_usage_percent >= thresholds.memory_critical_percent {
                memory_optimizations.push(MemoryOptimization::ClearOldLogs);
                memory_optimizations.push(MemoryOptimization::ForceGarbageCollection);
            }
        }

        // CPU optimizations
        if metrics.cpu_usage_percent >= thresholds.cpu_warning_percent {
            cpu_optimizations.push(CpuOptimization::EnableCompression);
            
            if metrics.cpu_usage_percent >= thresholds.cpu_critical_percent {
                cpu_optimizations.push(CpuOptimization::ReducePollingFrequency);
                cpu_optimizations.push(CpuOptimization::ThrottleConnections);
            }
        }

        // Apply memory optimizations
        for optimization in memory_optimizations {
            Self::apply_memory_optimization(optimization).await;
        }

        // Apply CPU optimizations
        for optimization in cpu_optimizations {
            Self::apply_cpu_optimization(optimization).await;
        }
    }

    async fn apply_memory_optimization(optimization: MemoryOptimization) {
        match optimization {
            MemoryOptimization::CompactBuffers => {
                info!("Applying optimization: Compacting buffers");
                // TODO: Implement buffer compaction
            }
            MemoryOptimization::ClearOldLogs => {
                info!("Applying optimization: Clearing old logs");
                // TODO: Implement old log clearing
            }
            MemoryOptimization::ReduceBufferSizes => {
                info!("Applying optimization: Reducing buffer sizes");
                // TODO: Implement buffer size reduction
            }
            MemoryOptimization::ForceGarbageCollection => {
                info!("Applying optimization: Forcing garbage collection");
                // In Rust, we don't have explicit GC, but we can drop unused data
            }
        }
    }

    async fn apply_cpu_optimization(optimization: CpuOptimization) {
        match optimization {
            CpuOptimization::ReducePollingFrequency => {
                info!("Applying optimization: Reducing polling frequency");
                // TODO: Implement polling frequency reduction
            }
            CpuOptimization::OptimizeBatchSizes => {
                info!("Applying optimization: Optimizing batch sizes");
                // TODO: Implement batch size optimization
            }
            CpuOptimization::EnableCompression => {
                info!("Applying optimization: Enabling compression");
                // TODO: Implement compression enabling
            }
            CpuOptimization::ThrottleConnections => {
                info!("Applying optimization: Throttling connections");
                // TODO: Implement connection throttling
            }
        }
    }

    /// Generate performance report
    pub fn generate_report(&self, duration: Duration) -> PerformanceReport {
        let history = self.metrics_history.read().unwrap();
        let current_time = SystemTime::now();
        let cutoff_time = current_time - duration;
        let cutoff_timestamp = cutoff_time.duration_since(UNIX_EPOCH).unwrap().as_secs();

        let relevant_metrics: Vec<_> = history
            .iter()
            .filter(|m| m.timestamp >= cutoff_timestamp)
            .cloned()
            .collect();

        PerformanceReport::from_metrics(relevant_metrics, duration)
    }
}

/// Performance report
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PerformanceReport {
    pub duration_seconds: u64,
    pub avg_cpu_usage: f64,
    pub max_cpu_usage: f64,
    pub avg_memory_usage_mb: u64,
    pub max_memory_usage_mb: u64,
    pub total_log_entries: u64,
    pub avg_log_entries_per_second: f64,
    pub total_bytes_transferred: u64,
    pub avg_response_time_ms: f64,
    pub max_response_time_ms: f64,
    pub total_errors: u64,
    pub peak_connections: usize,
    pub peak_streams: usize,
}

impl PerformanceReport {
    pub fn from_metrics(metrics: Vec<PerformanceMetrics>, duration: Duration) -> Self {
        if metrics.is_empty() {
            return Self::default();
        }

        let count = metrics.len() as f64;
        
        Self {
            duration_seconds: duration.as_secs(),
            avg_cpu_usage: metrics.iter().map(|m| m.cpu_usage_percent).sum::<f64>() / count,
            max_cpu_usage: metrics.iter().map(|m| m.cpu_usage_percent).fold(0.0, f64::max),
            avg_memory_usage_mb: (metrics.iter().map(|m| m.memory_usage_mb).sum::<u64>() as f64 / count) as u64,
            max_memory_usage_mb: metrics.iter().map(|m| m.memory_usage_mb).max().unwrap_or(0),
            total_log_entries: metrics.last().map(|m| m.log_entries_processed).unwrap_or(0),
            avg_log_entries_per_second: metrics.iter().map(|m| m.log_entries_per_second).sum::<f64>() / count,
            total_bytes_transferred: metrics.last().map(|m| m.network_bytes_sent + m.network_bytes_received).unwrap_or(0),
            avg_response_time_ms: metrics.iter().map(|m| m.average_response_time_ms).sum::<f64>() / count,
            max_response_time_ms: metrics.iter().map(|m| m.average_response_time_ms).fold(0.0, f64::max),
            total_errors: metrics.last().map(|m| m.error_count).unwrap_or(0),
            peak_connections: metrics.iter().map(|m| m.active_connections).max().unwrap_or(0),
            peak_streams: metrics.iter().map(|m| m.active_streams).max().unwrap_or(0),
        }
    }
}

impl Default for PerformanceReport {
    fn default() -> Self {
        Self {
            duration_seconds: 0,
            avg_cpu_usage: 0.0,
            max_cpu_usage: 0.0,
            avg_memory_usage_mb: 0,
            max_memory_usage_mb: 0,
            total_log_entries: 0,
            avg_log_entries_per_second: 0.0,
            total_bytes_transferred: 0,
            avg_response_time_ms: 0.0,
            max_response_time_ms: 0.0,
            total_errors: 0,
            peak_connections: 0,
            peak_streams: 0,
        }
    }
}

// Platform-specific system metrics functions
#[cfg(target_os = "linux")]
fn get_cpu_usage() -> f64 {
    // Simplified CPU usage calculation for Linux
    // In a real implementation, you would read from /proc/stat
    use std::fs;
    
    if let Ok(contents) = fs::read_to_string("/proc/loadavg") {
        if let Some(load_str) = contents.split_whitespace().next() {
            if let Ok(load) = load_str.parse::<f64>() {
                // Convert load average to approximate CPU percentage
                return (load * 100.0).min(100.0);
            }
        }
    }
    0.0
}

#[cfg(target_os = "macos")]
fn get_cpu_usage() -> f64 {
    // Simplified CPU usage for macOS
    // In a real implementation, you would use system calls
    0.0
}

#[cfg(target_os = "windows")]
fn get_cpu_usage() -> f64 {
    // Simplified CPU usage for Windows
    // In a real implementation, you would use Windows APIs
    0.0
}

#[cfg(not(any(target_os = "linux", target_os = "macos", target_os = "windows")))]
fn get_cpu_usage() -> f64 {
    0.0
}

fn get_memory_usage_mb() -> u64 {
    // Get current process memory usage
    #[cfg(target_os = "linux")]
    {
        use std::fs;
        if let Ok(contents) = fs::read_to_string("/proc/self/status") {
            for line in contents.lines() {
                if line.starts_with("VmRSS:") {
                    if let Some(kb_str) = line.split_whitespace().nth(1) {
                        if let Ok(kb) = kb_str.parse::<u64>() {
                            return kb / 1024; // Convert KB to MB
                        }
                    }
                }
            }
        }
    }
    
    // Fallback: estimate based on heap usage (very rough)
    0
}

fn get_memory_usage_percent() -> f64 {
    // Get system memory usage percentage
    #[cfg(target_os = "linux")]
    {
        use std::fs;
        if let Ok(contents) = fs::read_to_string("/proc/meminfo") {
            let mut total_kb = 0u64;
            let mut available_kb = 0u64;
            
            for line in contents.lines() {
                if line.starts_with("MemTotal:") {
                    if let Some(kb_str) = line.split_whitespace().nth(1) {
                        total_kb = kb_str.parse().unwrap_or(0);
                    }
                } else if line.starts_with("MemAvailable:") {
                    if let Some(kb_str) = line.split_whitespace().nth(1) {
                        available_kb = kb_str.parse().unwrap_or(0);
                    }
                }
            }
            
            if total_kb > 0 {
                let used_kb = total_kb - available_kb;
                return (used_kb as f64 / total_kb as f64) * 100.0;
            }
        }
    }
    
    0.0
}

/// Global performance monitor instance
static GLOBAL_PERFORMANCE_MONITOR: std::sync::OnceLock<Arc<tokio::sync::Mutex<PerformanceMonitor>>> = std::sync::OnceLock::new();

/// Initialize global performance monitor
pub fn init_global_performance_monitor(thresholds: PerformanceThresholds) -> Arc<tokio::sync::Mutex<PerformanceMonitor>> {
    GLOBAL_PERFORMANCE_MONITOR.get_or_init(|| {
        Arc::new(tokio::sync::Mutex::new(PerformanceMonitor::new(thresholds)))
    }).clone()
}

/// Get global performance monitor
pub fn global_performance_monitor() -> Arc<tokio::sync::Mutex<PerformanceMonitor>> {
    init_global_performance_monitor(PerformanceThresholds::default())
}

/// Get global performance counter
pub async fn global_performance_counter() -> Arc<PerformanceCounter> {
    let monitor = global_performance_monitor();
    let monitor = monitor.lock().await;
    monitor.get_counter()
}

/// Start global performance monitoring
pub async fn start_global_performance_monitoring(interval: Duration) -> Result<()> {
    let monitor = global_performance_monitor();
    let monitor = monitor.lock().await;
    monitor.start_monitoring(interval).await
}

/// Get current global performance metrics
pub async fn get_global_performance_metrics() -> PerformanceMetrics {
    let monitor = global_performance_monitor();
    let monitor = monitor.lock().await;
    monitor.get_current_metrics()
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::time::Duration;

    #[tokio::test]
    async fn test_performance_counter() {
        let counter = PerformanceCounter::new();
        
        counter.increment_log_entries(100);
        counter.add_bytes_sent(1024);
        counter.add_bytes_received(2048);
        counter.increment_errors();
        counter.set_active_connections(5);
        counter.set_active_streams(3);
        counter.record_response_time(Duration::from_millis(250));
        
        let metrics = counter.get_metrics();
        
        assert_eq!(metrics.log_entries_processed, 100);
        assert_eq!(metrics.network_bytes_sent, 1024);
        assert_eq!(metrics.network_bytes_received, 2048);
        assert_eq!(metrics.error_count, 1);
        assert_eq!(metrics.active_connections, 5);
        assert_eq!(metrics.active_streams, 3);
        assert!(metrics.average_response_time_ms > 0.0);
    }

    #[tokio::test]
    async fn test_performance_monitor() {
        let thresholds = PerformanceThresholds::default();
        let monitor = PerformanceMonitor::new(thresholds);
        
        let counter = monitor.get_counter();
        counter.increment_log_entries(50);
        
        let metrics = monitor.get_current_metrics();
        assert_eq!(metrics.log_entries_processed, 50);
        
        // Test metrics history
        let history = monitor.get_metrics_history();
        assert!(history.is_empty()); // No monitoring started yet
    }

    #[tokio::test]
    async fn test_performance_report() {
        let metrics = vec![
            PerformanceMetrics {
                cpu_usage_percent: 50.0,
                memory_usage_mb: 100,
                log_entries_processed: 1000,
                ..Default::default()
            },
            PerformanceMetrics {
                cpu_usage_percent: 75.0,
                memory_usage_mb: 150,
                log_entries_processed: 2000,
                ..Default::default()
            },
        ];
        
        let report = PerformanceReport::from_metrics(metrics, Duration::from_secs(60));
        
        assert_eq!(report.avg_cpu_usage, 62.5);
        assert_eq!(report.max_cpu_usage, 75.0);
        assert_eq!(report.max_memory_usage_mb, 150);
        assert_eq!(report.total_log_entries, 2000);
    }
}