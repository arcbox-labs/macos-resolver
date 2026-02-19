//! File-based `/etc/resolver/` management.
//!
//! Each file written by this module contains a caller-defined marker prefix
//! (e.g. `# managed by myapp`) with an optional PID, enabling safe ownership
//! checks and orphan cleanup.

use crate::config::ResolverConfig;
use crate::error::{ResolverError, Result};
use crate::util::is_process_alive;
use std::path::{Path, PathBuf};

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
/// let resolver = FileResolver::new("myapp");
/// resolver.register(&ResolverConfig::new("myapp.local", "127.0.0.1", 5553))?;
/// // ...
/// resolver.unregister("myapp.local")?;
/// ```
pub struct FileResolver {
    resolver_dir: PathBuf,
    /// Marker prefix, e.g. `"myapp"`.
    marker: String,
}

impl FileResolver {
    /// Creates a resolver targeting the default `/etc/resolver` directory.
    ///
    /// `prefix` is used for two purposes:
    ///
    /// 1. **Marker comment** — files are tagged with `# managed by <prefix>`.
    /// 2. **Environment variable namespace** — `{PREFIX}_RESOLVER_DIR` overrides
    ///    the default `/etc/resolver` directory (prefix is uppercased, `-` → `_`).
    #[must_use]
    pub fn new(prefix: &str) -> Self {
        let env_key = format!("{}_RESOLVER_DIR", to_env_prefix(prefix));
        let resolver_dir = std::env::var(env_key)
            .map_or_else(|_| PathBuf::from(DEFAULT_RESOLVER_DIR), PathBuf::from);
        Self {
            resolver_dir,
            marker: format!("# managed by {prefix}"),
        }
    }

    /// Creates a resolver with an exact marker string (written as-is).
    ///
    /// Use this when you need full control over the marker comment.
    #[must_use]
    pub fn with_marker(marker: impl Into<String>) -> Self {
        Self {
            resolver_dir: PathBuf::from(DEFAULT_RESOLVER_DIR),
            marker: marker.into(),
        }
    }

    /// Overrides the resolver directory (useful for testing).
    #[must_use]
    pub fn dir(mut self, resolver_dir: impl Into<PathBuf>) -> Self {
        self.resolver_dir = resolver_dir.into();
        self
    }

    /// Returns the resolver directory path.
    #[must_use]
    pub fn resolver_dir(&self) -> &Path {
        &self.resolver_dir
    }

    /// Returns the marker string used to identify managed files.
    #[must_use]
    pub fn marker(&self) -> &str {
        &self.marker
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
        let pid = std::process::id();
        let content = format!(
            "{marker} (pid={pid})\nnameserver {ns}\nport {port}\nsearch_order {order}\n",
            marker = self.marker,
            ns = config.nameserver,
            port = config.port,
            order = config.search_order,
        );
        std::fs::write(&path, content)?;

        tracing::info!(
            domain = %config.domain,
            port = config.port,
            path = %path.display(),
            "Registered macOS DNS resolver"
        );
        Ok(())
    }

    /// Writes `/etc/resolver/<domain>` as a permanent (static) entry.
    ///
    /// Unlike [`register`](Self::register), this does **not** embed a PID in
    /// the marker comment. The file is therefore immune to
    /// [`cleanup_orphaned`](Self::cleanup_orphaned) (which skips files without
    /// a PID) and survives daemon restarts.
    ///
    /// Intended for one-time installation commands (e.g. `sudo myapp dns install`).
    ///
    /// # Errors
    ///
    /// Returns [`ResolverError::Io`] if the directory cannot be created or
    /// the file cannot be written.
    pub fn register_permanent(&self, config: &ResolverConfig) -> Result<()> {
        if !self.resolver_dir.exists() {
            std::fs::create_dir_all(&self.resolver_dir)?;
        }

        let path = self.resolver_path(&config.domain);
        let content = format!(
            "{marker}\nnameserver {ns}\nport {port}\nsearch_order {order}\n",
            marker = self.marker,
            ns = config.nameserver,
            port = config.port,
            order = config.search_order,
        );
        std::fs::write(&path, content)?;

        tracing::info!(
            domain = %config.domain,
            port = config.port,
            path = %path.display(),
            "Registered permanent macOS DNS resolver"
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

        if !self.is_managed(&path) {
            tracing::warn!(
                domain = %domain,
                path = %path.display(),
                "Resolver file not managed by this instance, refusing to remove"
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
            if path.is_file() && self.is_managed(&path) {
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
        path.exists() && self.is_managed(&path)
    }

    /// Removes resolver files whose creating PID is no longer running.
    ///
    /// Returns the number of files removed. Non-managed files and files
    /// belonging to still-alive processes are left untouched.
    /// Permanent files (no PID) are also left untouched.
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
            if !path.is_file() || !self.is_managed(&path) {
                continue;
            }

            if let Some(pid) = self.extract_pid(&path) {
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

    /// Checks whether a file contains this instance's marker.
    fn is_managed(&self, path: &Path) -> bool {
        std::fs::read_to_string(path).is_ok_and(|c| c.contains(&self.marker))
    }

    /// Extracts the PID from `# managed by <app> (pid=<N>)`.
    fn extract_pid(&self, path: &Path) -> Option<u32> {
        let content = std::fs::read_to_string(path).ok()?;
        for line in content.lines() {
            if let Some(rest) = line.strip_prefix(self.marker.as_str()) {
                let rest = rest.trim().strip_prefix("(pid=")?;
                return rest.strip_suffix(')')?.parse().ok();
            }
        }
        None
    }
}

/// Converts a prefix like `"my-app"` to an environment variable prefix `"MY_APP"`.
///
/// Uppercases and replaces `-` with `_`.
#[must_use]
pub fn to_env_prefix(prefix: &str) -> String {
    prefix.to_uppercase().replace('-', "_")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_config() -> ResolverConfig {
        ResolverConfig::new("test.local", "127.0.0.1", 5553)
    }

    #[test]
    fn marker_is_derived_from_prefix() {
        let resolver = FileResolver::new("myapp");
        assert_eq!(resolver.marker(), "# managed by myapp");
    }

    #[test]
    fn register_writes_file_with_pid() {
        let dir = tempfile::tempdir().unwrap();
        let resolver = FileResolver::new("testapp").dir(dir.path());
        let config = test_config();

        resolver.register(&config).unwrap();
        let content = std::fs::read_to_string(dir.path().join("test.local")).unwrap();

        assert!(content.contains("testapp"));
        assert!(content.contains("nameserver 127.0.0.1"));
        assert!(content.contains("port 5553"));
        assert!(content.contains("search_order 1"));
        assert!(content.contains(&format!("pid={}", std::process::id())));
    }

    #[test]
    fn register_and_unregister() {
        let dir = tempfile::tempdir().unwrap();
        let resolver = FileResolver::new("testapp").dir(dir.path());
        let config = test_config();

        resolver.register(&config).unwrap();
        assert!(dir.path().join("test.local").exists());
        assert!(resolver.is_registered("test.local"));
        assert_eq!(resolver.list().unwrap(), vec!["test.local"]);

        resolver.unregister("test.local").unwrap();
        assert!(!dir.path().join("test.local").exists());
        assert!(!resolver.is_registered("test.local"));
    }

    #[test]
    fn unregister_nonexistent_is_noop() {
        let dir = tempfile::tempdir().unwrap();
        let resolver = FileResolver::new("testapp").dir(dir.path());
        resolver.unregister("nonexistent.local").unwrap();
    }

    #[test]
    fn unregister_refuses_unmanaged_file() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("other.local");
        std::fs::write(&path, "nameserver 1.1.1.1\nport 53\n").unwrap();

        let resolver = FileResolver::new("testapp").dir(dir.path());
        assert!(resolver.unregister("other.local").is_err());
        assert!(path.exists());
    }

    #[test]
    fn unregister_refuses_file_from_different_app() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("shared.local");
        std::fs::write(
            &path,
            "# managed by otherapp\nnameserver 127.0.0.1\nport 53\n",
        )
        .unwrap();

        let resolver = FileResolver::new("myapp").dir(dir.path());
        assert!(resolver.unregister("shared.local").is_err());
        assert!(path.exists());
    }

    #[test]
    fn extract_pid_parses_marker() {
        let dir = tempfile::tempdir().unwrap();
        let resolver = FileResolver::new("testapp").dir(dir.path());
        let path = dir.path().join("test.local");
        std::fs::write(
            &path,
            "# managed by testapp (pid=42)\nnameserver 127.0.0.1\nport 5553\n",
        )
        .unwrap();
        assert_eq!(resolver.extract_pid(&path), Some(42));
    }

    #[test]
    fn cleanup_removes_dead_pid_files() {
        let dir = tempfile::tempdir().unwrap();
        let resolver = FileResolver::new("testapp").dir(dir.path());

        let path = dir.path().join("orphan.local");
        std::fs::write(
            &path,
            "# managed by testapp (pid=999999999)\nnameserver 127.0.0.1\nport 5553\n",
        )
        .unwrap();

        assert_eq!(resolver.cleanup_orphaned().unwrap(), 1);
        assert!(!path.exists());
    }

    #[test]
    fn cleanup_preserves_alive_pid_files() {
        let dir = tempfile::tempdir().unwrap();
        let resolver = FileResolver::new("testapp").dir(dir.path());

        let pid = std::process::id();
        let path = dir.path().join("alive.local");
        std::fs::write(
            &path,
            format!("# managed by testapp (pid={pid})\nnameserver 127.0.0.1\nport 5553\n"),
        )
        .unwrap();

        assert_eq!(resolver.cleanup_orphaned().unwrap(), 0);
        assert!(path.exists());
    }

    #[test]
    fn list_empty_and_nonexistent() {
        let dir = tempfile::tempdir().unwrap();
        assert!(
            FileResolver::new("testapp")
                .dir(dir.path())
                .list()
                .unwrap()
                .is_empty()
        );
        assert!(
            FileResolver::new("testapp")
                .dir("/nonexistent")
                .list()
                .unwrap()
                .is_empty()
        );
    }

    #[test]
    fn multiple_domains() {
        let dir = tempfile::tempdir().unwrap();
        let resolver = FileResolver::new("testapp").dir(dir.path());

        resolver.register(&test_config()).unwrap();
        resolver
            .register(
                &ResolverConfig::new("docker.internal", "127.0.0.1", 5553).with_search_order(2),
            )
            .unwrap();

        let mut domains = resolver.list().unwrap();
        domains.sort();
        assert_eq!(domains, vec!["docker.internal", "test.local"]);
    }

    #[test]
    fn register_permanent_creates_file_without_pid() {
        let dir = tempfile::tempdir().unwrap();
        let resolver = FileResolver::new("testapp").dir(dir.path());
        let config = test_config();

        resolver.register_permanent(&config).unwrap();
        assert!(dir.path().join("test.local").exists());
        assert!(resolver.is_registered("test.local"));

        let content = std::fs::read_to_string(dir.path().join("test.local")).unwrap();
        assert!(content.contains("testapp"));
        assert!(content.contains("nameserver 127.0.0.1"));
        assert!(content.contains("port 5553"));
        assert!(!content.contains("pid="));
    }

    #[test]
    fn cleanup_skips_permanent_files() {
        let dir = tempfile::tempdir().unwrap();
        let resolver = FileResolver::new("testapp").dir(dir.path());
        let config = test_config();

        resolver.register_permanent(&config).unwrap();
        assert_eq!(resolver.cleanup_orphaned().unwrap(), 0);
        assert!(dir.path().join("test.local").exists());
    }

    #[test]
    fn register_overwrites() {
        let dir = tempfile::tempdir().unwrap();
        let resolver = FileResolver::new("testapp").dir(dir.path());

        resolver.register(&test_config()).unwrap();
        resolver
            .register(&ResolverConfig::new("test.local", "127.0.0.1", 6000))
            .unwrap();

        let content = std::fs::read_to_string(dir.path().join("test.local")).unwrap();
        assert!(content.contains("port 6000"));
        assert!(!content.contains("port 5553"));
    }
}
