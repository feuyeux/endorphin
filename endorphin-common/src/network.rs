//! Network communication layer for the remote ADB system
//!
//! This module provides async TCP connection management, connection pooling,
//! heartbeat mechanisms, authentication, and security features.

use crate::{Result, RemoteAdbError, ConnectionConfig, ConnectionStatus, ClientConnection, types};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::net::{IpAddr, SocketAddr};
use std::sync::Arc;
use std::time::{Duration, SystemTime};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::{mpsc, Mutex, RwLock};
use tokio::time::{interval, timeout};
use tracing::{debug, error, info, warn};

/// Maximum message size (16MB)
const MAX_MESSAGE_SIZE: usize = 16 * 1024 * 1024;

/// Heartbeat interval in seconds
const HEARTBEAT_INTERVAL: u64 = 30;

/// Connection timeout in seconds
const CONNECTION_TIMEOUT: u64 = 60;

/// Network message types
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum NetworkMessage {
    /// Heartbeat ping
    Ping,
    /// Heartbeat pong
    Pong,
    /// Authentication request
    AuthRequest { token: String },
    /// Authentication response
    AuthResponse { success: bool, message: String },
    /// Data message
    Data { payload: Vec<u8> },
    /// Error message
    Error { message: String },
    /// Connection close
    Close,
}

/// Authentication token
#[derive(Debug, Clone)]
pub struct AuthToken {
    pub token: String,
    pub expires_at: Option<SystemTime>,
}

impl AuthToken {
    pub fn new(token: String) -> Self {
        Self {
            token,
            expires_at: None,
        }
    }

    pub fn with_expiry(token: String, expires_at: SystemTime) -> Self {
        Self {
            token,
            expires_at: Some(expires_at),
        }
    }

    pub fn is_valid(&self) -> bool {
        if let Some(expires_at) = self.expires_at {
            SystemTime::now() < expires_at
        } else {
            true
        }
    }
}

/// IP whitelist for access control
#[derive(Debug, Clone)]
pub struct IpWhitelist {
    allowed_ips: Arc<RwLock<Vec<IpAddr>>>,
}

impl IpWhitelist {
    pub fn new() -> Self {
        Self {
            allowed_ips: Arc::new(RwLock::new(Vec::new())),
        }
    }

    pub async fn add_ip(&self, ip: IpAddr) {
        let mut ips = self.allowed_ips.write().await;
        if !ips.contains(&ip) {
            ips.push(ip);
            info!("Added IP {} to whitelist", ip);
        }
    }

    pub async fn remove_ip(&self, ip: IpAddr) {
        let mut ips = self.allowed_ips.write().await;
        if let Some(pos) = ips.iter().position(|&x| x == ip) {
            ips.remove(pos);
            info!("Removed IP {} from whitelist", ip);
        }
    }

    pub async fn is_allowed(&self, ip: IpAddr) -> bool {
        let ips = self.allowed_ips.read().await;
        ips.is_empty() || ips.contains(&ip)
    }

    pub async fn clear(&self) {
        let mut ips = self.allowed_ips.write().await;
        ips.clear();
        info!("Cleared IP whitelist");
    }

    pub async fn get_allowed_ips(&self) -> Vec<IpAddr> {
        self.allowed_ips.read().await.clone()
    }
}

impl Default for IpWhitelist {
    fn default() -> Self {
        Self::new()
    }
}

/// Connection pool for managing multiple TCP connections
#[derive(Debug)]
pub struct ConnectionPool {
    connections: Arc<Mutex<HashMap<String, Arc<NetworkConnection>>>>,
    max_connections: usize,
}

impl ConnectionPool {
    pub fn new(max_connections: usize) -> Self {
        Self {
            connections: Arc::new(Mutex::new(HashMap::new())),
            max_connections,
        }
    }

    pub async fn add_connection(&self, id: String, connection: Arc<NetworkConnection>) -> Result<()> {
        let mut connections = self.connections.lock().await;
        
        if connections.len() >= self.max_connections {
            return Err(RemoteAdbError::server("Connection pool is full"));
        }

        connections.insert(id.clone(), connection);
        info!("Added connection {} to pool", id);
        Ok(())
    }

    pub async fn remove_connection(&self, id: &str) -> Option<Arc<NetworkConnection>> {
        let mut connections = self.connections.lock().await;
        let connection = connections.remove(id);
        if connection.is_some() {
            info!("Removed connection {} from pool", id);
        }
        connection
    }

    pub async fn get_connection(&self, id: &str) -> Option<Arc<NetworkConnection>> {
        let connections = self.connections.lock().await;
        connections.get(id).cloned()
    }

    pub async fn get_all_connections(&self) -> Vec<Arc<NetworkConnection>> {
        let connections = self.connections.lock().await;
        connections.values().cloned().collect()
    }

    pub async fn connection_count(&self) -> usize {
        let connections = self.connections.lock().await;
        connections.len()
    }

    pub async fn cleanup_inactive_connections(&self) {
        let mut connections = self.connections.lock().await;
        let mut to_remove = Vec::new();

        for (id, connection) in connections.iter() {
            if !connection.is_active().await {
                to_remove.push(id.clone());
            }
        }

        for id in to_remove {
            connections.remove(&id);
            info!("Cleaned up inactive connection {}", id);
        }
    }
}

/// Network connection wrapper
#[derive(Debug)]
pub struct NetworkConnection {
    id: String,
    stream: Arc<Mutex<TcpStream>>,
    peer_addr: SocketAddr,
    status: Arc<Mutex<ConnectionStatus>>,
    is_authenticated: Arc<Mutex<bool>>,
    last_activity: Arc<Mutex<SystemTime>>,
    #[allow(dead_code)]
    shutdown_tx: Option<mpsc::Sender<()>>,
}

impl NetworkConnection {
    pub fn new(id: String, stream: TcpStream, peer_addr: SocketAddr) -> Self {
        let status = Arc::new(Mutex::new(types::ConnectionStatus {
            is_connected: true,
            connection_time: Some(SystemTime::now()),
            ..Default::default()
        }));

        Self {
            id,
            stream: Arc::new(Mutex::new(stream)),
            peer_addr,
            status,
            is_authenticated: Arc::new(Mutex::new(false)),
            last_activity: Arc::new(Mutex::new(SystemTime::now())),
            shutdown_tx: None,
        }
    }

    pub fn id(&self) -> &str {
        &self.id
    }

    pub fn peer_addr(&self) -> SocketAddr {
        self.peer_addr
    }

    pub async fn is_active(&self) -> bool {
        let status = self.status.lock().await;
        status.is_connected
    }

    pub async fn is_authenticated(&self) -> bool {
        *self.is_authenticated.lock().await
    }

    pub async fn set_authenticated(&self, authenticated: bool) {
        *self.is_authenticated.lock().await = authenticated;
        info!("Connection {} authentication status: {}", self.id, authenticated);
    }

    pub async fn get_status(&self) -> ConnectionStatus {
        self.status.lock().await.clone()
    }

    pub async fn update_activity(&self) {
        *self.last_activity.lock().await = SystemTime::now();
        let mut status = self.status.lock().await;
        status.last_heartbeat = Some(SystemTime::now());
    }

    pub async fn send_message(&self, message: &NetworkMessage) -> Result<()> {
        let data = serde_json::to_vec(message)
            .map_err(|e| RemoteAdbError::network(format!("Failed to serialize message: {}", e)))?;

        if data.len() > MAX_MESSAGE_SIZE {
            return Err(RemoteAdbError::network("Message too large"));
        }

        let mut stream = self.stream.lock().await;
        
        // Send message length first (4 bytes, big endian)
        let len_bytes = (data.len() as u32).to_be_bytes();
        stream.write_all(&len_bytes).await
            .map_err(|e| RemoteAdbError::network(format!("Failed to send message length: {}", e)))?;

        // Send message data
        stream.write_all(&data).await
            .map_err(|e| RemoteAdbError::network(format!("Failed to send message data: {}", e)))?;

        stream.flush().await
            .map_err(|e| RemoteAdbError::network(format!("Failed to flush stream: {}", e)))?;

        self.update_activity().await;
        debug!("Sent message to connection {}: {:?}", self.id, message);
        Ok(())
    }

    pub async fn receive_message(&self) -> Result<NetworkMessage> {
        let mut stream = self.stream.lock().await;

        // Read message length (4 bytes, big endian)
        let mut len_bytes = [0u8; 4];
        stream.read_exact(&mut len_bytes).await
            .map_err(|e| RemoteAdbError::network(format!("Failed to read message length: {}", e)))?;

        let message_len = u32::from_be_bytes(len_bytes) as usize;
        
        if message_len > MAX_MESSAGE_SIZE {
            return Err(RemoteAdbError::network("Message too large"));
        }

        // Read message data
        let mut data = vec![0u8; message_len];
        stream.read_exact(&mut data).await
            .map_err(|e| RemoteAdbError::network(format!("Failed to read message data: {}", e)))?;

        let message: NetworkMessage = serde_json::from_slice(&data)
            .map_err(|e| RemoteAdbError::network(format!("Failed to deserialize message: {}", e)))?;

        self.update_activity().await;
        debug!("Received message from connection {}: {:?}", self.id, message);
        Ok(message)
    }

    pub async fn close(&self) -> Result<()> {
        let mut status = self.status.lock().await;
        status.is_connected = false;
        
        // Send close message if possible
        if let Ok(mut stream) = self.stream.try_lock() {
            let _ = stream.shutdown().await;
        }

        info!("Closed connection {}", self.id);
        Ok(())
    }
}

/// Network server for handling incoming connections
#[derive(Debug)]
pub struct NetworkServer {
    listener: Option<TcpListener>,
    connection_pool: Arc<ConnectionPool>,
    ip_whitelist: Arc<IpWhitelist>,
    auth_tokens: Arc<RwLock<HashMap<String, AuthToken>>>,
    shutdown_tx: Option<mpsc::Sender<()>>,
    shutdown_rx: Option<mpsc::Receiver<()>>,
}

impl NetworkServer {
    pub fn new(max_connections: usize) -> Self {
        let (shutdown_tx, shutdown_rx) = mpsc::channel(1);
        
        Self {
            listener: None,
            connection_pool: Arc::new(ConnectionPool::new(max_connections)),
            ip_whitelist: Arc::new(IpWhitelist::new()),
            auth_tokens: Arc::new(RwLock::new(HashMap::new())),
            shutdown_tx: Some(shutdown_tx),
            shutdown_rx: Some(shutdown_rx),
        }
    }

    pub async fn bind(&mut self, addr: SocketAddr) -> Result<()> {
        let listener = TcpListener::bind(addr).await
            .map_err(|e| RemoteAdbError::network(format!("Failed to bind to {}: {}", addr, e)))?;

        info!("Network server bound to {}", addr);
        self.listener = Some(listener);
        Ok(())
    }

    pub fn get_ip_whitelist(&self) -> Arc<IpWhitelist> {
        self.ip_whitelist.clone()
    }

    pub async fn add_auth_token(&self, token: String, auth_token: AuthToken) {
        let mut tokens = self.auth_tokens.write().await;
        tokens.insert(token, auth_token);
    }

    pub async fn remove_auth_token(&self, token: &str) {
        let mut tokens = self.auth_tokens.write().await;
        tokens.remove(token);
    }

    pub async fn validate_auth_token(&self, token: &str) -> bool {
        let tokens = self.auth_tokens.read().await;
        if let Some(auth_token) = tokens.get(token) {
            auth_token.is_valid()
        } else {
            false
        }
    }

    pub async fn run(&mut self) -> Result<()> {
        let listener = self.listener.take()
            .ok_or_else(|| RemoteAdbError::server("Server not bound to address"))?;

        let mut shutdown_rx = self.shutdown_rx.take()
            .ok_or_else(|| RemoteAdbError::server("Shutdown receiver not available"))?;

        let connection_pool = self.connection_pool.clone();
        let ip_whitelist = self.ip_whitelist.clone();
        let auth_tokens = self.auth_tokens.clone();

        // Start heartbeat task
        let heartbeat_pool = connection_pool.clone();
        tokio::spawn(async move {
            let mut interval = interval(Duration::from_secs(HEARTBEAT_INTERVAL));
            loop {
                interval.tick().await;
                Self::send_heartbeats(&heartbeat_pool).await;
                heartbeat_pool.cleanup_inactive_connections().await;
            }
        });

        info!("Network server started");

        loop {
            tokio::select! {
                result = listener.accept() => {
                    match result {
                        Ok((stream, peer_addr)) => {
                            let ip = peer_addr.ip();
                            
                            // Check IP whitelist
                            if !ip_whitelist.is_allowed(ip).await {
                                warn!("Connection from {} rejected: not in whitelist", ip);
                                continue;
                            }

                            let connection_id = format!("{}:{}", peer_addr.ip(), peer_addr.port());
                            let connection = Arc::new(NetworkConnection::new(
                                connection_id.clone(),
                                stream,
                                peer_addr
                            ));

                            if let Err(e) = connection_pool.add_connection(connection_id.clone(), connection.clone()).await {
                                error!("Failed to add connection {}: {}", connection_id, e);
                                continue;
                            }

                            // Handle connection in separate task
                            let pool = connection_pool.clone();
                            let tokens = auth_tokens.clone();
                            tokio::spawn(async move {
                                if let Err(e) = Self::handle_connection(connection.clone(), tokens).await {
                                    error!("Connection {} error: {}", connection_id, e);
                                }
                                pool.remove_connection(&connection_id).await;
                            });
                        }
                        Err(e) => {
                            error!("Failed to accept connection: {}", e);
                        }
                    }
                }
                _ = shutdown_rx.recv() => {
                    info!("Network server shutting down");
                    break;
                }
            }
        }

        Ok(())
    }

    async fn handle_connection(
        connection: Arc<NetworkConnection>,
        auth_tokens: Arc<RwLock<HashMap<String, AuthToken>>>,
    ) -> Result<()> {
        info!("Handling new connection: {}", connection.id());

        // Wait for authentication
        let auth_timeout = Duration::from_secs(30);
        let auth_result = timeout(auth_timeout, async {
            loop {
                match connection.receive_message().await? {
                    NetworkMessage::AuthRequest { token } => {
                        let is_valid = {
                            let tokens = auth_tokens.read().await;
                            tokens.get(&token).is_some_and(|t| t.is_valid())
                        };

                        if is_valid {
                            connection.set_authenticated(true).await;
                            connection.send_message(&NetworkMessage::AuthResponse {
                                success: true,
                                message: "Authentication successful".to_string(),
                            }).await?;
                            break;
                        } else {
                            connection.send_message(&NetworkMessage::AuthResponse {
                                success: false,
                                message: "Invalid token".to_string(),
                            }).await?;
                            return Err(RemoteAdbError::auth("Invalid authentication token"));
                        }
                    }
                    NetworkMessage::Close => {
                        return Err(RemoteAdbError::client("Connection closed during authentication"));
                    }
                    _ => {
                        connection.send_message(&NetworkMessage::Error {
                            message: "Authentication required".to_string(),
                        }).await?;
                    }
                }
            }
            Ok::<(), RemoteAdbError>(())
        }).await;

        match auth_result {
            Ok(Ok(())) => {
                info!("Connection {} authenticated successfully", connection.id());
            }
            Ok(Err(e)) => {
                error!("Authentication failed for connection {}: {}", connection.id(), e);
                return Err(e);
            }
            Err(_) => {
                error!("Authentication timeout for connection {}", connection.id());
                return Err(RemoteAdbError::Timeout);
            }
        }

        // Handle authenticated connection
        loop {
            let receive_timeout = Duration::from_secs(CONNECTION_TIMEOUT);
            match timeout(receive_timeout, connection.receive_message()).await {
                Ok(Ok(message)) => {
                    match message {
                        NetworkMessage::Ping => {
                            connection.send_message(&NetworkMessage::Pong).await?;
                        }
                        NetworkMessage::Pong => {
                            // Heartbeat response received
                        }
                        NetworkMessage::Close => {
                            info!("Connection {} closed by client", connection.id());
                            break;
                        }
                        NetworkMessage::Data { payload: _ } => {
                            // Handle data messages (to be implemented in higher layers)
                            debug!("Received data message from connection {}", connection.id());
                        }
                        _ => {
                            warn!("Unexpected message from connection {}: {:?}", connection.id(), message);
                        }
                    }
                }
                Ok(Err(e)) => {
                    error!("Error receiving message from connection {}: {}", connection.id(), e);
                    break;
                }
                Err(_) => {
                    warn!("Connection {} timed out", connection.id());
                    break;
                }
            }
        }

        connection.close().await?;
        Ok(())
    }

    async fn send_heartbeats(connection_pool: &ConnectionPool) {
        let connections = connection_pool.get_all_connections().await;
        for connection in connections {
            if connection.is_authenticated().await {
                if let Err(e) = connection.send_message(&NetworkMessage::Ping).await {
                    error!("Failed to send heartbeat to connection {}: {}", connection.id(), e);
                }
            }
        }
    }

    pub async fn shutdown(&mut self) -> Result<()> {
        if let Some(shutdown_tx) = self.shutdown_tx.take() {
            let _ = shutdown_tx.send(()).await;
        }
        info!("Network server shutdown initiated");
        Ok(())
    }

    pub async fn get_active_connections(&self) -> Vec<ClientConnection> {
        let connections = self.connection_pool.get_all_connections().await;
        let mut client_connections = Vec::new();

        for connection in connections {
            if connection.is_active().await {
                let status = connection.get_status().await;
                let client_connection = ClientConnection {
                    client_ip: connection.peer_addr().ip().to_string(),
                    connection_id: connection.id().to_string(),
                    connected_at: status.connection_time.unwrap_or_else(SystemTime::now),
                    is_authenticated: connection.is_authenticated().await,
                    active_streams: Vec::new(), // To be populated by higher layers
                };
                client_connections.push(client_connection);
            }
        }

        client_connections
    }
}

/// Network client for establishing connections to remote servers
#[derive(Debug)]
pub struct NetworkClient {
    config: ConnectionConfig,
    connection: Option<Arc<NetworkConnection>>,
    #[allow(dead_code)]
    shutdown_tx: Option<mpsc::Sender<()>>,
    #[allow(dead_code)]
    shutdown_rx: Option<mpsc::Receiver<()>>,
}

impl NetworkClient {
    pub fn new(config: ConnectionConfig) -> Self {
        let (shutdown_tx, shutdown_rx) = mpsc::channel(1);
        
        Self {
            config,
            connection: None,
            shutdown_tx: Some(shutdown_tx),
            shutdown_rx: Some(shutdown_rx),
        }
    }

    pub async fn connect(&mut self) -> Result<()> {
        let addr = format!("{}:{}", self.config.host, self.config.port);
        let socket_addr: SocketAddr = addr.parse()
            .map_err(|e| RemoteAdbError::config(format!("Invalid address {}: {}", addr, e)))?;

        let connect_timeout = Duration::from_secs(self.config.timeout_seconds);
        let stream = timeout(connect_timeout, TcpStream::connect(socket_addr)).await
            .map_err(|_| RemoteAdbError::Timeout)?
            .map_err(|e| RemoteAdbError::network(format!("Failed to connect to {}: {}", addr, e)))?;

        let peer_addr = stream.peer_addr()
            .map_err(|e| RemoteAdbError::network(format!("Failed to get peer address: {}", e)))?;

        let connection_id = format!("client-{}", peer_addr);
        let connection = Arc::new(NetworkConnection::new(connection_id, stream, peer_addr));

        // Authenticate
        connection.send_message(&NetworkMessage::AuthRequest {
            token: self.config.auth_token.clone(),
        }).await?;

        let auth_timeout = Duration::from_secs(30);
        match timeout(auth_timeout, connection.receive_message()).await {
            Ok(Ok(NetworkMessage::AuthResponse { success: true, .. })) => {
                connection.set_authenticated(true).await;
                info!("Successfully authenticated with server");
            }
            Ok(Ok(NetworkMessage::AuthResponse { success: false, message })) => {
                return Err(RemoteAdbError::auth(format!("Authentication failed: {}", message)));
            }
            Ok(Ok(_)) => {
                return Err(RemoteAdbError::auth("Unexpected response during authentication"));
            }
            Ok(Err(e)) => {
                return Err(e);
            }
            Err(_) => {
                return Err(RemoteAdbError::Timeout);
            }
        }

        self.connection = Some(connection);
        info!("Connected to server at {}", addr);
        Ok(())
    }

    pub async fn disconnect(&mut self) -> Result<()> {
        if let Some(connection) = self.connection.take() {
            connection.send_message(&NetworkMessage::Close).await?;
            connection.close().await?;
            info!("Disconnected from server");
        }
        Ok(())
    }

    pub async fn is_connected(&self) -> bool {
        if let Some(connection) = &self.connection {
            connection.is_active().await && connection.is_authenticated().await
        } else {
            false
        }
    }

    pub async fn send_message(&self, message: &NetworkMessage) -> Result<()> {
        if let Some(connection) = &self.connection {
            if connection.is_authenticated().await {
                connection.send_message(message).await
            } else {
                Err(RemoteAdbError::auth("Connection not authenticated"))
            }
        } else {
            Err(RemoteAdbError::client("Not connected to server"))
        }
    }

    pub async fn receive_message(&self) -> Result<NetworkMessage> {
        if let Some(connection) = &self.connection {
            if connection.is_authenticated().await {
                connection.receive_message().await
            } else {
                Err(RemoteAdbError::auth("Connection not authenticated"))
            }
        } else {
            Err(RemoteAdbError::client("Not connected to server"))
        }
    }

    pub async fn get_connection_status(&self) -> ConnectionStatus {
        if let Some(connection) = &self.connection {
            connection.get_status().await
        } else {
            ConnectionStatus::default()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::Ipv4Addr;

    #[test]
    fn test_auth_token_validity() {
        let token = AuthToken::new("test-token".to_string());
        assert!(token.is_valid());

        let expired_token = AuthToken::with_expiry(
            "expired-token".to_string(),
            SystemTime::now() - Duration::from_secs(3600)
        );
        assert!(!expired_token.is_valid());

        let future_token = AuthToken::with_expiry(
            "future-token".to_string(),
            SystemTime::now() + Duration::from_secs(3600)
        );
        assert!(future_token.is_valid());
    }

    #[tokio::test]
    async fn test_ip_whitelist() {
        let whitelist = IpWhitelist::new();
        let ip1 = IpAddr::V4(Ipv4Addr::new(192, 168, 1, 1));
        let ip2 = IpAddr::V4(Ipv4Addr::new(192, 168, 1, 2));

        // Empty whitelist allows all IPs
        assert!(whitelist.is_allowed(ip1).await);
        assert!(whitelist.is_allowed(ip2).await);

        // Add IP to whitelist
        whitelist.add_ip(ip1).await;
        assert!(whitelist.is_allowed(ip1).await);
        assert!(!whitelist.is_allowed(ip2).await);

        // Remove IP from whitelist
        whitelist.remove_ip(ip1).await;
        assert!(whitelist.is_allowed(ip1).await); // Empty whitelist allows all
        assert!(whitelist.is_allowed(ip2).await);
    }

    #[tokio::test]
    async fn test_connection_pool() {
        let pool = ConnectionPool::new(2);
        assert_eq!(pool.connection_count().await, 0);

        // Create mock connections (we can't easily create real TcpStreams in tests)
        // This test focuses on the pool logic rather than actual network connections
        let _addr = SocketAddr::from(([127, 0, 0, 1], 8080));
        
        // Test would require actual TcpStream, so we'll test the pool capacity logic
        assert_eq!(pool.max_connections, 2);
    }

    #[test]
    fn test_network_message_serialization() {
        let ping = NetworkMessage::Ping;
        let json = serde_json::to_string(&ping).unwrap();
        let deserialized: NetworkMessage = serde_json::from_str(&json).unwrap();
        
        match deserialized {
            NetworkMessage::Ping => (),
            _ => panic!("Deserialization failed"),
        }

        let auth_request = NetworkMessage::AuthRequest {
            token: "test-token".to_string(),
        };
        let json = serde_json::to_string(&auth_request).unwrap();
        let deserialized: NetworkMessage = serde_json::from_str(&json).unwrap();
        
        match deserialized {
            NetworkMessage::AuthRequest { token } => {
                assert_eq!(token, "test-token");
            }
            _ => panic!("Deserialization failed"),
        }
    }
}