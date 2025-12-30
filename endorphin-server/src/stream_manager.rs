//! Log Stream Manager
//! 
//! This module manages multiple concurrent log streams from different devices,
//! handles stream lifecycle, and provides stream status monitoring.

use endorphin_common::{
    Result, RemoteAdbError, StreamHandle, StreamStatus, LogEntry, LogFilters, DeviceInfo,
    logcat::LogcatManager,
    AdbExecutor,
};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;
use std::time::SystemTime;
use tokio::sync::{mpsc, RwLock, Mutex};
use tokio::task::JoinHandle;
use tracing::{debug, error, info};

/// Stream configuration and metadata
#[derive(Debug, Clone)]
#[allow(dead_code)]
pub struct StreamInfo {
    pub handle: StreamHandle,
    pub status: StreamStatus,
    pub device_info: DeviceInfo,
    pub created_at: SystemTime,
    pub last_activity: SystemTime,
}

/// Stream statistics for monitoring
#[derive(Debug, Clone, Serialize, Deserialize)]
#[allow(dead_code)]
pub struct StreamStats {
    pub stream_id: String,
    pub device_id: String,
    pub is_active: bool,
    pub lines_captured: u64,
    pub bytes_transferred: u64,
    pub uptime_seconds: u64,
    pub last_activity: Option<SystemTime>,
    pub error_count: u64,
}

/// Log stream event types
#[derive(Debug, Clone)]
#[allow(dead_code)]
pub enum StreamEvent {
    /// Stream started successfully
    StreamStarted { stream_id: String, device_id: String },
    /// Stream stopped
    StreamStopped { stream_id: String, reason: String },
    /// Stream error occurred
    StreamError { stream_id: String, error: String },
    /// New log entry received
    LogEntry { stream_id: String, entry: LogEntry },
    /// Stream status updated
    StatusUpdate { stream_id: String, status: StreamStatus },
}

/// Log Stream Manager
/// 
/// Manages multiple concurrent log streams from different Android devices.
/// Provides stream lifecycle management, status monitoring, and resource cleanup.
pub struct LogStreamManager {
    adb_executor: Arc<AdbExecutor>,
    logcat_manager: Arc<LogcatManager>,
    active_streams: Arc<RwLock<HashMap<String, StreamInfo>>>,
    stream_tasks: Arc<RwLock<HashMap<String, JoinHandle<()>>>>,
    event_sender: Arc<Mutex<Option<mpsc::Sender<StreamEvent>>>>,
    #[allow(dead_code)]
    shutdown_tx: Option<mpsc::Sender<()>>,
    #[allow(dead_code)]
    shutdown_rx: Option<mpsc::Receiver<()>>,
    max_concurrent_streams: usize,
}

impl LogStreamManager {
    /// Create a new log stream manager
    pub fn new(adb_executor: Arc<AdbExecutor>, max_concurrent_streams: usize) -> Self {
        let (shutdown_tx, shutdown_rx) = mpsc::channel(1);
        
        Self {
            adb_executor: adb_executor.clone(),
            logcat_manager: Arc::new(LogcatManager::new((*adb_executor).clone())),
            active_streams: Arc::new(RwLock::new(HashMap::new())),
            stream_tasks: Arc::new(RwLock::new(HashMap::new())),
            event_sender: Arc::new(Mutex::new(None)),
            shutdown_tx: Some(shutdown_tx),
            shutdown_rx: Some(shutdown_rx),
            max_concurrent_streams,
        }
    }

    /// Set event sender for stream events
    pub async fn set_event_sender(&self, sender: mpsc::Sender<StreamEvent>) {
        let mut event_sender = self.event_sender.lock().await;
        *event_sender = Some(sender);
    }

    /// Start a new log stream for the specified device
    pub async fn start_log_stream(
        &self,
        device_id: String,
        filters: LogFilters,
    ) -> Result<StreamHandle> {
        info!("Starting log stream for device: {}", device_id);

        // Check if we've reached the maximum number of concurrent streams
        {
            let streams = self.active_streams.read().await;
            if streams.len() >= self.max_concurrent_streams {
                return Err(RemoteAdbError::server(
                    "Maximum number of concurrent streams reached"
                ));
            }
        }

        // Verify device is online
        let devices = self.adb_executor.get_devices().await?;
        let device_info = devices
            .into_iter()
            .find(|d| d.device_id == device_id)
            .ok_or_else(|| RemoteAdbError::client(format!("Device {} not found", device_id)))?;

        if device_info.status != endorphin_common::DeviceStatus::Online {
            return Err(RemoteAdbError::client(format!(
                "Device {} is not online (status: {:?})",
                device_id, device_info.status
            )));
        }

        // Create stream handle
        let stream_id = format!(
            "stream-{}-{}",
            device_id,
            SystemTime::now()
                .duration_since(SystemTime::UNIX_EPOCH)
                .unwrap_or_default()
                .as_millis()
        );

        let handle = StreamHandle {
            stream_id: stream_id.clone(),
            device_id: device_id.clone(),
            created_at: SystemTime::now(),
            filters: filters.clone(),
        };

        // Create stream info
        let stream_info = StreamInfo {
            handle: handle.clone(),
            status: StreamStatus {
                is_active: true,
                lines_captured: 0,
                bytes_transferred: 0,
                last_activity: Some(SystemTime::now()),
            },
            device_info: device_info.clone(),
            created_at: SystemTime::now(),
            last_activity: SystemTime::now(),
        };

        // Store stream info
        {
            let mut streams = self.active_streams.write().await;
            streams.insert(stream_id.clone(), stream_info);
        }

        // Start the log capture task
        let task_handle = self.start_stream_task(handle.clone(), filters).await?;

        // Store task handle
        {
            let mut tasks = self.stream_tasks.write().await;
            tasks.insert(stream_id.clone(), task_handle);
        }

        // Send stream started event
        self.send_event(StreamEvent::StreamStarted {
            stream_id: stream_id.clone(),
            device_id: device_id.clone(),
        }).await;

        info!("Log stream {} started for device {}", stream_id, device_id);
        Ok(handle)
    }

    /// Stop a log stream
    pub async fn stop_log_stream(&self, stream_id: &str) -> Result<()> {
        info!("Stopping log stream: {}", stream_id);

        // Remove and cancel the task
        let task_handle = {
            let mut tasks = self.stream_tasks.write().await;
            tasks.remove(stream_id)
        };

        if let Some(handle) = task_handle {
            handle.abort();
            debug!("Aborted task for stream {}", stream_id);
        }

        // Remove stream info
        let stream_info = {
            let mut streams = self.active_streams.write().await;
            streams.remove(stream_id)
        };

        if stream_info.is_some() {
            // Send stream stopped event
            self.send_event(StreamEvent::StreamStopped {
                stream_id: stream_id.to_string(),
                reason: "User requested".to_string(),
            }).await;

            info!("Log stream {} stopped", stream_id);
            Ok(())
        } else {
            Err(RemoteAdbError::client(format!("Stream {} not found", stream_id)))
        }
    }

    /// Get status of a specific stream
    pub async fn get_stream_status(&self, stream_id: &str) -> Result<StreamStatus> {
        let streams = self.active_streams.read().await;
        if let Some(stream_info) = streams.get(stream_id) {
            Ok(stream_info.status.clone())
        } else {
            Err(RemoteAdbError::client(format!("Stream {} not found", stream_id)))
        }
    }

    /// Get all active streams
    #[allow(dead_code)]
    pub async fn get_active_streams(&self) -> Vec<StreamHandle> {
        let streams = self.active_streams.read().await;
        streams.values().map(|info| info.handle.clone()).collect()
    }

    /// Get stream statistics
    #[allow(dead_code)]
    pub async fn get_stream_stats(&self, stream_id: &str) -> Result<StreamStats> {
        let streams = self.active_streams.read().await;
        if let Some(stream_info) = streams.get(stream_id) {
            let uptime = stream_info.created_at
                .elapsed()
                .unwrap_or_default()
                .as_secs();

            Ok(StreamStats {
                stream_id: stream_id.to_string(),
                device_id: stream_info.handle.device_id.clone(),
                is_active: stream_info.status.is_active,
                lines_captured: stream_info.status.lines_captured,
                bytes_transferred: stream_info.status.bytes_transferred,
                uptime_seconds: uptime,
                last_activity: stream_info.status.last_activity,
                error_count: 0, // TODO: Track error count
            })
        } else {
            Err(RemoteAdbError::client(format!("Stream {} not found", stream_id)))
        }
    }

    /// Get all stream statistics
    #[allow(dead_code)]
    pub async fn get_all_stream_stats(&self) -> Vec<StreamStats> {
        let streams = self.active_streams.read().await;
        let mut stats = Vec::new();

        for (stream_id, stream_info) in streams.iter() {
            let uptime = stream_info.created_at
                .elapsed()
                .unwrap_or_default()
                .as_secs();

            stats.push(StreamStats {
                stream_id: stream_id.clone(),
                device_id: stream_info.handle.device_id.clone(),
                is_active: stream_info.status.is_active,
                lines_captured: stream_info.status.lines_captured,
                bytes_transferred: stream_info.status.bytes_transferred,
                uptime_seconds: uptime,
                last_activity: stream_info.status.last_activity,
                error_count: 0, // TODO: Track error count
            });
        }

        stats
    }

    /// Stop all active streams
    #[allow(dead_code)]
    pub async fn stop_all_streams(&self) -> Result<()> {
        info!("Stopping all active streams");

        let stream_ids: Vec<String> = {
            let streams = self.active_streams.read().await;
            streams.keys().cloned().collect()
        };

        for stream_id in stream_ids {
            if let Err(e) = self.stop_log_stream(&stream_id).await {
                error!("Failed to stop stream {}: {}", stream_id, e);
            }
        }

        info!("All streams stopped");
        Ok(())
    }

    /// Cleanup inactive streams
    #[allow(dead_code)]
    pub async fn cleanup_inactive_streams(&self) {
        let mut to_remove = Vec::new();

        {
            let streams = self.active_streams.read().await;
            for (stream_id, stream_info) in streams.iter() {
                if !stream_info.status.is_active {
                    to_remove.push(stream_id.clone());
                }
            }
        }

        for stream_id in to_remove {
            if let Err(e) = self.stop_log_stream(&stream_id).await {
                error!("Failed to cleanup inactive stream {}: {}", stream_id, e);
            }
        }
    }

    /// Start the stream monitoring task
    async fn start_stream_task(
        &self,
        handle: StreamHandle,
        filters: LogFilters,
    ) -> Result<JoinHandle<()>> {
        let logcat_manager = self.logcat_manager.clone();
        let active_streams = self.active_streams.clone();
        let event_sender = self.event_sender.clone();
        let stream_id = handle.stream_id.clone();
        let device_id = handle.device_id.clone();

        let task_handle = tokio::spawn(async move {
            debug!("Starting stream task for {}", stream_id);

            match logcat_manager.start_log_capture(device_id.clone(), filters).await {
                Ok(_stream_handle) => {
                    info!("Logcat stream started for device {}", device_id);

                    // For now, we'll simulate log processing
                    // In a real implementation, we would need to properly integrate with the logcat manager
                    // to receive log entries through a proper channel or callback mechanism
                    
                    // TODO: Implement proper log entry streaming
                    // This is a placeholder that would need to be replaced with actual logcat integration
                    tokio::time::sleep(std::time::Duration::from_millis(100)).await;
                    
                    info!("Log stream processing started for device {}", device_id);
                }
                Err(e) => {
                    error!("Failed to start logcat stream for device {}: {}", device_id, e);
                    
                    // Send error event
                    if let Some(sender) = event_sender.lock().await.as_ref() {
                        let _ = sender.send(StreamEvent::StreamError {
                            stream_id: stream_id.clone(),
                            error: e.to_string(),
                        }).await;
                    }
                }
            }

            // Mark stream as inactive
            {
                let mut streams = active_streams.write().await;
                if let Some(stream_info) = streams.get_mut(&stream_id) {
                    stream_info.status.is_active = false;
                }
            }

            debug!("Stream task completed for {}", stream_id);
        });

        Ok(task_handle)
    }

    /// Send a stream event
    async fn send_event(&self, event: StreamEvent) {
        if let Some(sender) = self.event_sender.lock().await.as_ref() {
            if let Err(e) = sender.send(event).await {
                error!("Failed to send stream event: {}", e);
            }
        }
    }

    /// Shutdown the stream manager
    #[allow(dead_code)]
    pub async fn shutdown(&mut self) -> Result<()> {
        info!("Shutting down log stream manager");

        // Stop all active streams
        self.stop_all_streams().await?;

        // Send shutdown signal
        if let Some(shutdown_tx) = self.shutdown_tx.take() {
            let _ = shutdown_tx.send(()).await;
        }

        info!("Log stream manager shutdown complete");
        Ok(())
    }

    /// Get manager statistics
    #[allow(dead_code)]
    pub async fn get_manager_stats(&self) -> ManagerStats {
        let streams = self.active_streams.read().await;
        let active_count = streams.values().filter(|s| s.status.is_active).count();
        let total_lines = streams.values().map(|s| s.status.lines_captured).sum();
        let total_bytes = streams.values().map(|s| s.status.bytes_transferred).sum();

        ManagerStats {
            total_streams: streams.len(),
            active_streams: active_count,
            max_concurrent_streams: self.max_concurrent_streams,
            total_lines_captured: total_lines,
            total_bytes_transferred: total_bytes,
        }
    }
}

/// Manager statistics
#[derive(Debug, Clone, Serialize, Deserialize)]
#[allow(dead_code)]
pub struct ManagerStats {
    pub total_streams: usize,
    pub active_streams: usize,
    pub max_concurrent_streams: usize,
    pub total_lines_captured: u64,
    pub total_bytes_transferred: u64,
}

#[cfg(test)]
mod tests {
    use super::*;
    use endorphin_common::DeviceStatus;

    #[tokio::test]
    async fn test_stream_manager_creation() {
        let adb_executor = Arc::new(AdbExecutor::new());
        let manager = LogStreamManager::new(adb_executor, 5);
        
        let stats = manager.get_manager_stats().await;
        assert_eq!(stats.total_streams, 0);
        assert_eq!(stats.active_streams, 0);
        assert_eq!(stats.max_concurrent_streams, 5);
    }

    #[tokio::test]
    async fn test_stream_info_creation() {
        let handle = StreamHandle {
            stream_id: "test-stream".to_string(),
            device_id: "test-device".to_string(),
            created_at: SystemTime::now(),
            filters: LogFilters::default(),
        };

        let device_info = DeviceInfo {
            device_id: "test-device".to_string(),
            device_name: "Test Device".to_string(),
            android_version: "13".to_string(),
            api_level: 33,
            status: DeviceStatus::Online,
            emulator_port: Some(5554),
        };

        let stream_info = StreamInfo {
            handle: handle.clone(),
            status: StreamStatus {
                is_active: true,
                lines_captured: 0,
                bytes_transferred: 0,
                last_activity: Some(SystemTime::now()),
            },
            device_info,
            created_at: SystemTime::now(),
            last_activity: SystemTime::now(),
        };

        assert_eq!(stream_info.handle.stream_id, "test-stream");
        assert_eq!(stream_info.handle.device_id, "test-device");
        assert!(stream_info.status.is_active);
    }

    #[test]
    fn test_stream_stats_serialization() {
        let stats = StreamStats {
            stream_id: "test-stream".to_string(),
            device_id: "test-device".to_string(),
            is_active: true,
            lines_captured: 100,
            bytes_transferred: 5000,
            uptime_seconds: 60,
            last_activity: Some(SystemTime::now()),
            error_count: 0,
        };

        let json = serde_json::to_string(&stats).unwrap();
        let deserialized: StreamStats = serde_json::from_str(&json).unwrap();
        
        assert_eq!(stats.stream_id, deserialized.stream_id);
        assert_eq!(stats.device_id, deserialized.device_id);
        assert_eq!(stats.lines_captured, deserialized.lines_captured);
    }

    #[test]
    fn test_stream_event_variants() {
        let event1 = StreamEvent::StreamStarted {
            stream_id: "stream1".to_string(),
            device_id: "device1".to_string(),
        };

        let event2 = StreamEvent::StreamStopped {
            stream_id: "stream1".to_string(),
            reason: "User requested".to_string(),
        };

        let event3 = StreamEvent::StreamError {
            stream_id: "stream1".to_string(),
            error: "Connection lost".to_string(),
        };

        match event1 {
            StreamEvent::StreamStarted { stream_id, device_id } => {
                assert_eq!(stream_id, "stream1");
                assert_eq!(device_id, "device1");
            }
            _ => panic!("Wrong event type"),
        }

        match event2 {
            StreamEvent::StreamStopped { stream_id, reason } => {
                assert_eq!(stream_id, "stream1");
                assert_eq!(reason, "User requested");
            }
            _ => panic!("Wrong event type"),
        }

        match event3 {
            StreamEvent::StreamError { stream_id, error } => {
                assert_eq!(stream_id, "stream1");
                assert_eq!(error, "Connection lost");
            }
            _ => panic!("Wrong event type"),
        }
    }
}