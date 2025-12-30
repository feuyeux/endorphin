//! Remote ADB Client Library
//!
//! This library provides client-side functionality for connecting to
//! remote ADB servers and monitoring Android device logs.

pub mod connection;
pub mod ui;

pub use connection::{RemoteConnectionManager, ReconnectPolicy, ConnectionState};
pub use ui::{TerminalUi, SimpleLogViewer, UiConfig, DisplayFilter, UiCommand};