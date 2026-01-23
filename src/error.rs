use thiserror::Error;

#[derive(Error, Debug)]
pub enum TelemetryError {
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    #[error("System info error: {0}")]
    SystemInfo(String),

    #[error("Process error: {0}")]
    Process(String),

    #[error("Network error: {0}")]
    Network(String),

    #[error("WebSocket error: {0}")]
    WebSocket(String),

    #[error("Configuration error: {0}")]
    Config(String),

    #[error("Daemon not running")]
    DaemonNotRunning,

    #[error("Daemon already running")]
    DaemonAlreadyRunning,

    #[error("Unknown error: {0}")]
    Unknown(String),
}

pub type Result<T> = std::result::Result<T, TelemetryError>;