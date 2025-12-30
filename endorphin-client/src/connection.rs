//! Remote Connection Manager
//!
//! This module provides the client-side connection management functionality,
//! including automatic reconnection, error recovery, and connection state management.

use endorphin_common::{
    Result, RemoteAdbError, ConnectionConfig, ConnectionStatus, DeviceInfo, LogEntry, LogFilters,
    StreamHandle,
};
use endorphin_common::network::{NetworkClient, NetworkMessage};
use std::sync::Arc;
use std::time::{Duration, SystemTime};
use tokio::sync::{mpsc, Mutex, RwLock};
use tokio::time::{interval, sleep, timeout};
use tracing::{debug, error, info, warn};

/// Reconnection policy configuration
#[derive(Debug, Clone)]
pub struct ReconnectPolicy {
    /// Maximum number of reconnection attempts
    pub max_attempts: u32,
    /// Initial delay between reconnection attempts
    pub initial_delay: Duration,
    /// Maximum delay between reconnection attempts
    pub max_delay: Duration,
    /// Multiplier for exponential backoff
    pub backoff_multiplier: f64,
    /// Whether to enable automatic reconnection
    pub auto_reconnect: bool,
}

impl Default for ReconnectPolicy {
    fn default() -> Self {
        Self {
            max_attempts: 5,
            initial_delay: Duration::from_secs(1),
            max_delay: Duration::from_secs(60),
            backoff_multiplier: 2.0,
            auto_reconnect: true,
        }
    }
}

/// Connection state for the remote connection manager
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum ConnectionState {
    Disconnected,
    Connecting,
    Connected,
    Reconnecting,
    Failed,
}

/// Remote Connection Manager
/// 
/// Manages the client-side connection to the remote ADB server,
/// including automatic reconnection and error recovery.
#[derive(Debug)]
pub struct RemoteConnectionManager {
    config: ConnectionConfig,
    client: Arc<Mutex<Option<NetworkClient>>>,
    state: Arc<RwLock<ConnectionState>>,
    status: Arc<RwLock<ConnectionStatus>>,
    reconnect_policy: Arc<RwLock<ReconnectPolicy>>,
    reconnect_attempts: Arc<Mutex<u32>>,
    shutdown_tx: Option<mpsc::Sender<()>>,
    #[allow(dead_code)]
    shutdown_rx: Option<mpsc::Receiver<()>>,
    state_change_tx: mpsc::Sender<ConnectionState>,
    state_change_rx: Option<mpsc::Receiver<ConnectionState>>,
}

impl RemoteConnectionManager {
    /// Create a new remote connection manager
    pub fn new(config: ConnectionConfig) -> Self {
        let (shutdown_tx, shutdown_rx) = mpsc::channel(1);
        let (state_change_tx, state_change_rx) = mpsc::channel(100);

        Self {
            config,
            client: Arc::new(Mutex::new(None)),
            state: Arc::new(RwLock::new(ConnectionState::Disconnected)),
            status: Arc::new(RwLock::new(ConnectionStatus::default())),
            reconnect_policy: Arc::new(RwLock::new(ReconnectPolicy::default())),
            reconnect_attempts: Arc::new(Mutex::new(0)),
            shutdown_tx: Some(shutdown_tx),
            shutdown_rx: Some(shutdown_rx),
            state_change_tx,
            state_change_rx: Some(state_change_rx),
        }
    }

    /// Connect to the remote ADB server
    pub async fn connect(&self) -> Result<()> {
        self.set_state(ConnectionState::Connecting).await;
        
        let mut client = NetworkClient::new(self.config.clone());
        
        match client.connect().await {
            Ok(()) => {
                // Update connection status
                let mut status = self.status.write().await;
                status.is_connected = true;
                status.connection_time = Some(SystemTime::now());
                status.last_heartbeat = Some(SystemTime::now());
                status.error_message = None;

                // Store the client
                *self.client.lock().await = Some(client);
                
                // Reset reconnect attempts
                *self.reconnect_attempts.lock().await = 0;
                
                self.set_state(ConnectionState::Connected).await;
                info!("Successfully connected to remote ADB server at {}:{}", 
                      self.config.host, self.config.port);
                
                // Start heartbeat monitoring
                self.start_heartbeat_monitor().await;
                
                Ok(())
            }
            Err(e) => {
                let mut status = self.status.write().await;
                status.is_connected = false;
                status.error_message = Some(e.to_string());
                
                self.set_state(ConnectionState::Failed).await;
                error!("Failed to connect to remote ADB server: {}", e);
                Err(e)
            }
        }
    }

    /// Disconnect from the remote ADB server
    pub async fn disconnect(&self) -> Result<()> {
        info!("Disconnecting from remote ADB server");
        
        if let Some(mut client) = self.client.lock().await.take() {
            if let Err(e) = client.disconnect().await {
                warn!("Error during disconnect: {}", e);
            }
        }

        let mut status = self.status.write().await;
        status.is_connected = false;
        status.error_message = None;

        self.set_state(ConnectionState::Disconnected).await;
        info!("Disconnected from remote ADB server");
        Ok(())
    }

    /// Check if connected to the remote server
    pub async fn is_connected(&self) -> bool {
        if let Some(client) = self.client.lock().await.as_ref() {
            client.is_connected().await
        } else {
            false
        }
    }

    /// Get current connection status
    pub async fn get_connection_status(&self) -> ConnectionStatus {
        self.status.read().await.clone()
    }

    /// Get current connection state
    pub async fn get_connection_state(&self) -> ConnectionState {
        *self.state.read().await
    }

    /// Set reconnection policy
    pub async fn set_reconnect_policy(&self, policy: ReconnectPolicy) {
        let max_attempts = policy.max_attempts;
        let auto_reconnect = policy.auto_reconnect;
        *self.reconnect_policy.write().await = policy;
        info!("Updated reconnection policy: max_attempts={}, auto_reconnect={}", 
              max_attempts, auto_reconnect);
    }

    /// Get reconnection policy
    pub async fn get_reconnect_policy(&self) -> ReconnectPolicy {
        self.reconnect_policy.read().await.clone()
    }

    /// Start automatic reconnection if enabled
    pub async fn start_auto_reconnect(&self) {
        let policy = self.reconnect_policy.read().await.clone();
        if !policy.auto_reconnect {
            return;
        }

        let state = self.state.clone();
        let client = self.client.clone();
        let status = self.status.clone();
        let reconnect_attempts = self.reconnect_attempts.clone();
        let config = self.config.clone();
        let state_change_tx = self.state_change_tx.clone();

        tokio::spawn(async move {
            let mut current_delay = policy.initial_delay;
            
            loop {
                // Wait for connection failure
                let current_state = *state.read().await;
                if current_state != ConnectionState::Failed {
                    sleep(Duration::from_secs(1)).await;
                    continue;
                }

                let attempts = *reconnect_attempts.lock().await;
                if attempts >= policy.max_attempts {
                    error!("Maximum reconnection attempts ({}) reached", policy.max_attempts);
                    break;
                }

                info!("Attempting reconnection ({}/{})", attempts + 1, policy.max_attempts);
                
                // Set reconnecting state
                *state.write().await = ConnectionState::Reconnecting;
                let _ = state_change_tx.send(ConnectionState::Reconnecting).await;

                // Wait before reconnecting
                sleep(current_delay).await;

                // Attempt reconnection
                let mut new_client = NetworkClient::new(config.clone());
                match new_client.connect().await {
                    Ok(()) => {
                        // Successful reconnection
                        let mut status_guard = status.write().await;
                        status_guard.is_connected = true;
                        status_guard.connection_time = Some(SystemTime::now());
                        status_guard.last_heartbeat = Some(SystemTime::now());
                        status_guard.error_message = None;

                        *client.lock().await = Some(new_client);
                        *reconnect_attempts.lock().await = 0;
                        *state.write().await = ConnectionState::Connected;
                        let _ = state_change_tx.send(ConnectionState::Connected).await;

                        info!("Successfully reconnected to remote ADB server");
                        // Reset delay on successful connection
                        let _ = policy.initial_delay;
                        break;
                    }
                    Err(e) => {
                        // Failed reconnection
                        *reconnect_attempts.lock().await += 1;
                        let mut status_guard = status.write().await;
                        status_guard.error_message = Some(e.to_string());
                        
                        error!("Reconnection attempt failed: {}", e);
                        
                        // Exponential backoff
                        current_delay = Duration::from_secs_f64(
                            (current_delay.as_secs_f64() * policy.backoff_multiplier)
                                .min(policy.max_delay.as_secs_f64())
                        );
                    }
                }
            }
        });
    }

    /// Send a request to list available devices
    pub async fn list_devices(&self) -> Result<Vec<DeviceInfo>> {
        let client = self.client.lock().await;
        let client = client.as_ref()
            .ok_or_else(|| RemoteAdbError::client("Not connected to server"))?;

        // Send device list request
        client.send_message(&NetworkMessage::Data {
            payload: b"LIST_DEVICES".to_vec(),
        }).await?;

        // Wait for response
        let response_timeout = Duration::from_secs(10);
        match timeout(response_timeout, client.receive_message()).await {
            Ok(Ok(NetworkMessage::Data { payload })) => {
                // Parse device list from payload
                let devices: Vec<DeviceInfo> = serde_json::from_slice(&payload)
                    .map_err(|e| RemoteAdbError::network(format!("Failed to parse device list: {}", e)))?;
                
                info!("Received {} devices from server", devices.len());
                Ok(devices)
            }
            Ok(Ok(NetworkMessage::Error { message })) => {
                Err(RemoteAdbError::server(message))
            }
            Ok(Ok(_)) => {
                Err(RemoteAdbError::network("Unexpected response to device list request"))
            }
            Ok(Err(e)) => Err(e),
            Err(_) => Err(RemoteAdbError::Timeout),
        }
    }

    /// Start monitoring logs from a specific device
    pub async fn start_log_monitoring(&self, device_id: &str, filters: LogFilters) -> Result<StreamHandle> {
        let client = self.client.lock().await;
        let client = client.as_ref()
            .ok_or_else(|| RemoteAdbError::client("Not connected to server"))?;

        // Create log monitoring request
        let request = serde_json::json!({
            "action": "START_LOG_MONITORING",
            "device_id": device_id,
            "filters": filters
        });

        client.send_message(&NetworkMessage::Data {
            payload: request.to_string().into_bytes(),
        }).await?;

        // Wait for response
        let response_timeout = Duration::from_secs(10);
        match timeout(response_timeout, client.receive_message()).await {
            Ok(Ok(NetworkMessage::Data { payload })) => {
                let stream_handle: StreamHandle = serde_json::from_slice(&payload)
                    .map_err(|e| RemoteAdbError::network(format!("Failed to parse stream handle: {}", e)))?;
                
                info!("Started log monitoring for device {} with stream {}", device_id, stream_handle.stream_id);
                Ok(stream_handle)
            }
            Ok(Ok(NetworkMessage::Error { message })) => {
                Err(RemoteAdbError::server(message))
            }
            Ok(Ok(_)) => {
                Err(RemoteAdbError::network("Unexpected response to log monitoring request"))
            }
            Ok(Err(e)) => Err(e),
            Err(_) => Err(RemoteAdbError::Timeout),
        }
    }

    /// Stop monitoring logs for a specific stream
    pub async fn stop_log_monitoring(&self, stream_handle: &StreamHandle) -> Result<()> {
        let client = self.client.lock().await;
        let client = client.as_ref()
            .ok_or_else(|| RemoteAdbError::client("Not connected to server"))?;

        // Create stop monitoring request
        let request = serde_json::json!({
            "action": "STOP_LOG_MONITORING",
            "stream_id": stream_handle.stream_id
        });

        client.send_message(&NetworkMessage::Data {
            payload: request.to_string().into_bytes(),
        }).await?;

        // Wait for response
        let response_timeout = Duration::from_secs(5);
        match timeout(response_timeout, client.receive_message()).await {
            Ok(Ok(NetworkMessage::Data { payload })) => {
                let response: serde_json::Value = serde_json::from_slice(&payload)
                    .map_err(|e| RemoteAdbError::network(format!("Failed to parse response: {}", e)))?;
                
                if response["success"].as_bool().unwrap_or(false) {
                    info!("Stopped log monitoring for stream {}", stream_handle.stream_id);
                    Ok(())
                } else {
                    let message = response["message"].as_str().unwrap_or("Unknown error");
                    Err(RemoteAdbError::server(message.to_string()))
                }
            }
            Ok(Ok(NetworkMessage::Error { message })) => {
                Err(RemoteAdbError::server(message))
            }
            Ok(Ok(_)) => {
                Err(RemoteAdbError::network("Unexpected response to stop monitoring request"))
            }
            Ok(Err(e)) => Err(e),
            Err(_) => Err(RemoteAdbError::Timeout),
        }
    }

    /// Receive log entries from the server
    pub async fn receive_log_entries(&self) -> Result<Vec<LogEntry>> {
        let client = self.client.lock().await;
        let client = client.as_ref()
            .ok_or_else(|| RemoteAdbError::client("Not connected to server"))?;

        match client.receive_message().await? {
            NetworkMessage::Data { payload } => {
                let log_entries: Vec<LogEntry> = serde_json::from_slice(&payload)
                    .map_err(|e| RemoteAdbError::network(format!("Failed to parse log entries: {}", e)))?;
                
                debug!("Received {} log entries", log_entries.len());
                Ok(log_entries)
            }
            NetworkMessage::Error { message } => {
                Err(RemoteAdbError::server(message))
            }
            _ => {
                Err(RemoteAdbError::network("Unexpected message type"))
            }
        }
    }

    /// Get a receiver for connection state changes
    pub fn get_state_change_receiver(&mut self) -> Option<mpsc::Receiver<ConnectionState>> {
        self.state_change_rx.take()
    }

    /// Shutdown the connection manager
    pub async fn shutdown(&mut self) -> Result<()> {
        info!("Shutting down remote connection manager");
        
        if let Some(shutdown_tx) = self.shutdown_tx.take() {
            let _ = shutdown_tx.send(()).await;
        }

        self.disconnect().await?;
        Ok(())
    }

    /// Set connection state and notify listeners
    async fn set_state(&self, new_state: ConnectionState) {
        let mut state = self.state.write().await;
        if *state != new_state {
            *state = new_state;
            let _ = self.state_change_tx.send(new_state).await;
        }
    }

    /// Start heartbeat monitoring to detect connection failures
    async fn start_heartbeat_monitor(&self) {
        let client = self.client.clone();
        let state = self.state.clone();
        let status = self.status.clone();
        let state_change_tx = self.state_change_tx.clone();

        tokio::spawn(async move {
            let mut heartbeat_interval = interval(Duration::from_secs(30));
            let mut missed_heartbeats = 0;
            const MAX_MISSED_HEARTBEATS: u32 = 3;

            loop {
                heartbeat_interval.tick().await;

                let current_state = *state.read().await;
                if current_state != ConnectionState::Connected {
                    break;
                }

                let client_guard = client.lock().await;
                if let Some(client_ref) = client_guard.as_ref() {
                    // Send ping
                    match client_ref.send_message(&NetworkMessage::Ping).await {
                        Ok(()) => {
                            // Wait for pong with timeout
                            let pong_timeout = Duration::from_secs(10);
                            match timeout(pong_timeout, client_ref.receive_message()).await {
                                Ok(Ok(NetworkMessage::Pong)) => {
                                    // Heartbeat successful
                                    missed_heartbeats = 0;
                                    let mut status_guard = status.write().await;
                                    status_guard.last_heartbeat = Some(SystemTime::now());
                                }
                                _ => {
                                    missed_heartbeats += 1;
                                    warn!("Missed heartbeat ({}/{})", missed_heartbeats, MAX_MISSED_HEARTBEATS);
                                }
                            }
                        }
                        Err(e) => {
                            error!("Failed to send heartbeat: {}", e);
                            missed_heartbeats += 1;
                        }
                    }

                    if missed_heartbeats >= MAX_MISSED_HEARTBEATS {
                        error!("Connection lost - too many missed heartbeats");
                        let mut status_guard = status.write().await;
                        status_guard.is_connected = false;
                        status_guard.error_message = Some("Connection lost".to_string());
                        
                        *state.write().await = ConnectionState::Failed;
                        let _ = state_change_tx.send(ConnectionState::Failed).await;
                        break;
                    }
                } else {
                    break;
                }
            }
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_reconnect_policy_default() {
        let policy = ReconnectPolicy::default();
        assert_eq!(policy.max_attempts, 5);
        assert_eq!(policy.initial_delay, Duration::from_secs(1));
        assert_eq!(policy.max_delay, Duration::from_secs(60));
        assert_eq!(policy.backoff_multiplier, 2.0);
        assert!(policy.auto_reconnect);
    }

    #[test]
    fn test_connection_state_transitions() {
        assert_eq!(ConnectionState::Disconnected, ConnectionState::Disconnected);
        assert_ne!(ConnectionState::Disconnected, ConnectionState::Connected);
    }

    #[tokio::test]
    async fn test_connection_manager_creation() {
        let config = ConnectionConfig::default();
        let manager = RemoteConnectionManager::new(config);
        
        assert_eq!(manager.get_connection_state().await, ConnectionState::Disconnected);
        assert!(!manager.is_connected().await);
    }

    #[tokio::test]
    async fn test_reconnect_policy_update() {
        let config = ConnectionConfig::default();
        let manager = RemoteConnectionManager::new(config);
        
        let new_policy = ReconnectPolicy {
            max_attempts: 10,
            initial_delay: Duration::from_secs(2),
            max_delay: Duration::from_secs(120),
            backoff_multiplier: 1.5,
            auto_reconnect: false,
        };
        
        manager.set_reconnect_policy(new_policy.clone()).await;
        let retrieved_policy = manager.get_reconnect_policy().await;
        
        assert_eq!(retrieved_policy.max_attempts, new_policy.max_attempts);
        assert_eq!(retrieved_policy.initial_delay, new_policy.initial_delay);
        assert_eq!(retrieved_policy.auto_reconnect, new_policy.auto_reconnect);
    }
}