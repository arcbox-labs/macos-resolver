//! Integration tests for `macos-resolver`.
//!
//! Tests marked `#[ignore]` require root:
//!
//! ```bash
//! sudo cargo test -- --ignored
//! ```

use macos_resolver::{FileResolver, ResolverConfig};

// ---------------------------------------------------------------------------
// Tempdir tests (no root required)
// ---------------------------------------------------------------------------

#[test]
fn full_lifecycle() {
    let dir = tempfile::tempdir().unwrap();
    let r = FileResolver::with_dir(dir.path());

    assert!(r.list().unwrap().is_empty());

    // Register two domains.
    r.register(&ResolverConfig::arcbox_default(5553)).unwrap();
    r.register(&ResolverConfig::new("docker.internal", "127.0.0.1", 5553))
        .unwrap();

    assert!(r.is_registered("arcbox.local"));
    let mut domains = r.list().unwrap();
    domains.sort();
    assert_eq!(domains, vec!["arcbox.local", "docker.internal"]);

    // Unregister one at a time.
    r.unregister("arcbox.local").unwrap();
    assert!(!r.is_registered("arcbox.local"));
    assert!(r.is_registered("docker.internal"));

    r.unregister("docker.internal").unwrap();
    assert!(r.list().unwrap().is_empty());
}

#[test]
fn orphan_cleanup() {
    let dir = tempfile::tempdir().unwrap();
    let r = FileResolver::with_dir(dir.path());

    // Dead process.
    std::fs::write(
        dir.path().join("stale.local"),
        "# managed by arcbox (pid=999999999)\nnameserver 127.0.0.1\nport 5553\nsearch_order 1\n",
    )
    .unwrap();

    // Alive process (ourselves).
    let pid = std::process::id();
    std::fs::write(
        dir.path().join("alive.local"),
        format!(
            "# managed by arcbox (pid={pid})\nnameserver 127.0.0.1\nport 5553\nsearch_order 1\n"
        ),
    )
    .unwrap();

    // Unmanaged file.
    std::fs::write(dir.path().join("other.local"), "nameserver 8.8.8.8\n").unwrap();

    assert_eq!(r.cleanup_orphaned().unwrap(), 1);

    assert!(!dir.path().join("stale.local").exists());
    assert!(dir.path().join("alive.local").exists());
    assert!(dir.path().join("other.local").exists());
}

#[test]
fn idempotent_register() {
    let dir = tempfile::tempdir().unwrap();
    let r = FileResolver::with_dir(dir.path());
    let config = ResolverConfig::arcbox_default(5553);

    r.register(&config).unwrap();
    r.register(&config).unwrap();
    assert_eq!(r.list().unwrap().len(), 1);
}

#[test]
fn idempotent_unregister() {
    let dir = tempfile::tempdir().unwrap();
    let r = FileResolver::with_dir(dir.path());

    r.register(&ResolverConfig::arcbox_default(5553)).unwrap();
    r.unregister("arcbox.local").unwrap();
    // Second call is a no-op (file already gone).
    r.unregister("arcbox.local").unwrap();
}

// ---------------------------------------------------------------------------
// Root-only tests
// ---------------------------------------------------------------------------

#[test]
#[ignore = "requires root to write /etc/resolver/"]
fn real_register_and_unregister() {
    let r = FileResolver::new();
    let config = ResolverConfig::new("arcbox-test.local", "127.0.0.1", 15553);

    r.register(&config).unwrap();
    assert!(r.is_registered("arcbox-test.local"));
    assert!(std::path::Path::new("/etc/resolver/arcbox-test.local").exists());

    r.unregister("arcbox-test.local").unwrap();
    assert!(!std::path::Path::new("/etc/resolver/arcbox-test.local").exists());
}
