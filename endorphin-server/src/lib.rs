//! Remote ADB Server Library
//!
//! This library provides server-side functionality for sharing local ADB
//! access with remote clients and streaming Android device logs.

pub mod bridge;
pub mod stream_manager;

pub use bridge::{NetworkBridge, BridgeConfig, BridgeRequest, BridgeResponse, ServerStats};
pub use stream_manager::{LogStreamManager, StreamEvent};