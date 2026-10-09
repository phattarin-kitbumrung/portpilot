use thiserror::Error;

#[derive(Debug, Error)]
pub enum PortPilotError {
    #[error("port {0} not found")]
    PortNotFound(u16),

    #[error("process '{0}' not found")]
    ProcessNotFound(String),

    #[error("pid {0} not found")]
    PidNotFound(u32),

    #[error("invalid input: {0}")]
    InvalidInput(String),

    #[error("operation failed: {0}")]
    OperationFailed(String),

    #[error("platform error: {0}")]
    Platform(String),
}
