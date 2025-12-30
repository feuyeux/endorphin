//! Security layer for the remote ADB system
//!
//! This module provides TLS encryption, certificate management,
//! and enhanced authentication mechanisms.

use crate::{Result, RemoteAdbError};
use rustls::{Certificate, ClientConfig, PrivateKey, ServerConfig};
use rustls_pemfile::{certs, pkcs8_private_keys};
use std::fs::File;
use std::io::BufReader;
use std::path::Path;
use std::sync::Arc;
use tokio::io::{AsyncRead, AsyncWrite};
use tokio_rustls::{TlsAcceptor, TlsConnector};
use tracing::{debug, info};

/// TLS configuration for the server
#[derive(Debug, Clone)]
pub struct TlsServerConfig {
    pub cert_path: String,
    pub key_path: String,
    pub client_ca_path: Option<String>,
    pub require_client_cert: bool,
}

impl TlsServerConfig {
    pub fn new(cert_path: String, key_path: String) -> Self {
        Self {
            cert_path,
            key_path,
            client_ca_path: None,
            require_client_cert: false,
        }
    }

    pub fn with_client_ca(mut self, ca_path: String, require_cert: bool) -> Self {
        self.client_ca_path = Some(ca_path);
        self.require_client_cert = require_cert;
        self
    }
}

/// TLS configuration for the client
#[derive(Debug, Clone)]
pub struct TlsClientConfig {
    pub ca_path: Option<String>,
    pub cert_path: Option<String>,
    pub key_path: Option<String>,
    pub server_name: String,
    pub insecure: bool,
}

impl TlsClientConfig {
    pub fn new(server_name: String) -> Self {
        Self {
            ca_path: None,
            cert_path: None,
            key_path: None,
            server_name,
            insecure: false,
        }
    }

    pub fn with_ca(mut self, ca_path: String) -> Self {
        self.ca_path = Some(ca_path);
        self
    }

    pub fn with_client_cert(mut self, cert_path: String, key_path: String) -> Self {
        self.cert_path = Some(cert_path);
        self.key_path = Some(key_path);
        self
    }

    pub fn insecure(mut self) -> Self {
        self.insecure = true;
        self
    }
}

/// TLS server wrapper
pub struct TlsServer {
    acceptor: TlsAcceptor,
}

impl TlsServer {
    pub fn new(config: &TlsServerConfig) -> Result<Self> {
        let server_config = Self::build_server_config(config)?;
        let acceptor = TlsAcceptor::from(Arc::new(server_config));
        
        Ok(Self { acceptor })
    }

    fn build_server_config(config: &TlsServerConfig) -> Result<ServerConfig> {
        // Load server certificate
        let cert_file = File::open(&config.cert_path)
            .map_err(|e| RemoteAdbError::config(format!("Failed to open cert file {}: {}", config.cert_path, e)))?;
        let mut cert_reader = BufReader::new(cert_file);
        let cert_chain = certs(&mut cert_reader)
            .map_err(|e| RemoteAdbError::config(format!("Failed to parse certificates: {}", e)))?
            .into_iter()
            .map(Certificate)
            .collect();

        // Load server private key
        let key_file = File::open(&config.key_path)
            .map_err(|e| RemoteAdbError::config(format!("Failed to open key file {}: {}", config.key_path, e)))?;
        let mut key_reader = BufReader::new(key_file);
        let mut keys = pkcs8_private_keys(&mut key_reader)
            .map_err(|e| RemoteAdbError::config(format!("Failed to parse private key: {}", e)))?;

        if keys.is_empty() {
            return Err(RemoteAdbError::config("No private keys found"));
        }

        let private_key = PrivateKey(keys.remove(0));

        // Build server config
        let mut server_config = ServerConfig::builder()
            .with_safe_defaults()
            .with_no_client_auth()
            .with_single_cert(cert_chain, private_key)
            .map_err(|e| RemoteAdbError::config(format!("Failed to build TLS config: {}", e)))?;

        // Enable ALPN if needed
        server_config.alpn_protocols = vec![b"endorphin".to_vec()];

        info!("TLS server configuration loaded successfully");
        Ok(server_config)
    }

    pub async fn accept<S>(&self, stream: S) -> Result<tokio_rustls::server::TlsStream<S>>
    where
        S: AsyncRead + AsyncWrite + Unpin,
    {
        self.acceptor
            .accept(stream)
            .await
            .map_err(|e| RemoteAdbError::network(format!("TLS handshake failed: {}", e)))
    }
}

/// TLS client wrapper
pub struct TlsClient {
    connector: TlsConnector,
    server_name: String,
}

impl TlsClient {
    pub fn new(config: &TlsClientConfig) -> Result<Self> {
        let client_config = Self::build_client_config(config)?;
        let connector = TlsConnector::from(Arc::new(client_config));
        
        Ok(Self {
            connector,
            server_name: config.server_name.clone(),
        })
    }

    fn build_client_config(config: &TlsClientConfig) -> Result<ClientConfig> {
        let mut client_config = if config.insecure {
            // Create insecure config that accepts any certificate
            ClientConfig::builder()
                .with_safe_defaults()
                .with_custom_certificate_verifier(Arc::new(InsecureVerifier))
                .with_no_client_auth()
        } else {
            // Use system root certificates or custom CA
            let mut root_store = rustls::RootCertStore::empty();
            
            if let Some(ca_path) = &config.ca_path {
                // Load custom CA certificate
                let ca_file = File::open(ca_path)
                    .map_err(|e| RemoteAdbError::config(format!("Failed to open CA file {}: {}", ca_path, e)))?;
                let mut ca_reader = BufReader::new(ca_file);
                let ca_certs = certs(&mut ca_reader)
                    .map_err(|e| RemoteAdbError::config(format!("Failed to parse CA certificates: {}", e)))?;

                for cert in ca_certs {
                    root_store.add(&Certificate(cert))
                        .map_err(|e| RemoteAdbError::config(format!("Failed to add CA certificate: {}", e)))?;
                }
            } else {
                // Use system root certificates
                root_store.add_trust_anchors(webpki_roots::TLS_SERVER_ROOTS.iter().map(|ta| {
                    rustls::OwnedTrustAnchor::from_subject_spki_name_constraints(
                        ta.subject,
                        ta.spki,
                        ta.name_constraints,
                    )
                }));
            }

            ClientConfig::builder()
                .with_safe_defaults()
                .with_root_certificates(root_store)
                .with_no_client_auth()
        };

        // Add client certificate if provided
        if let (Some(cert_path), Some(key_path)) = (&config.cert_path, &config.key_path) {
            // Load client certificate
            let cert_file = File::open(cert_path)
                .map_err(|e| RemoteAdbError::config(format!("Failed to open client cert file {}: {}", cert_path, e)))?;
            let mut cert_reader = BufReader::new(cert_file);
            let cert_chain = certs(&mut cert_reader)
                .map_err(|e| RemoteAdbError::config(format!("Failed to parse client certificates: {}", e)))?
                .into_iter()
                .map(Certificate)
                .collect();

            // Load client private key
            let key_file = File::open(key_path)
                .map_err(|e| RemoteAdbError::config(format!("Failed to open client key file {}: {}", key_path, e)))?;
            let mut key_reader = BufReader::new(key_file);
            let mut keys = pkcs8_private_keys(&mut key_reader)
                .map_err(|e| RemoteAdbError::config(format!("Failed to parse client private key: {}", e)))?;

            if keys.is_empty() {
                return Err(RemoteAdbError::config("No client private keys found"));
            }

            let private_key = PrivateKey(keys.remove(0));

            // Rebuild config with client certificate
            let mut root_store = rustls::RootCertStore::empty();
            if let Some(ca_path) = &config.ca_path {
                let ca_file = File::open(ca_path)
                    .map_err(|e| RemoteAdbError::config(format!("Failed to open CA file {}: {}", ca_path, e)))?;
                let mut ca_reader = BufReader::new(ca_file);
                let ca_certs = certs(&mut ca_reader)
                    .map_err(|e| RemoteAdbError::config(format!("Failed to parse CA certificates: {}", e)))?;

                for cert in ca_certs {
                    root_store.add(&Certificate(cert))
                        .map_err(|e| RemoteAdbError::config(format!("Failed to add CA certificate: {}", e)))?;
                }
            } else {
                root_store.add_trust_anchors(webpki_roots::TLS_SERVER_ROOTS.iter().map(|ta| {
                    rustls::OwnedTrustAnchor::from_subject_spki_name_constraints(
                        ta.subject,
                        ta.spki,
                        ta.name_constraints,
                    )
                }));
            }

            client_config = ClientConfig::builder()
                .with_safe_defaults()
                .with_root_certificates(root_store)
                .with_client_auth_cert(cert_chain, private_key)
                .map_err(|e| RemoteAdbError::config(format!("Failed to build client TLS config: {}", e)))?;
        }

        // Enable ALPN
        client_config.alpn_protocols = vec![b"endorphin".to_vec()];

        info!("TLS client configuration loaded successfully");
        Ok(client_config)
    }

    pub async fn connect<S>(&self, stream: S) -> Result<tokio_rustls::client::TlsStream<S>>
    where
        S: AsyncRead + AsyncWrite + Unpin,
    {
        let server_name = self.server_name.as_str().try_into()
            .map_err(|e| RemoteAdbError::config(format!("Invalid server name {}: {}", self.server_name, e)))?;

        self.connector
            .connect(server_name, stream)
            .await
            .map_err(|e| RemoteAdbError::network(format!("TLS connection failed: {}", e)))
    }
}

/// Insecure certificate verifier for testing/development
struct InsecureVerifier;

impl rustls::client::ServerCertVerifier for InsecureVerifier {
    fn verify_server_cert(
        &self,
        _end_entity: &Certificate,
        _intermediates: &[Certificate],
        _server_name: &rustls::ServerName,
        _scts: &mut dyn Iterator<Item = &[u8]>,
        _ocsp_response: &[u8],
        _now: std::time::SystemTime,
    ) -> std::result::Result<rustls::client::ServerCertVerified, rustls::Error> {
        debug!("Accepting any server certificate (insecure mode)");
        Ok(rustls::client::ServerCertVerified::assertion())
    }
}

/// Enhanced authentication with rate limiting and token management
pub mod auth {
    use super::*;
    use std::collections::HashMap;
    use std::time::{Duration, Instant, SystemTime};
    use tokio::sync::RwLock;
    use tracing::warn;

    /// Rate limiter for authentication attempts
    #[derive(Debug)]
    pub struct RateLimiter {
        attempts: Arc<RwLock<HashMap<String, Vec<Instant>>>>,
        max_attempts: usize,
        window_duration: Duration,
    }

    impl RateLimiter {
        pub fn new(max_attempts: usize, window_duration: Duration) -> Self {
            Self {
                attempts: Arc::new(RwLock::new(HashMap::new())),
                max_attempts,
                window_duration,
            }
        }

        pub async fn check_rate_limit(&self, identifier: &str) -> bool {
            let mut attempts = self.attempts.write().await;
            let now = Instant::now();
            
            let entry = attempts.entry(identifier.to_string()).or_insert_with(Vec::new);
            
            // Remove old attempts outside the window
            entry.retain(|&attempt_time| now.duration_since(attempt_time) < self.window_duration);
            
            if entry.len() >= self.max_attempts {
                warn!("Rate limit exceeded for identifier: {}", identifier);
                false
            } else {
                entry.push(now);
                true
            }
        }

        pub async fn reset_rate_limit(&self, identifier: &str) {
            let mut attempts = self.attempts.write().await;
            attempts.remove(identifier);
        }
    }

    /// Enhanced authentication token with metadata
    #[derive(Debug, Clone)]
    pub struct EnhancedAuthToken {
        pub token: String,
        pub created_at: SystemTime,
        pub expires_at: Option<SystemTime>,
        pub permissions: Vec<String>,
        pub metadata: HashMap<String, String>,
    }

    impl EnhancedAuthToken {
        pub fn new(token: String, permissions: Vec<String>) -> Self {
            Self {
                token,
                created_at: SystemTime::now(),
                expires_at: None,
                permissions,
                metadata: HashMap::new(),
            }
        }

        pub fn with_expiry(mut self, expires_at: SystemTime) -> Self {
            self.expires_at = Some(expires_at);
            self
        }

        pub fn with_metadata(mut self, key: String, value: String) -> Self {
            self.metadata.insert(key, value);
            self
        }

        pub fn is_valid(&self) -> bool {
            if let Some(expires_at) = self.expires_at {
                SystemTime::now() < expires_at
            } else {
                true
            }
        }

        pub fn has_permission(&self, permission: &str) -> bool {
            self.permissions.contains(&permission.to_string()) || self.permissions.contains(&"*".to_string())
        }
    }

    /// Authentication manager with enhanced features
    #[derive(Debug)]
    pub struct AuthManager {
        tokens: Arc<RwLock<HashMap<String, EnhancedAuthToken>>>,
        rate_limiter: RateLimiter,
    }

    impl AuthManager {
        pub fn new(max_attempts: usize, window_duration: Duration) -> Self {
            Self {
                tokens: Arc::new(RwLock::new(HashMap::new())),
                rate_limiter: RateLimiter::new(max_attempts, window_duration),
            }
        }

        pub async fn add_token(&self, token: EnhancedAuthToken) {
            let mut tokens = self.tokens.write().await;
            tokens.insert(token.token.clone(), token);
        }

        pub async fn remove_token(&self, token: &str) {
            let mut tokens = self.tokens.write().await;
            tokens.remove(token);
        }

        pub async fn validate_token(&self, token: &str, required_permission: Option<&str>) -> bool {
            let tokens = self.tokens.read().await;
            if let Some(auth_token) = tokens.get(token) {
                if !auth_token.is_valid() {
                    return false;
                }

                if let Some(permission) = required_permission {
                    auth_token.has_permission(permission)
                } else {
                    true
                }
            } else {
                false
            }
        }

        pub async fn authenticate(&self, identifier: &str, token: &str, required_permission: Option<&str>) -> bool {
            // Check rate limit first
            if !self.rate_limiter.check_rate_limit(identifier).await {
                return false;
            }

            let is_valid = self.validate_token(token, required_permission).await;
            
            if is_valid {
                // Reset rate limit on successful authentication
                self.rate_limiter.reset_rate_limit(identifier).await;
            }

            is_valid
        }

        pub async fn cleanup_expired_tokens(&self) {
            let mut tokens = self.tokens.write().await;
            tokens.retain(|_, token| token.is_valid());
        }
    }
}

/// Utility functions for certificate generation and management
pub mod cert_utils {
    use super::*;

    /// Check if certificate files exist
    pub fn check_cert_files(cert_path: &str, key_path: &str) -> Result<()> {
        if !Path::new(cert_path).exists() {
            return Err(RemoteAdbError::config(format!("Certificate file not found: {}", cert_path)));
        }

        if !Path::new(key_path).exists() {
            return Err(RemoteAdbError::config(format!("Private key file not found: {}", key_path)));
        }

        Ok(())
    }

    /// Validate certificate file format
    pub fn validate_cert_file(cert_path: &str) -> Result<()> {
        let cert_file = File::open(cert_path)
            .map_err(|e| RemoteAdbError::config(format!("Failed to open cert file {}: {}", cert_path, e)))?;
        let mut cert_reader = BufReader::new(cert_file);
        
        certs(&mut cert_reader)
            .map_err(|e| RemoteAdbError::config(format!("Invalid certificate format in {}: {}", cert_path, e)))?;

        Ok(())
    }

    /// Validate private key file format
    pub fn validate_key_file(key_path: &str) -> Result<()> {
        let key_file = File::open(key_path)
            .map_err(|e| RemoteAdbError::config(format!("Failed to open key file {}: {}", key_path, e)))?;
        let mut key_reader = BufReader::new(key_file);
        
        let keys = pkcs8_private_keys(&mut key_reader)
            .map_err(|e| RemoteAdbError::config(format!("Invalid private key format in {}: {}", key_path, e)))?;

        if keys.is_empty() {
            return Err(RemoteAdbError::config(format!("No private keys found in {}", key_path)));
        }

        Ok(())
    }

    /// Create a simple self-signed certificate for testing (requires openssl command)
    pub fn create_self_signed_cert(cert_path: &str, key_path: &str, common_name: &str) -> Result<()> {
        use std::process::Command;

        let output = Command::new("openssl")
            .args([
                "req", "-x509", "-newkey", "rsa:4096", "-keyout", key_path,
                "-out", cert_path, "-days", "365", "-nodes",
                "-subj", &format!("/CN={}", common_name)
            ])
            .output();

        match output {
            Ok(output) => {
                if output.status.success() {
                    info!("Self-signed certificate created: {} and {}", cert_path, key_path);
                    Ok(())
                } else {
                    let error = String::from_utf8_lossy(&output.stderr);
                    Err(RemoteAdbError::config(format!("Failed to create certificate: {}", error)))
                }
            }
            Err(e) => {
                Err(RemoteAdbError::config(format!("OpenSSL command failed: {}. Make sure openssl is installed.", e)))
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use super::auth::*;
    use std::time::Duration;

    #[tokio::test]
    async fn test_rate_limiter() {
        let rate_limiter = RateLimiter::new(3, Duration::from_secs(60));
        let identifier = "test_user";

        // First 3 attempts should succeed
        assert!(rate_limiter.check_rate_limit(identifier).await);
        assert!(rate_limiter.check_rate_limit(identifier).await);
        assert!(rate_limiter.check_rate_limit(identifier).await);

        // 4th attempt should fail
        assert!(!rate_limiter.check_rate_limit(identifier).await);

        // Reset and try again
        rate_limiter.reset_rate_limit(identifier).await;
        assert!(rate_limiter.check_rate_limit(identifier).await);
    }

    #[tokio::test]
    async fn test_enhanced_auth_token() {
        let token = EnhancedAuthToken::new(
            "test-token".to_string(),
            vec!["read".to_string(), "write".to_string()]
        );

        assert!(token.is_valid());
        assert!(token.has_permission("read"));
        assert!(token.has_permission("write"));
        assert!(!token.has_permission("admin"));

        let admin_token = EnhancedAuthToken::new(
            "admin-token".to_string(),
            vec!["*".to_string()]
        );

        assert!(admin_token.has_permission("read"));
        assert!(admin_token.has_permission("write"));
        assert!(admin_token.has_permission("admin"));
        assert!(admin_token.has_permission("anything"));
    }

    #[tokio::test]
    async fn test_auth_manager() {
        let auth_manager = AuthManager::new(3, Duration::from_secs(60));
        
        let token = EnhancedAuthToken::new(
            "valid-token".to_string(),
            vec!["read".to_string()]
        );
        auth_manager.add_token(token).await;

        // Valid authentication
        assert!(auth_manager.authenticate("user1", "valid-token", Some("read")).await);
        
        // Invalid token
        assert!(!auth_manager.authenticate("user2", "invalid-token", None).await);
        
        // Valid token but insufficient permissions
        assert!(!auth_manager.authenticate("user3", "valid-token", Some("admin")).await);
    }

    #[test]
    fn test_tls_config_creation() {
        let server_config = TlsServerConfig::new(
            "cert.pem".to_string(),
            "key.pem".to_string()
        );
        assert_eq!(server_config.cert_path, "cert.pem");
        assert_eq!(server_config.key_path, "key.pem");
        assert!(!server_config.require_client_cert);

        let client_config = TlsClientConfig::new("example.com".to_string())
            .insecure();
        assert_eq!(client_config.server_name, "example.com");
        assert!(client_config.insecure);
    }
}