use std::io;
#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("I/O operation failed")]
    Io(#[from] io::Error),
    #[error("operation timed out")]
    Timeout,
    #[error("operation cancelled")]
    Cancelled,
    #[error("protocol violation: {0}")]
    Protocol(&'static str),
    #[error("resource limit: {0}")]
    Limit(&'static str),
    #[error("authentication failed")]
    Auth,
    #[error("invalid configuration: {0}")]
    Config(&'static str),
    #[error("capture or encoder ended")]
    Closed,
}
impl Error {
    pub fn retryable(&self) -> bool {
        matches!(self, Self::Io(_) | Self::Timeout | Self::Closed)
    }
}
pub type Result<T> = std::result::Result<T, Error>;
