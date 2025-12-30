//! System status monitoring and notification system

use crate::{Result, RemoteAdbError, DeviceInfo};
use serde::{Serialize, Deserialize};
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::{SystemTime, UNIX_EPOCH, Duration};
use tokio::sync::broadcast;
use tokio::time::interval;
use tracing::{info, warn, error, debug};

/// System component types for monitoring
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum ComponentType {
    AdbServer,
    NetworkBridge,
    LogStream,
    Device,
    Connection,
    Authentication,
    Storage,
}

/// System status levels
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub enum SystemStatus {
    Healthy,
    Warning,
    Critical,
    Offline,
}

/// Component status information
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ComponentStatus {
    pub component_type: ComponentType,
    pub component_id: String,
    pub status: SystemStatus,
    pub message: String,
    pub last_updated: u64,
    pub metadata: HashMap<String, String>,
}

impl ComponentStatus {
    pub fn new(
        component_type: ComponentType,
        component_id: String,
        status: SystemStatus,
        message: String,
    ) -> Self {
        Self {
            component_type,
            component_id,
            status,
            message,
            last_updated: SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs(),
            metadata: HashMap::new(),
        }
    }

    pub fn with_metadata<K: Into<String>, V: Into<String>>(mut self, key: K, value: V) -> Self {
        self.metadata.insert(key.into(), value.into());
        self
    }

    pub fn update_status(&mut self, status: SystemStatus, message: String) {
        self.status = status;
        self.message = message;
        self.last_updated = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();
    }
}

/// System status change event
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StatusChangeEvent {
    pub component_status: ComponentStatus,
    pub previous_status: Option<SystemStatus>,
    pub timestamp: u64,
    pub event_id: String,
}

impl StatusChangeEvent {
    pub fn new(component_status: ComponentStatus, previous_status: Option<SystemStatus>) -> Self {
        Self {
            component_status,
            previous_status,
            timestamp: SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs(),
            event_id: uuid::Uuid::new_v4().to_string(),
        }
    }
}

/// Notification types
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum NotificationType {
    StatusChange(StatusChangeEvent),
    DeviceConnected(DeviceInfo),
    DeviceDisconnected(String), // device_id
    ConnectionEstablished(String), // connection_id
    ConnectionLost(String), // connection_id
    StreamStarted(String), // stream_id
    StreamStopped(String), // stream_id
    ErrorOccurred(RemoteAdbError),
    PerformanceAlert { component: String, metric: String, value: f64, threshold: f64 },
}

/// Notification message
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Notification {
    pub id: String,
    pub notification_type: NotificationType,
    pub severity: NotificationSeverity,
    pub timestamp: u64,
    pub message: String,
}

/// Notification severity levels
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub enum NotificationSeverity {
    Info,
    Warning,
    Error,
    Critical,
}

impl Notification {
    pub fn new(notification_type: NotificationType, message: String) -> Self {
        let severity = match &notification_type {
            NotificationType::StatusChange(event) => match event.component_status.status {
                SystemStatus::Healthy => NotificationSeverity::Info,
                SystemStatus::Warning => NotificationSeverity::Warning,
                SystemStatus::Critical => NotificationSeverity::Error,
                SystemStatus::Offline => NotificationSeverity::Critical,
            },
            NotificationType::DeviceConnected(_) | NotificationType::ConnectionEstablished(_) |
            NotificationType::StreamStarted(_) => NotificationSeverity::Info,
            NotificationType::DeviceDisconnected(_) | NotificationType::ConnectionLost(_) |
            NotificationType::StreamStopped(_) => NotificationSeverity::Warning,
            NotificationType::ErrorOccurred(_) => NotificationSeverity::Error,
            NotificationType::PerformanceAlert { .. } => NotificationSeverity::Warning,
        };

        Self {
            id: uuid::Uuid::new_v4().to_string(),
            notification_type,
            severity,
            timestamp: SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs(),
            message,
        }
    }
}

/// System status monitor
#[derive(Debug)]
pub struct SystemStatusMonitor {
    component_statuses: Arc<Mutex<HashMap<String, ComponentStatus>>>,
    notification_tx: broadcast::Sender<Notification>,
    _notification_rx: broadcast::Receiver<Notification>,
    health_check_interval: Duration,
    performance_thresholds: Arc<Mutex<HashMap<String, f64>>>,
}

impl SystemStatusMonitor {
    pub fn new(health_check_interval: Duration) -> Self {
        let (notification_tx, notification_rx) = broadcast::channel(1000);
        
        Self {
            component_statuses: Arc::new(Mutex::new(HashMap::new())),
            notification_tx,
            _notification_rx: notification_rx,
            health_check_interval,
            performance_thresholds: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    /// Subscribe to notifications
    pub fn subscribe(&self) -> broadcast::Receiver<Notification> {
        self.notification_tx.subscribe()
    }

    /// Update component status
    pub fn update_component_status(&self, mut new_status: ComponentStatus) -> Result<()> {
        let component_key = format!("{}:{}", 
            format!("{:?}", new_status.component_type).to_lowercase(),
            new_status.component_id
        );

        let previous_status = {
            let mut statuses = self.component_statuses.lock()
                .map_err(|_| RemoteAdbError::server("Failed to acquire status lock"))?;
            
            let previous = statuses.get(&component_key).map(|s| s.status);
            new_status.last_updated = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs();
            
            statuses.insert(component_key.clone(), new_status.clone());
            previous
        };

        // Send notification if status changed
        if previous_status != Some(new_status.status) {
            let event = StatusChangeEvent::new(new_status.clone(), previous_status);
            let message = format!(
                "Component {} status changed from {:?} to {:?}: {}",
                component_key,
                previous_status.unwrap_or(SystemStatus::Offline),
                new_status.status,
                new_status.message
            );
            
            let notification = Notification::new(
                NotificationType::StatusChange(event),
                message
            );

            if let Err(e) = self.notification_tx.send(notification) {
                warn!("Failed to send status change notification: {}", e);
            }

            info!(
                component = %component_key,
                old_status = ?previous_status,
                new_status = ?new_status.status,
                "Component status updated"
            );
        }

        Ok(())
    }

    /// Get current status of a component
    pub fn get_component_status(&self, component_type: ComponentType, component_id: &str) -> Option<ComponentStatus> {
        let component_key = format!("{}:{}", 
            format!("{:?}", component_type).to_lowercase(),
            component_id
        );

        self.component_statuses.lock()
            .ok()?
            .get(&component_key)
            .cloned()
    }

    /// Get all component statuses
    pub fn get_all_statuses(&self) -> HashMap<String, ComponentStatus> {
        match self.component_statuses.lock() {
            Ok(statuses) => statuses.clone(),
            Err(_) => {
                error!("Failed to acquire status lock");
                HashMap::new()
            }
        }
    }

    /// Get overall system status
    pub fn get_system_status(&self) -> SystemStatus {
        let statuses = match self.component_statuses.lock() {
            Ok(statuses) => statuses,
            Err(_) => return SystemStatus::Critical,
        };

        if statuses.is_empty() {
            return SystemStatus::Offline;
        }

        let mut has_critical = false;
        let mut has_warning = false;

        for status in statuses.values() {
            match status.status {
                SystemStatus::Critical | SystemStatus::Offline => has_critical = true,
                SystemStatus::Warning => has_warning = true,
                SystemStatus::Healthy => {}
            }
        }

        if has_critical {
            SystemStatus::Critical
        } else if has_warning {
            SystemStatus::Warning
        } else {
            SystemStatus::Healthy
        }
    }

    /// Send a notification
    pub fn send_notification(&self, notification_type: NotificationType, message: String) -> Result<()> {
        let notification = Notification::new(notification_type, message);
        
        match notification.severity {
            NotificationSeverity::Critical => error!("Critical notification: {}", notification.message),
            NotificationSeverity::Error => error!("Error notification: {}", notification.message),
            NotificationSeverity::Warning => warn!("Warning notification: {}", notification.message),
            NotificationSeverity::Info => info!("Info notification: {}", notification.message),
        }

        self.notification_tx.send(notification)
            .map_err(|e| RemoteAdbError::server(format!("Failed to send notification: {}", e)))?;

        Ok(())
    }

    /// Set performance threshold for monitoring
    pub fn set_performance_threshold(&self, metric: String, threshold: f64) -> Result<()> {
        self.performance_thresholds.lock()
            .map_err(|_| RemoteAdbError::server("Failed to acquire thresholds lock"))?
            .insert(metric, threshold);
        Ok(())
    }

    /// Check performance metric against threshold
    pub fn check_performance_metric(&self, component: String, metric: String, value: f64) -> Result<()> {
        let threshold = {
            self.performance_thresholds.lock()
                .map_err(|_| RemoteAdbError::server("Failed to acquire thresholds lock"))?
                .get(&metric)
                .copied()
        };

        if let Some(threshold) = threshold {
            if value > threshold {
                let notification_type = NotificationType::PerformanceAlert {
                    component: component.clone(),
                    metric: metric.clone(),
                    value,
                    threshold,
                };
                
                let message = format!(
                    "Performance alert: {} {} = {:.2} exceeds threshold {:.2}",
                    component, metric, value, threshold
                );

                self.send_notification(notification_type, message)?;
            }
        }

        Ok(())
    }

    /// Start periodic health checks
    pub async fn start_health_monitoring(&self) -> Result<()> {
        let mut interval = interval(self.health_check_interval);
        let statuses = Arc::clone(&self.component_statuses);
        let notification_tx = self.notification_tx.clone();

        tokio::spawn(async move {
            loop {
                interval.tick().await;
                
                let current_time = SystemTime::now()
                    .duration_since(UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_secs();

                let mut stale_components = Vec::new();
                
                // Check for stale components
                if let Ok(statuses_guard) = statuses.lock() {
                    for (key, status) in statuses_guard.iter() {
                        let age = current_time.saturating_sub(status.last_updated);
                        
                        // Consider component stale if not updated in 5 minutes
                        if age > 300 && status.status != SystemStatus::Offline {
                            stale_components.push((key.clone(), status.clone()));
                        }
                    }
                }

                // Mark stale components as offline
                for (key, mut status) in stale_components {
                    status.update_status(SystemStatus::Offline, "Component appears to be offline".to_string());
                    
                    let event = StatusChangeEvent::new(status.clone(), Some(SystemStatus::Healthy));
                    let message = format!("Component {} marked as offline due to inactivity", key);
                    
                    let notification = Notification::new(
                        NotificationType::StatusChange(event),
                        message
                    );

                    if let Err(e) = notification_tx.send(notification) {
                        warn!("Failed to send stale component notification: {}", e);
                    }

                    // Update the status in the map
                    if let Ok(mut statuses_guard) = statuses.lock() {
                        statuses_guard.insert(key, status);
                    }
                }

                debug!("Health check completed");
            }
        });

        info!("Health monitoring started with interval: {:?}", self.health_check_interval);
        Ok(())
    }

    /// Clear all component statuses
    pub fn clear_all_statuses(&self) -> Result<()> {
        self.component_statuses.lock()
            .map_err(|_| RemoteAdbError::server("Failed to acquire status lock"))?
            .clear();
        
        info!("All component statuses cleared");
        Ok(())
    }
}

/// Global system status monitor instance
static GLOBAL_STATUS_MONITOR: std::sync::OnceLock<SystemStatusMonitor> = std::sync::OnceLock::new();

/// Get the global status monitor instance
pub fn global_status_monitor() -> &'static SystemStatusMonitor {
    GLOBAL_STATUS_MONITOR.get_or_init(|| {
        SystemStatusMonitor::new(Duration::from_secs(60)) // Default 1-minute health checks
    })
}

/// Initialize the global status monitor with custom configuration
pub fn init_global_status_monitor(health_check_interval: Duration) {
    let _ = GLOBAL_STATUS_MONITOR.set(SystemStatusMonitor::new(health_check_interval));
}

/// Convenience function to update component status globally
pub fn update_component_status(status: ComponentStatus) -> Result<()> {
    global_status_monitor().update_component_status(status)
}

/// Convenience function to send notifications globally
pub fn send_notification(notification_type: NotificationType, message: String) -> Result<()> {
    global_status_monitor().send_notification(notification_type, message)
}

/// Convenience function to get system status globally
pub fn get_system_status() -> SystemStatus {
    global_status_monitor().get_system_status()
}

/// Helper functions for common status updates
pub mod helpers {
    use super::*;

    pub fn report_device_connected(device: DeviceInfo) -> Result<()> {
        // Update device status
        let status = ComponentStatus::new(
            ComponentType::Device,
            device.device_id.clone(),
            SystemStatus::Healthy,
            format!("Device {} connected", device.device_name),
        ).with_metadata("android_version", device.android_version.clone())
         .with_metadata("api_level", device.api_level.to_string());

        update_component_status(status)?;

        // Send notification
        send_notification(
            NotificationType::DeviceConnected(device.clone()),
            format!("Device {} ({}) connected", device.device_name, device.device_id),
        )?;

        Ok(())
    }

    pub fn report_device_disconnected(device_id: String) -> Result<()> {
        // Update device status
        let status = ComponentStatus::new(
            ComponentType::Device,
            device_id.clone(),
            SystemStatus::Offline,
            "Device disconnected".to_string(),
        );

        update_component_status(status)?;

        // Send notification
        send_notification(
            NotificationType::DeviceDisconnected(device_id.clone()),
            format!("Device {} disconnected", device_id),
        )?;

        Ok(())
    }

    pub fn report_connection_established(connection_id: String, client_info: String) -> Result<()> {
        let status = ComponentStatus::new(
            ComponentType::Connection,
            connection_id.clone(),
            SystemStatus::Healthy,
            format!("Connection established with {}", client_info),
        ).with_metadata("client_info", client_info);

        update_component_status(status)?;

        send_notification(
            NotificationType::ConnectionEstablished(connection_id.clone()),
            format!("Connection {} established", connection_id),
        )?;

        Ok(())
    }

    pub fn report_connection_lost(connection_id: String, reason: String) -> Result<()> {
        let status = ComponentStatus::new(
            ComponentType::Connection,
            connection_id.clone(),
            SystemStatus::Offline,
            format!("Connection lost: {}", reason),
        ).with_metadata("disconnect_reason", reason);

        update_component_status(status)?;

        send_notification(
            NotificationType::ConnectionLost(connection_id.clone()),
            format!("Connection {} lost", connection_id),
        )?;

        Ok(())
    }

    pub fn report_stream_started(stream_id: String, device_id: String) -> Result<()> {
        let status = ComponentStatus::new(
            ComponentType::LogStream,
            stream_id.clone(),
            SystemStatus::Healthy,
            format!("Log stream started for device {}", device_id),
        ).with_metadata("device_id", device_id);

        update_component_status(status)?;

        send_notification(
            NotificationType::StreamStarted(stream_id.clone()),
            format!("Log stream {} started", stream_id),
        )?;

        Ok(())
    }

    pub fn report_stream_stopped(stream_id: String, reason: String) -> Result<()> {
        let status = ComponentStatus::new(
            ComponentType::LogStream,
            stream_id.clone(),
            SystemStatus::Offline,
            format!("Log stream stopped: {}", reason),
        ).with_metadata("stop_reason", reason);

        update_component_status(status)?;

        send_notification(
            NotificationType::StreamStopped(stream_id.clone()),
            format!("Log stream {} stopped", stream_id),
        )?;

        Ok(())
    }

    pub fn report_adb_server_status(status: SystemStatus, message: String) -> Result<()> {
        let component_status = ComponentStatus::new(
            ComponentType::AdbServer,
            "main".to_string(),
            status,
            message,
        );

        update_component_status(component_status)?;
        Ok(())
    }

    pub fn report_network_bridge_status(status: SystemStatus, message: String) -> Result<()> {
        let component_status = ComponentStatus::new(
            ComponentType::NetworkBridge,
            "main".to_string(),
            status,
            message,
        );

        update_component_status(component_status)?;
        Ok(())
    }
}