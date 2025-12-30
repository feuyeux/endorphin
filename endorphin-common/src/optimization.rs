//! Performance Optimization Utilities
//! 
//! This module provides utilities for optimizing memory usage, CPU usage,
//! and overall system performance.

use crate::{Result, PerformanceMetrics};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::RwLock;
use tracing::{info, debug};

/// Memory optimization manager
pub struct MemoryOptimizer {
    max_memory_mb: AtomicUsize,
    current_memory_mb: AtomicUsize,
    gc_enabled: AtomicBool,
    last_gc_time: Arc<RwLock<Instant>>,
    gc_interval: Duration,
}

impl MemoryOptimizer {
    pub fn new(max_memory_mb: usize, gc_interval: Duration) -> Self {
        Self {
            max_memory_mb: AtomicUsize::new(max_memory_mb),
            current_memory_mb: AtomicUsize::new(0),
            gc_enabled: AtomicBool::new(true),
            last_gc_time: Arc::new(RwLock::new(Instant::now())),
            gc_interval,
        }
    }

    pub fn set_max_memory(&self, max_memory_mb: usize) {
        self.max_memory_mb.store(max_memory_mb, Ordering::Relaxed);
        info!("Memory limit updated to {} MB", max_memory_mb);
    }

    pub fn update_current_memory(&self, current_memory_mb: usize) {
        self.current_memory_mb.store(current_memory_mb, Ordering::Relaxed);
    }

    pub fn get_memory_usage_percent(&self) -> f64 {
        let current = self.current_memory_mb.load(Ordering::Relaxed);
        let max = self.max_memory_mb.load(Ordering::Relaxed);
        
        if max == 0 {
            0.0
        } else {
            (current as f64 / max as f64) * 100.0
        }
    }

    pub fn should_trigger_gc(&self) -> bool {
        if !self.gc_enabled.load(Ordering::Relaxed) {
            return false;
        }

        let usage_percent = self.get_memory_usage_percent();
        usage_percent > 80.0 // Trigger GC when memory usage exceeds 80%
    }

    pub async fn maybe_trigger_gc(&self) -> Result<bool> {
        if !self.should_trigger_gc() {
            return Ok(false);
        }

        let last_gc = *self.last_gc_time.read().await;
        let now = Instant::now();
        
        if now.duration_since(last_gc) < self.gc_interval {
            return Ok(false); // Too soon since last GC
        }

        self.trigger_gc().await?;
        Ok(true)
    }

    pub async fn trigger_gc(&self) -> Result<()> {
        info!("Triggering garbage collection");
        
        // Update last GC time
        {
            let mut last_gc = self.last_gc_time.write().await;
            *last_gc = Instant::now();
        }

        // In Rust, we don't have explicit GC, but we can:
        // 1. Drop unused data structures
        // 2. Compact buffers
        // 3. Clear caches
        
        // This would be implemented by the calling code
        // by dropping unused data and compacting structures
        
        info!("Garbage collection completed");
        Ok(())
    }

    pub fn enable_gc(&self, enabled: bool) {
        self.gc_enabled.store(enabled, Ordering::Relaxed);
        info!("Garbage collection {}", if enabled { "enabled" } else { "disabled" });
    }
}

/// CPU optimization manager
pub struct CpuOptimizer {
    max_cpu_percent: AtomicUsize,
    current_cpu_percent: AtomicUsize,
    throttling_enabled: AtomicBool,
    batch_size: AtomicUsize,
    polling_interval: Arc<RwLock<Duration>>,
}

impl CpuOptimizer {
    pub fn new(max_cpu_percent: usize, initial_batch_size: usize, initial_polling_interval: Duration) -> Self {
        Self {
            max_cpu_percent: AtomicUsize::new(max_cpu_percent),
            current_cpu_percent: AtomicUsize::new(0),
            throttling_enabled: AtomicBool::new(true),
            batch_size: AtomicUsize::new(initial_batch_size),
            polling_interval: Arc::new(RwLock::new(initial_polling_interval)),
        }
    }

    pub fn update_current_cpu(&self, cpu_percent: f64) {
        self.current_cpu_percent.store(cpu_percent as usize, Ordering::Relaxed);
    }

    pub fn get_cpu_usage_percent(&self) -> f64 {
        self.current_cpu_percent.load(Ordering::Relaxed) as f64
    }

    pub fn should_throttle(&self) -> bool {
        if !self.throttling_enabled.load(Ordering::Relaxed) {
            return false;
        }

        let current = self.current_cpu_percent.load(Ordering::Relaxed);
        let max = self.max_cpu_percent.load(Ordering::Relaxed);
        
        current > max
    }

    pub async fn optimize_for_cpu_usage(&self) -> Result<()> {
        let current_cpu = self.get_cpu_usage_percent();
        let max_cpu = self.max_cpu_percent.load(Ordering::Relaxed) as f64;

        if current_cpu > max_cpu {
            // Reduce batch size to lower CPU usage
            let current_batch = self.batch_size.load(Ordering::Relaxed);
            let new_batch = (current_batch * 80 / 100).max(1); // Reduce by 20%
            self.batch_size.store(new_batch, Ordering::Relaxed);
            
            // Increase polling interval to reduce CPU usage
            {
                let mut interval = self.polling_interval.write().await;
                *interval = (*interval).mul_f64(1.2); // Increase by 20%
                if *interval > Duration::from_secs(10) {
                    *interval = Duration::from_secs(10); // Cap at 10 seconds
                }
            }
            
            info!("CPU optimization applied: batch_size={}, polling_interval={:?}", 
                  new_batch, *self.polling_interval.read().await);
        } else if current_cpu < max_cpu * 0.5 {
            // CPU usage is low, we can increase performance
            let current_batch = self.batch_size.load(Ordering::Relaxed);
            let new_batch = (current_batch * 110 / 100).min(1000); // Increase by 10%
            self.batch_size.store(new_batch, Ordering::Relaxed);
            
            // Decrease polling interval for better responsiveness
            {
                let mut interval = self.polling_interval.write().await;
                *interval = (*interval).mul_f64(0.9); // Decrease by 10%
                if *interval < Duration::from_millis(100) {
                    *interval = Duration::from_millis(100); // Minimum 100ms
                }
            }
            
            debug!("CPU optimization applied: batch_size={}, polling_interval={:?}", 
                   new_batch, *self.polling_interval.read().await);
        }

        Ok(())
    }

    pub fn get_batch_size(&self) -> usize {
        self.batch_size.load(Ordering::Relaxed)
    }

    pub async fn get_polling_interval(&self) -> Duration {
        *self.polling_interval.read().await
    }

    pub fn enable_throttling(&self, enabled: bool) {
        self.throttling_enabled.store(enabled, Ordering::Relaxed);
        info!("CPU throttling {}", if enabled { "enabled" } else { "disabled" });
    }
}

/// Network optimization manager
pub struct NetworkOptimizer {
    compression_enabled: AtomicBool,
    max_bandwidth_mbps: AtomicUsize,
    current_bandwidth_mbps: AtomicUsize,
    buffer_size: AtomicUsize,
}

impl NetworkOptimizer {
    pub fn new(max_bandwidth_mbps: usize, initial_buffer_size: usize) -> Self {
        Self {
            compression_enabled: AtomicBool::new(true),
            max_bandwidth_mbps: AtomicUsize::new(max_bandwidth_mbps),
            current_bandwidth_mbps: AtomicUsize::new(0),
            buffer_size: AtomicUsize::new(initial_buffer_size),
        }
    }

    pub fn enable_compression(&self, enabled: bool) {
        self.compression_enabled.store(enabled, Ordering::Relaxed);
        info!("Network compression {}", if enabled { "enabled" } else { "disabled" });
    }

    pub fn is_compression_enabled(&self) -> bool {
        self.compression_enabled.load(Ordering::Relaxed)
    }

    pub fn update_bandwidth_usage(&self, mbps: usize) {
        self.current_bandwidth_mbps.store(mbps, Ordering::Relaxed);
    }

    pub fn get_bandwidth_usage_percent(&self) -> f64 {
        let current = self.current_bandwidth_mbps.load(Ordering::Relaxed);
        let max = self.max_bandwidth_mbps.load(Ordering::Relaxed);
        
        if max == 0 {
            0.0
        } else {
            (current as f64 / max as f64) * 100.0
        }
    }

    pub fn optimize_buffer_size(&self) -> usize {
        let bandwidth_usage = self.get_bandwidth_usage_percent();
        let current_buffer = self.buffer_size.load(Ordering::Relaxed);
        
        let new_buffer = if bandwidth_usage > 80.0 {
            // High bandwidth usage, increase buffer size
            (current_buffer * 120 / 100).min(64 * 1024) // Max 64KB
        } else if bandwidth_usage < 20.0 {
            // Low bandwidth usage, decrease buffer size
            (current_buffer * 80 / 100).max(1024) // Min 1KB
        } else {
            current_buffer
        };
        
        if new_buffer != current_buffer {
            self.buffer_size.store(new_buffer, Ordering::Relaxed);
            debug!("Network buffer size optimized: {} bytes", new_buffer);
        }
        
        new_buffer
    }

    pub fn get_buffer_size(&self) -> usize {
        self.buffer_size.load(Ordering::Relaxed)
    }
}

/// Comprehensive system optimizer
pub struct SystemOptimizer {
    memory_optimizer: MemoryOptimizer,
    cpu_optimizer: CpuOptimizer,
    network_optimizer: NetworkOptimizer,
    optimization_enabled: AtomicBool,
}

impl SystemOptimizer {
    pub fn new(
        max_memory_mb: usize,
        max_cpu_percent: usize,
        max_bandwidth_mbps: usize,
        gc_interval: Duration,
        initial_batch_size: usize,
        initial_polling_interval: Duration,
        initial_buffer_size: usize,
    ) -> Self {
        Self {
            memory_optimizer: MemoryOptimizer::new(max_memory_mb, gc_interval),
            cpu_optimizer: CpuOptimizer::new(max_cpu_percent, initial_batch_size, initial_polling_interval),
            network_optimizer: NetworkOptimizer::new(max_bandwidth_mbps, initial_buffer_size),
            optimization_enabled: AtomicBool::new(true),
        }
    }

    pub fn enable_optimization(&self, enabled: bool) {
        self.optimization_enabled.store(enabled, Ordering::Relaxed);
        info!("System optimization {}", if enabled { "enabled" } else { "disabled" });
    }

    pub async fn optimize_based_on_metrics(&self, metrics: &PerformanceMetrics) -> Result<()> {
        if !self.optimization_enabled.load(Ordering::Relaxed) {
            return Ok(());
        }

        // Update current metrics
        self.memory_optimizer.update_current_memory(metrics.memory_usage_mb as usize);
        self.cpu_optimizer.update_current_cpu(metrics.cpu_usage_percent);
        
        // Calculate network bandwidth (simplified)
        let total_bytes = metrics.network_bytes_sent + metrics.network_bytes_received;
        let bandwidth_mbps = if metrics.uptime_seconds > 0 {
            (total_bytes as f64 / metrics.uptime_seconds as f64 / 1024.0 / 1024.0 * 8.0) as usize
        } else {
            0
        };
        self.network_optimizer.update_bandwidth_usage(bandwidth_mbps);

        // Apply optimizations
        self.memory_optimizer.maybe_trigger_gc().await?;
        self.cpu_optimizer.optimize_for_cpu_usage().await?;
        self.network_optimizer.optimize_buffer_size();

        Ok(())
    }

    pub fn get_memory_optimizer(&self) -> &MemoryOptimizer {
        &self.memory_optimizer
    }

    pub fn get_cpu_optimizer(&self) -> &CpuOptimizer {
        &self.cpu_optimizer
    }

    pub fn get_network_optimizer(&self) -> &NetworkOptimizer {
        &self.network_optimizer
    }

    /// Get optimization recommendations based on current metrics
    pub fn get_optimization_recommendations(&self, metrics: &PerformanceMetrics) -> Vec<String> {
        let mut recommendations = Vec::new();

        // Memory recommendations
        if metrics.memory_usage_percent > 90.0 {
            recommendations.push("Critical: Memory usage is very high. Consider reducing buffer sizes or clearing old data.".to_string());
        } else if metrics.memory_usage_percent > 80.0 {
            recommendations.push("Warning: Memory usage is high. Monitor for memory leaks.".to_string());
        }

        // CPU recommendations
        if metrics.cpu_usage_percent > 90.0 {
            recommendations.push("Critical: CPU usage is very high. Consider reducing polling frequency or batch sizes.".to_string());
        } else if metrics.cpu_usage_percent > 70.0 {
            recommendations.push("Warning: CPU usage is high. Consider enabling compression or optimizing algorithms.".to_string());
        }

        // Response time recommendations
        if metrics.average_response_time_ms > 5000.0 {
            recommendations.push("Critical: Response times are very slow. Check network connectivity and server load.".to_string());
        } else if metrics.average_response_time_ms > 1000.0 {
            recommendations.push("Warning: Response times are slow. Consider optimizing network buffers.".to_string());
        }

        // Connection recommendations
        if metrics.active_connections > 50 {
            recommendations.push("Info: High number of active connections. Monitor for connection leaks.".to_string());
        }

        // Error rate recommendations
        if metrics.error_count > 0 && metrics.uptime_seconds > 0 {
            let error_rate_per_minute = (metrics.error_count as f64 / metrics.uptime_seconds as f64) * 60.0;
            if error_rate_per_minute > 10.0 {
                recommendations.push("Critical: High error rate detected. Check logs for recurring issues.".to_string());
            } else if error_rate_per_minute > 1.0 {
                recommendations.push("Warning: Elevated error rate. Monitor for potential issues.".to_string());
            }
        }

        if recommendations.is_empty() {
            recommendations.push("System performance is within normal parameters.".to_string());
        }

        recommendations
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_memory_optimizer() {
        let optimizer = MemoryOptimizer::new(1000, Duration::from_millis(1)); // Very short interval
        
        optimizer.update_current_memory(500);
        assert_eq!(optimizer.get_memory_usage_percent(), 50.0);
        
        optimizer.update_current_memory(900);
        assert!(optimizer.should_trigger_gc());
        
        // Wait a bit to ensure GC interval has passed
        tokio::time::sleep(Duration::from_millis(2)).await;
        
        let gc_triggered = optimizer.maybe_trigger_gc().await.unwrap();
        assert!(gc_triggered);
    }

    #[tokio::test]
    async fn test_cpu_optimizer() {
        let optimizer = CpuOptimizer::new(80, 100, Duration::from_millis(500));
        
        optimizer.update_current_cpu(90.0);
        assert!(optimizer.should_throttle());
        
        optimizer.optimize_for_cpu_usage().await.unwrap();
        assert!(optimizer.get_batch_size() < 100); // Should be reduced
    }

    #[test]
    fn test_network_optimizer() {
        let optimizer = NetworkOptimizer::new(100, 8192);
        
        optimizer.update_bandwidth_usage(90);
        assert!(optimizer.get_bandwidth_usage_percent() > 80.0);
        
        let new_buffer_size = optimizer.optimize_buffer_size();
        assert!(new_buffer_size > 8192); // Should be increased due to high usage
    }

    #[tokio::test]
    async fn test_system_optimizer() {
        let optimizer = SystemOptimizer::new(
            1000, // max memory MB
            80,   // max CPU %
            100,  // max bandwidth Mbps
            Duration::from_secs(60), // GC interval
            100,  // initial batch size
            Duration::from_millis(500), // initial polling interval
            8192, // initial buffer size
        );

        let metrics = PerformanceMetrics {
            cpu_usage_percent: 85.0,
            memory_usage_mb: 900,
            memory_usage_percent: 90.0,
            network_bytes_sent: 1000000,
            network_bytes_received: 2000000,
            uptime_seconds: 60,
            ..Default::default()
        };

        optimizer.optimize_based_on_metrics(&metrics).await.unwrap();
        
        let recommendations = optimizer.get_optimization_recommendations(&metrics);
        assert!(!recommendations.is_empty());
        assert!(recommendations.iter().any(|r| r.contains("Memory usage is high")));
        assert!(recommendations.iter().any(|r| r.contains("CPU usage is high")));
    }
}