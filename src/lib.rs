//! # macos-resolver
//!
//! Manage macOS `/etc/resolver/` files for custom DNS domain resolution.
//!
//! macOS reads files under `/etc/resolver/<domain>` to route DNS queries for
//! specific domain suffixes to designated nameservers. This crate provides a
//! safe, idempotent API to register, unregister, and list these resolver
//! entries — with built-in orphan cleanup for crash recovery.
//!
//! ## Quick start
//!
//! ```rust,ignore
//! use macos_resolver::{FileResolver, ResolverConfig};
//!
//! let resolver = FileResolver::new("myapp");
//!
//! // Register (requires root).
//! resolver.register(&ResolverConfig::new("myapp.local", "127.0.0.1", 5553))?;
//!
//! // Query state.
//! assert!(resolver.is_registered("myapp.local"));
//! let domains = resolver.list()?;
//!
//! // Unregister on shutdown.
//! resolver.unregister("myapp.local")?;
//! ```
//!
//! ## Crash recovery
//!
//! Each resolver file records the PID of the process that created it.
//! On next startup, call [`FileResolver::cleanup_orphaned`] to remove stale
//! files left by processes that exited without cleaning up:
//!
//! ```rust,ignore
//! let resolver = FileResolver::new("myapp");
//! let removed = resolver.cleanup_orphaned()?;
//! ```
//!
//! ## Verification
//!
//! Changes take effect immediately — no daemon restart needed. Verify with:
//!
//! ```bash
//! scutil --dns              # show all registered resolvers
//! dscacheutil -q host -a name test.myapp.local
//! ```
//!
//! **Note:** `dig` bypasses the macOS system resolver and will *not* show
//! these entries. Use `scutil`, `dscacheutil`, or `ping` instead.
//!
//! ## Permissions
//!
//! Writing to `/etc/resolver/` requires root. The caller is responsible for
//! privilege elevation (`sudo`, `launchd` helper, `SMAppService`, etc.).

#![warn(clippy::all, clippy::pedantic, clippy::nursery)]
#![allow(clippy::module_name_repetitions)]

pub mod config;
pub mod error;
pub mod file_resolver;
pub mod util;

pub use config::ResolverConfig;
pub use error::{ResolverError, Result};
pub use file_resolver::{FileResolver, to_env_prefix};
