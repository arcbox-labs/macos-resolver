//! Error types.

use thiserror::Error;

/// Result alias for resolver operations.
pub type Result<T> = std::result::Result<T, ResolverError>;

/// Errors returned by resolver operations.
#[derive(Debug, Error)]
pub enum ResolverError {
    /// Filesystem I/O failed (typically `PermissionDenied` on `/etc/resolver/`).
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),

    /// The resolver directory does not exist and could not be created.
    #[error("resolver directory not found: {path}")]
    DirNotFound {
        /// The expected path.
        path: String,
    },

    /// Attempted to remove a resolver file not managed by this crate.
    #[error("resolver file not managed by macos-resolver: {domain}")]
    NotManaged {
        /// The domain whose file is unmanaged.
        domain: String,
    },

    /// Invalid configuration values.
    #[error("invalid config: {0}")]
    InvalidConfig(String),
}

impl ResolverError {
    /// Returns `true` if the underlying I/O error is `PermissionDenied`.
    #[must_use]
    pub fn is_permission_denied(&self) -> bool {
        matches!(self, Self::Io(e) if e.kind() == std::io::ErrorKind::PermissionDenied)
    }
}
