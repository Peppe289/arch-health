use thiserror::Error;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ErrorKind {
    /// The service/library/file is not available on this system.
    Unavailable,
    PermissionDenied,
    Network,
    /// Data was received but could not be understood.
    Data,
}

/// Error returned by ports. Cloneable so cached results can be shared
/// between checks running on different threads.
#[derive(Debug, Clone, Error)]
#[error("{message}")]
pub struct PortError {
    pub kind: ErrorKind,
    pub message: String,
}

impl PortError {
    pub fn new(kind: ErrorKind, message: impl Into<String>) -> Self {
        Self {
            kind,
            message: message.into(),
        }
    }

    pub fn unavailable(message: impl Into<String>) -> Self {
        Self::new(ErrorKind::Unavailable, message)
    }

    pub fn network(message: impl Into<String>) -> Self {
        Self::new(ErrorKind::Network, message)
    }

    pub fn data(message: impl Into<String>) -> Self {
        Self::new(ErrorKind::Data, message)
    }
}

pub type PortResult<T> = Result<T, PortError>;
