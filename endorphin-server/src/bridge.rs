//! Network Bridge Server
//! 
//! This module implements the network bridge that accepts remote connections
//! and forwards ADB requests to the local ADB server.

use crate::stream_manager::{LogStreamManager, StreamEvent};
use endorphin_common::{
    Result, RemoteAdbError, ClientConnection, DeviceInfo, LogFilters, StreamHandle,
    network::{NetworkServer, AuthToken},
    AdbExecutor,
};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::net::{IpAddr, SocketAddr};
use std::sync::Arc;
use std::time::SystemTime;
use tokio::sync::{mpsc, RwLock};
use tracing::{debug, error, info, warn};

/// Type alias for request and response channel
type RequestChannel = mpsc::Sender<(BridgeRequest, mpsc::Sender<BridgeResponse>)>;
type ResponseChannel = mpsc::Receiver<(BridgeRequest, mpsc::Sender<BridgeResponse>)>;

/// Request types that can be sent to the bridge server
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum BridgeRequest {
    /// List available devices
    ListDevices,
    /// Get device information
    GetDeviceInfo { device_id: String },
    /// Start log stream for a device
    StartLogStream { device_id: String, filters: LogFilters },
    /// Stop log stream
    StopLogStream { stream_id: String },
    /// Get stream status
    GetStreamStatus { stream_id: String },
    /// Execute ADB command
    ExecuteCommand { device_id: String, command: Vec<String> },
}

/// Response types from the bridge server
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum BridgeResponse {
    /// Device list response
    DeviceList { devices: Vec<DeviceInfo> },
    /// Device info response
    DeviceInfo { device: DeviceInfo },
    /// Stream started response
    StreamStarted { handle: StreamHandle },
    /// Stream stopped response
    StreamStopped { stream_id: String },
    /// Stream status response
    StreamStatus { status: endorphin_common::StreamStatus },
    /// Command execution response
    CommandResult { output: String, exit_code: i32 },
    /// Error response
    Error { message: String },
    /// Success response
    Success { message: String },
}

/// Configuration for the bridge server
#[derive(Debug, Clone)]
pub struct BridgeConfig {
    pub bind_address: String,
    pub port: u16,
    pub max_connections: usize,
    pub enable_auth: bool,
    pub auth_token: Option<String>,
    pub ip_whitelist: Vec<IpAddr>,
}

impl Default for BridgeConfig {
    fn default() -> Self {
        Self {
            bind_address: "0.0.0.0".to_string(),
            port: 5555,
            max_connections: 10,
            enable_auth: false,
            auth_token: None,
            ip_whitelist: Vec::new(),
        }
    }
}

/// Active client session information
#[derive(Debug, Clone)]
#[allow(dead_code)]
pub struct ClientSession {
    pub connection_id: String,
    pub client_ip: String,
    pub connected_at: SystemTime,
    pub is_authenticated: bool,
    pub active_streams: Vec<String>,
    pub last_activity: SystemTime,
}

/// Network Bridge Server
/// 
/// This server accepts incoming connections from remote clients and bridges
/// their requests to the local ADB server. It manages multiple client connections,
/// handles authentication, and routes requests appropriately.
pub struct NetworkBridge {
    config: BridgeConfig,
    network_server: NetworkServer,
    adb_executor: Arc<AdbExecutor>,
    stream_manager: Arc<LogStreamManager>,
    client_sessions: Arc<RwLock<HashMap<String, ClientSession>>>,
    active_streams: Arc<RwLock<HashMap<String, StreamHandle>>>,
    #[allow(dead_code)]
    request_handlers: Arc<RwLock<HashMap<String, RequestChannel>>>,
    shutdown_tx: Option<mpsc::Sender<()>>,
    shutdown_rx: Option<mpsc::Receiver<()>>,
}

impl NetworkBridge {
    /// Create a new network bridge with the given configuration
    pub fn new(config: BridgeConfig) -> Self {
        let (shutdown_tx, shutdown_rx) = mpsc::channel(1);
        let network_server = NetworkServer::new(config.max_connections);
        let adb_executor = Arc::new(AdbExecutor::new());
        let stream_manager = Arc::new(LogStreamManager::new(adb_executor.clone(), config.max_connections));
        
        Self {
            config,
            network_server,
            adb_executor,
            stream_manager,
            client_sessions: Arc::new(RwLock::new(HashMap::new())),
            active_streams: Arc::new(RwLock::new(HashMap::new())),
            request_handlers: Arc::new(RwLock::new(HashMap::new())),
            shutdown_tx: Some(shutdown_tx),
            shutdown_rx: Some(shutdown_rx),
        }
    }

    /// Start the bridge server
    pub async fn start(&mut self) -> Result<()> {
        info!("Starting Network Bridge Server");
        
        // Configure IP whitelist
        let ip_whitelist = self.network_server.get_ip_whitelist();
        for ip in &self.config.ip_whitelist {
            ip_whitelist.add_ip(*ip).await;
        }

        // Configure authentication if enabled
        if self.config.enable_auth {
            if let Some(token) = &self.config.auth_token {
                let auth_token = AuthToken::new(token.clone());
                self.network_server.add_auth_token(token.clone(), auth_token).await;
                info!("Authentication enabled with token");
            } else {
                warn!("Authentication enabled but no token provided");
            }
        }

        // Bind to address
        let bind_addr: SocketAddr = format!("{}:{}", self.config.bind_address, self.config.port)
            .parse()
            .map_err(|e| RemoteAdbError::config(format!("Invalid bind address: {}", e)))?;

        self.network_server.bind(bind_addr).await?;

        // Start request processing task
        self.start_request_processor().await?;

        // Start stream event processing task
        self.start_stream_event_processor().await?;

        // Start the network server
        info!("Network Bridge Server listening on {}", bind_addr);
        self.network_server.run().await?;

        Ok(())
    }

    /// Stop the bridge server
    pub async fn stop(&mut self) -> Result<()> {
        info!("Stopping Network Bridge Server");
        
        if let Some(shutdown_tx) = self.shutdown_tx.take() {
            let _ = shutdown_tx.send(()).await;
        }

        // Stop stream manager (we can't easily unwrap the Arc, so we'll just let it drop)
        // In a real implementation, we would need a proper shutdown mechanism

        self.network_server.shutdown().await?;
        
        // Clean up active streams
        let mut streams = self.active_streams.write().await;
        streams.clear();

        // Clean up client sessions
        let mut sessions = self.client_sessions.write().await;
        sessions.clear();

        info!("Network Bridge Server stopped");
        Ok(())
    }

    /// Start the request processor that handles incoming requests
    async fn start_request_processor(&mut self) -> Result<()> {
        let adb_executor = self.adb_executor.clone();
        let stream_manager = self.stream_manager.clone();
        let client_sessions = self.client_sessions.clone();
        let active_streams = self.active_streams.clone();
        let mut shutdown_rx = self.shutdown_rx.take()
            .ok_or_else(|| RemoteAdbError::server("Shutdown receiver not available"))?;

        tokio::spawn(async move {
            let (_request_tx, mut request_rx): (RequestChannel, ResponseChannel) = mpsc::channel(100);
            
            loop {
                tokio::select! {
                    request = request_rx.recv() => {
                        if let Some((request, response_tx)) = request {
                            let response = Self::handle_bridge_request(
                                request,
                                &adb_executor,
                                &stream_manager,
                                &client_sessions,
                                &active_streams,
                            ).await;
                            
                            if let Err(e) = response_tx.send(response).await {
                                error!("Failed to send response: {}", e);
                            }
                        }
                    }
                    _ = shutdown_rx.recv() => {
                        info!("Request processor shutting down");
                        break;
                    }
                }
            }
        });

        Ok(())
    }

    /// Start the stream event processor
    async fn start_stream_event_processor(&self) -> Result<()> {
        let (event_tx, mut event_rx) = mpsc::channel(1000);
        
        // Set the event sender in the stream manager
        self.stream_manager.set_event_sender(event_tx).await;

        let active_streams = self.active_streams.clone();
        let _client_sessions = self.client_sessions.clone();

        tokio::spawn(async move {
            while let Some(event) = event_rx.recv().await {
                match event {
                    StreamEvent::StreamStarted { stream_id, device_id } => {
                        info!("Stream {} started for device {}", stream_id, device_id);
                        // TODO: Notify connected clients about new stream
                    }
                    StreamEvent::StreamStopped { stream_id, reason } => {
                        info!("Stream {} stopped: {}", stream_id, reason);
                        // Remove from active streams
                        let mut streams = active_streams.write().await;
                        streams.remove(&stream_id);
                    }
                    StreamEvent::StreamError { stream_id, error } => {
                        error!("Stream {} error: {}", stream_id, error);
                        // TODO: Notify clients about stream error
                    }
                    StreamEvent::LogEntry { stream_id, entry } => {
                        debug!("Log entry from stream {}: {}", stream_id, entry.message);
                        // TODO: Forward log entry to subscribed clients
                    }
                    StreamEvent::StatusUpdate { stream_id, status } => {
                        debug!("Status update for stream {}: active={}", stream_id, status.is_active);
                        // TODO: Update client connections with status
                    }
                }
            }
            info!("Stream event processor shutting down");
        });

        Ok(())
    }

    /// Handle a bridge request and return the appropriate response
    async fn handle_bridge_request(
        request: BridgeRequest,
        adb_executor: &AdbExecutor,
        stream_manager: &LogStreamManager,
        _client_sessions: &Arc<RwLock<HashMap<String, ClientSession>>>,
        active_streams: &Arc<RwLock<HashMap<String, StreamHandle>>>,
    ) -> BridgeResponse {
        debug!("Handling bridge request: {:?}", request);

        match request {
            BridgeRequest::ListDevices => {
                match adb_executor.get_devices().await {
                    Ok(devices) => BridgeResponse::DeviceList { devices },
                    Err(e) => BridgeResponse::Error {
                        message: format!("Failed to list devices: {}", e),
                    },
                }
            }

            BridgeRequest::GetDeviceInfo { device_id } => {
                // Get device info from the device list since AdbExecutor doesn't have get_device_info
                match adb_executor.get_devices().await {
                    Ok(devices) => {
                        if let Some(device) = devices.iter().find(|d| d.device_id == device_id) {
                            BridgeResponse::DeviceInfo { device: device.clone() }
                        } else {
                            BridgeResponse::Error {
                                message: format!("Device {} not found", device_id),
                            }
                        }
                    }
                    Err(e) => BridgeResponse::Error {
                        message: format!("Failed to get device info: {}", e),
                    },
                }
            }

            BridgeRequest::StartLogStream { device_id, filters } => {
                match stream_manager.start_log_stream(device_id.clone(), filters).await {
                    Ok(handle) => {
                        // Store the stream handle in active streams
                        {
                            let mut streams = active_streams.write().await;
                            streams.insert(handle.stream_id.clone(), handle.clone());
                        }
                        
                        info!("Started log stream {} for device {}", handle.stream_id, device_id);
                        BridgeResponse::StreamStarted { handle }
                    }
                    Err(e) => BridgeResponse::Error {
                        message: format!("Failed to start log stream: {}", e),
                    },
                }
            }

            BridgeRequest::StopLogStream { stream_id } => {
                match stream_manager.stop_log_stream(&stream_id).await {
                    Ok(()) => {
                        // Remove from active streams
                        {
                            let mut streams = active_streams.write().await;
                            streams.remove(&stream_id);
                        }
                        
                        info!("Stopped log stream {}", stream_id);
                        BridgeResponse::StreamStopped { stream_id }
                    }
                    Err(e) => BridgeResponse::Error {
                        message: format!("Failed to stop log stream: {}", e),
                    },
                }
            }

            BridgeRequest::GetStreamStatus { stream_id } => {
                match stream_manager.get_stream_status(&stream_id).await {
                    Ok(status) => BridgeResponse::StreamStatus { status },
                    Err(e) => BridgeResponse::Error {
                        message: format!("Failed to get stream status: {}", e),
                    },
                }
            }

            BridgeRequest::ExecuteCommand { device_id, command } => {
                // Convert Vec<String> to Vec<&str> and add device selection
                let mut args = vec!["-s", &device_id];
                let command_strs: Vec<&str> = command.iter().map(|s| s.as_str()).collect();
                args.extend(command_strs);
                
                match adb_executor.execute_command(&args).await {
                    Ok(output) => BridgeResponse::CommandResult {
                        output,
                        exit_code: 0, // AdbExecutor returns String on success, so exit code is 0
                    },
                    Err(e) => BridgeResponse::Error {
                        message: format!("Command execution failed: {}", e),
                    },
                }
            }
        }
    }

    /// Get all active client connections
    #[allow(dead_code)]
    pub async fn get_active_connections(&self) -> Vec<ClientConnection> {
        let network_connections = self.network_server.get_active_connections().await;
        let sessions = self.client_sessions.read().await;

        network_connections
            .into_iter()
            .map(|conn| {
                let active_streams = sessions
                    .get(&conn.connection_id)
                    .map(|session| session.active_streams.clone())
                    .unwrap_or_default();

                ClientConnection {
                    client_ip: conn.client_ip,
                    connection_id: conn.connection_id,
                    connected_at: conn.connected_at,
                    is_authenticated: conn.is_authenticated,
                    active_streams,
                }
            })
            .collect()
    }

    /// Add an IP to the whitelist
    #[allow(dead_code)]
    pub async fn add_client_whitelist(&self, client_ip: IpAddr) {
        let ip_whitelist = self.network_server.get_ip_whitelist();
        ip_whitelist.add_ip(client_ip).await;
    }

    /// Remove an IP from the whitelist
    #[allow(dead_code)]
    pub async fn remove_client_whitelist(&self, client_ip: IpAddr) {
        let ip_whitelist = self.network_server.get_ip_whitelist();
        ip_whitelist.remove_ip(client_ip).await;
    }

    /// Get current server statistics
    #[allow(dead_code)]
    pub async fn get_server_stats(&self) -> ServerStats {
        let connections = self.get_active_connections().await;
        let streams = self.active_streams.read().await;
        
        ServerStats {
            active_connections: connections.len(),
            authenticated_connections: connections.iter().filter(|c| c.is_authenticated).count(),
            active_streams: streams.len(),
            uptime: SystemTime::now(), // TODO: Track actual uptime
        }
    }
}

/// Server statistics
#[derive(Debug, Clone, Serialize, Deserialize)]
#[allow(dead_code)]
pub struct ServerStats {
    pub active_connections: usize,
    pub authenticated_connections: usize,
    pub active_streams: usize,
    pub uptime: SystemTime,
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::Ipv4Addr;

    #[test]
    fn test_bridge_config_default() {
        let config = BridgeConfig::default();
        assert_eq!(config.bind_address, "0.0.0.0");
        assert_eq!(config.port, 5555);
        assert_eq!(config.max_connections, 10);
        assert!(!config.enable_auth);
        assert!(config.auth_token.is_none());
        assert!(config.ip_whitelist.is_empty());
    }

    #[test]
    fn test_bridge_request_serialization() {
        let request = BridgeRequest::ListDevices;
        let json = serde_json::to_string(&request).unwrap();
        let deserialized: BridgeRequest = serde_json::from_str(&json).unwrap();
        
        match deserialized {
            BridgeRequest::ListDevices => (),
            _ => panic!("Serialization failed"),
        }

        let request = BridgeRequest::StartLogStream {
            device_id: "emulator-5554".to_string(),
            filters: LogFilters::default(),
        };
        let json = serde_json::to_string(&request).unwrap();
        let deserialized: BridgeRequest = serde_json::from_str(&json).unwrap();
        
        match deserialized {
            BridgeRequest::StartLogStream { device_id, .. } => {
                assert_eq!(device_id, "emulator-5554");
            }
            _ => panic!("Serialization failed"),
        }
    }

    #[test]
    fn test_bridge_response_serialization() {
        let response = BridgeResponse::Success {
            message: "Operation completed".to_string(),
        };
        let json = serde_json::to_string(&response).unwrap();
        let deserialized: BridgeResponse = serde_json::from_str(&json).unwrap();
        
        match deserialized {
            BridgeResponse::Success { message } => {
                assert_eq!(message, "Operation completed");
            }
            _ => panic!("Serialization failed"),
        }
    }

    #[tokio::test]
    async fn test_network_bridge_creation() {
        let config = BridgeConfig {
            bind_address: "127.0.0.1".to_string(),
            port: 8080,
            max_connections: 5,
            enable_auth: true,
            auth_token: Some("test-token".to_string()),
            ip_whitelist: vec![IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1))],
        };

        let bridge = NetworkBridge::new(config.clone());
        assert_eq!(bridge.config.bind_address, "127.0.0.1");
        assert_eq!(bridge.config.port, 8080);
        assert_eq!(bridge.config.max_connections, 5);
        assert!(bridge.config.enable_auth);
        assert_eq!(bridge.config.auth_token, Some("test-token".to_string()));
    }

    #[tokio::test]
    async fn test_server_stats() {
        let config = BridgeConfig::default();
        let bridge = NetworkBridge::new(config);
        
        let stats = bridge.get_server_stats().await;
        assert_eq!(stats.active_connections, 0);
        assert_eq!(stats.authenticated_connections, 0);
        assert_eq!(stats.active_streams, 0);
    }
}