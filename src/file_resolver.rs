//! File-based `/etc/resolver/` management.
//!
//! Each file written by this module contains a marker comment with the
//! creating process's PID, enabling safe ownership checks and orphan cleanup.

use crate::config::ResolverConfig;
use crate::error::{ResolverError, Result};
use crate::util::is_process_alive;
use std::path::{Path, PathBuf};

/// Marker comment embedded in every managed resolver file.
const MANAGED_BY_MARKER: &str = "# managed by arcbox";

/// Default macOS resolver directory.
const DEFAULT_RESOLVER_DIR: &str = "/etc/resolver";

/// Manages `/etc/resolver/<domain>` files.
///
/// # Lifecycle
///
/// 1. [`register`](Self::register) writes a resolver file.
/// 2. macOS picks it up immediately (no restart needed).
/// 3. [`unregister`](Self::unregister) removes the file on shutdown.
///
/// # Crash recovery
///
/// If the process exits without calling [`unregister`](Self::unregister),
/// the file persists. On next startup, call
/// [`cleanup_orphaned`](Self::cleanup_orphaned) to remove files whose
/// creating PID is no longer running.
///
/// # Permissions
///
/// `/etc/resolver/` requires root. The caller must handle elevation.
///
/// # Example
///
/// ```rust,ignore
/// use macos_resolver::{FileResolver, ResolverConfig};
///
/// let resolver = FileResolver::new();
/// resolver.register(&ResolverConfig::new("myapp.local", "127.0.0.1", 5553))?;
/// // ...
/// resolver.unregister("myapp.local")?;
/// ```
pub struct FileResolver {
    resolver_dir: PathBuf,
}

impl FileResolver {
    /// Creates a resolver targeting the default `/etc/resolver` directory.
    #[must_use]
    pub fn new() -> Self {
        Self {
            resolver_dir: PathBuf::from(DEFAULT_RESOLVER_DIR),
        }
    }

    /// Creates a resolver targeting a custom directory (useful for testing).
    #[must_use]
    pub fn with_dir(resolver_dir: impl Into<PathBuf>) -> Self {
        Self {
            resolver_dir: resolver_dir.into(),
        }
    }

    /// Returns the resolver directory path.
    #[must_use]
    pub fn resolver_dir(&self) -> &Path {
        &self.resolver_dir
    }

    /// Writes `/etc/resolver/<domain>` with the given configuration.
    ///
    /// The file contains a marker with the current PID for orphan detection.
    /// Calling this again for the same domain overwrites the previous file.
    ///
    /// # Errors
    ///
    /// Returns [`ResolverError::Io`] if the directory cannot be created or
    /// the file cannot be written.
    pub fn register(&self, config: &ResolverConfig) -> Result<()> {
        if !self.resolver_dir.exists() {
            std::fs::create_dir_all(&self.resolver_dir)?;
        }

        let path = self.resolver_path(&config.domain);
        std::fs::write(&path, generate_file_content(config))?;

        tracing::info!(
            domain = %config.domain,
            port = config.port,
            path = %path.display(),
            "Registered macOS DNS resolver"
        );
        Ok(())
    }

    /// Removes `/etc/resolver/<domain>`.
    ///
    /// Only removes files that contain the ownership marker. Files created
    /// by other tools are left untouched and a [`ResolverError::NotManaged`]
    /// error is returned.
    ///
    /// If the file does not exist, this is a no-op.
    ///
    /// # Errors
    ///
    /// Returns [`ResolverError::Io`] on I/O failure, or
    /// [`ResolverError::NotManaged`] if the file belongs to another tool.
    pub fn unregister(&self, domain: &str) -> Result<()> {
        let path = self.resolver_path(domain);

        if !path.exists() {
            tracing::debug!(domain = %domain, "Resolver file does not exist, skipping");
            return Ok(());
        }

        if !is_managed(&path) {
            tracing::warn!(
                domain = %domain,
                path = %path.display(),
                "Resolver file not managed by this crate, refusing to remove"
            );
            return Err(ResolverError::NotManaged {
                domain: domain.to_string(),
            });
        }

        std::fs::remove_file(&path)?;
        tracing::info!(domain = %domain, "Unregistered macOS DNS resolver");
        Ok(())
    }

    /// Lists all domains with a managed resolver file.
    ///
    /// Returns an empty vec if the directory does not exist.
    ///
    /// # Errors
    ///
    /// Returns [`ResolverError::Io`] if the directory cannot be read.
    pub fn list(&self) -> Result<Vec<String>> {
        if !self.resolver_dir.exists() {
            return Ok(Vec::new());
        }

        let mut domains = Vec::new();
        for entry in std::fs::read_dir(&self.resolver_dir)? {
            let path = entry?.path();
            if path.is_file() && is_managed(&path) {
                if let Some(name) = path.file_name().and_then(|n| n.to_str()) {
                    domains.push(name.to_string());
                }
            }
        }
        Ok(domains)
    }

    /// Returns `true` if `domain` has a managed resolver file on disk.
    #[must_use]
    pub fn is_registered(&self, domain: &str) -> bool {
        let path = self.resolver_path(domain);
        path.exists() && is_managed(&path)
    }

    /// Removes resolver files whose creating PID is no longer running.
    ///
    /// Returns the number of files removed. Non-managed files and files
    /// belonging to still-alive processes are left untouched.
    ///
    /// # Errors
    ///
    /// Returns [`ResolverError::Io`] if the directory cannot be read.
    pub fn cleanup_orphaned(&self) -> Result<usize> {
        if !self.resolver_dir.exists() {
            return Ok(0);
        }

        let mut removed = 0;
        for entry in std::fs::read_dir(&self.resolver_dir)? {
            let path = entry?.path();
            if !path.is_file() || !is_managed(&path) {
                continue;
            }

            if let Some(pid) = extract_pid(&path) {
                if !is_process_alive(pid) {
                    let domain = path
                        .file_name()
                        .and_then(|n| n.to_str())
                        .unwrap_or("unknown");
                    tracing::info!(
                        domain = %domain,
                        pid = pid,
                        "Removing orphaned resolver file (process dead)"
                    );
                    match std::fs::remove_file(&path) {
                        Ok(()) => removed += 1,
                        Err(e) => tracing::warn!(
                            domain = %domain,
                            error = %e,
                            "Failed to remove orphaned resolver file"
                        ),
                    }
                }
            }
        }
        Ok(removed)
    }

    fn resolver_path(&self, domain: &str) -> PathBuf {
        self.resolver_dir.join(domain)
    }
}

impl Default for FileResolver {
    fn default() -> Self {
        Self::new()
    }
}

// ---------------------------------------------------------------------------
// File content helpers
// ---------------------------------------------------------------------------

/// Generates resolver file content.
///
/// ```text
/// # managed by arcbox (pid=12345)
/// nameserver 127.0.0.1
/// port 5553
/// search_order 1
/// ```
fn generate_file_content(config: &ResolverConfig) -> String {
    let pid = std::process::id();
    format!(
        "{MANAGED_BY_MARKER} (pid={pid})\nnameserver {ns}\nport {port}\nsearch_order {order}\n",
        ns = config.nameserver,
        port = config.port,
        order = config.search_order,
    )
}

/// Checks whether a file contains the ownership marker.
fn is_managed(path: &Path) -> bool {
    std::fs::read_to_string(path).is_ok_and(|c| c.contains(MANAGED_BY_MARKER))
}

/// Extracts the PID from `# managed by arcbox (pid=<N>)`.
fn extract_pid(path: &Path) -> Option<u32> {
    let content = std::fs::read_to_string(path).ok()?;
    for line in content.lines() {
        if let Some(rest) = line.strip_prefix(MANAGED_BY_MARKER) {
            let rest = rest.trim().strip_prefix("(pid=")?;
            return rest.strip_suffix(')')?.parse().ok();
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn generate_content_includes_marker_and_pid() {
        let config = ResolverConfig::arcbox_default(5553);
        let content = generate_file_content(&config);

        assert!(content.contains(MANAGED_BY_MARKER));
        assert!(content.contains("nameserver 127.0.0.1"));
        assert!(content.contains("port 5553"));
        assert!(content.contains("search_order 1"));
        assert!(content.contains(&format!("pid={}", std::process::id())));
    }

    #[test]
    fn register_and_unregister() {
        let dir = tempfile::tempdir().unwrap();
        let resolver = FileResolver::with_dir(dir.path());
        let config = ResolverConfig::arcbox_default(5553);

        resolver.register(&config).unwrap();
        assert!(dir.path().join("arcbox.local").exists());
        assert!(resolver.is_registered("arcbox.local"));

        let content = std::fs::read_to_string(dir.path().join("arcbox.local")).unwrap();
        assert!(content.contains(MANAGED_BY_MARKER));
        assert!(content.contains("nameserver 127.0.0.1"));

        assert_eq!(resolver.list().unwrap(), vec!["arcbox.local"]);

        resolver.unregister("arcbox.local").unwrap();
        assert!(!dir.path().join("arcbox.local").exists());
        assert!(!resolver.is_registered("arcbox.local"));
    }

    #[test]
    fn unregister_nonexistent_is_noop() {
        let dir = tempfile::tempdir().unwrap();
        let resolver = FileResolver::with_dir(dir.path());
        resolver.unregister("nonexistent.local").unwrap();
    }

    #[test]
    fn unregister_refuses_unmanaged_file() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("other.local");
        std::fs::write(&path, "nameserver 1.1.1.1\nport 53\n").unwrap();

        let resolver = FileResolver::with_dir(dir.path());
        assert!(resolver.unregister("other.local").is_err());
        assert!(path.exists());
    }

    #[test]
    fn extract_pid_parses_marker() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("test.local");
        std::fs::write(
            &path,
            "# managed by arcbox (pid=42)\nnameserver 127.0.0.1\nport 5553\n",
        )
        .unwrap();
        assert_eq!(extract_pid(&path), Some(42));
    }

    #[test]
    fn cleanup_removes_dead_pid_files() {
        let dir = tempfile::tempdir().unwrap();
        let resolver = FileResolver::with_dir(dir.path());

        let path = dir.path().join("orphan.local");
        std::fs::write(
            &path,
            "# managed by arcbox (pid=999999999)\nnameserver 127.0.0.1\nport 5553\n",
        )
        .unwrap();

        assert_eq!(resolver.cleanup_orphaned().unwrap(), 1);
        assert!(!path.exists());
    }

    #[test]
    fn cleanup_preserves_alive_pid_files() {
        let dir = tempfile::tempdir().unwrap();
        let resolver = FileResolver::with_dir(dir.path());

        let pid = std::process::id();
        let path = dir.path().join("alive.local");
        std::fs::write(
            &path,
            format!("# managed by arcbox (pid={pid})\nnameserver 127.0.0.1\nport 5553\n"),
        )
        .unwrap();

        assert_eq!(resolver.cleanup_orphaned().unwrap(), 0);
        assert!(path.exists());
    }

    #[test]
    fn list_empty_and_nonexistent() {
        let dir = tempfile::tempdir().unwrap();
        assert!(FileResolver::with_dir(dir.path()).list().unwrap().is_empty());
        assert!(FileResolver::with_dir("/nonexistent").list().unwrap().is_empty());
    }

    #[test]
    fn multiple_domains() {
        let dir = tempfile::tempdir().unwrap();
        let resolver = FileResolver::with_dir(dir.path());

        resolver.register(&ResolverConfig::arcbox_default(5553)).unwrap();
        resolver
            .register(
                &ResolverConfig::new("docker.internal", "127.0.0.1", 5553).with_search_order(2),
            )
            .unwrap();

        let mut domains = resolver.list().unwrap();
        domains.sort();
        assert_eq!(domains, vec!["arcbox.local", "docker.internal"]);
    }

    #[test]
    fn register_overwrites() {
        let dir = tempfile::tempdir().unwrap();
        let resolver = FileResolver::with_dir(dir.path());

        resolver.register(&ResolverConfig::arcbox_default(5553)).unwrap();
        resolver.register(&ResolverConfig::arcbox_default(6000)).unwrap();

        let content = std::fs::read_to_string(dir.path().join("arcbox.local")).unwrap();
        assert!(content.contains("port 6000"));
        assert!(!content.contains("port 5553"));
    }
}
