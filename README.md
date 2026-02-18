# macos-resolver

Manage macOS `/etc/resolver/` files for custom DNS domain resolution.

macOS reads files under `/etc/resolver/<domain>` to route DNS queries for specific domain suffixes to designated nameservers. This crate provides a safe, idempotent Rust API to register, unregister, and list these resolver entries — with built-in orphan cleanup for crash recovery.

## Use case

If you run a local DNS server (e.g., for container runtimes, development tools, or VPN integrations) and want host applications to resolve custom domains like `*.myapp.local`, this crate handles the system-level plumbing.

## Quick start

```rust
use macos_resolver::{FileResolver, ResolverConfig};

let resolver = FileResolver::new();

resolver.register(&ResolverConfig::new("myapp.local", "127.0.0.1", 5553))?;
assert!(resolver.is_registered("myapp.local"));
resolver.unregister("myapp.local")?;
```

See [USAGE.md](USAGE.md) for the full API reference, crash recovery, file format, verification, and permissions guide.

## License

MIT OR Apache-2.0
