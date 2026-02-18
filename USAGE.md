# Usage

## Installation

Add to your `Cargo.toml`:

```toml
[dependencies]
macos-resolver = { git = "https://github.com/arcbox-labs/macos-resolver" }
```

## Quick start

```rust
use macos_resolver::{FileResolver, ResolverConfig};

let resolver = FileResolver::new();

// Register a domain → creates /etc/resolver/myapp.local
resolver.register(&ResolverConfig::new("myapp.local", "127.0.0.1", 5553))?;

// Check state
assert!(resolver.is_registered("myapp.local"));
let domains = resolver.list()?;

// Unregister on shutdown → removes /etc/resolver/myapp.local
resolver.unregister("myapp.local")?;
```

## API

### `FileResolver`

| Method | Description |
|--------|-------------|
| `new()` | Target the default `/etc/resolver` directory |
| `with_dir(path)` | Target a custom directory (useful for testing) |
| `register(config)` | Write a resolver file for the given domain |
| `unregister(domain)` | Remove a managed resolver file |
| `is_registered(domain)` | Check if a managed resolver file exists |
| `list()` | List all managed domains |
| `cleanup_orphaned()` | Remove files left by dead processes |

### `ResolverConfig`

| Method | Description |
|--------|-------------|
| `new(domain, nameserver, port)` | Create a config with `search_order = 1` |
| `with_search_order(order)` | Override the search order |
| `arcbox_default(port)` | Shorthand for `arcbox.local` → `127.0.0.1` |

### Error handling

```rust
use macos_resolver::ResolverError;

match resolver.register(&config) {
    Ok(()) => println!("registered"),
    Err(e) if e.is_permission_denied() => eprintln!("run with sudo"),
    Err(e) => eprintln!("error: {e}"),
}
```

## Crash recovery

Each resolver file records the PID of the process that created it. On startup, call `cleanup_orphaned()` to remove stale files left by processes that crashed without cleaning up:

```rust
let resolver = FileResolver::new();
let removed = resolver.cleanup_orphaned()?;
// removed = number of stale files cleaned up
```

## File format

Files written to `/etc/resolver/` look like:

```
# managed by arcbox (pid=12345)
nameserver 127.0.0.1
port 5553
search_order 1
```

The `# managed by arcbox` marker is used for ownership detection — this crate will **never** modify or delete files it didn't create.

## Verification

Changes take effect immediately. Verify with:

```bash
scutil --dns                                         # list all resolvers
dscacheutil -q host -a name test.myapp.local         # test resolution
```

> **Note:** `dig` bypasses the macOS system resolver and will not show these entries. Use `scutil`, `dscacheutil`, or `ping` instead.

## Permissions

Writing to `/etc/resolver/` requires root. The caller is responsible for privilege elevation:

| Method | Use case |
|--------|----------|
| `sudo` | CLI / manual operations |
| `launchd` helper | Background services |
| `SMAppService` | macOS 13+ privileged helper |

## Testing

```bash
cargo test                        # unit + integration tests (no root needed)
sudo cargo test -- --ignored      # tests that write to /etc/resolver/
```
