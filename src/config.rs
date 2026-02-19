//! Resolver entry configuration.

/// Configuration for a single `/etc/resolver/<domain>` entry.
///
/// # Example
///
/// ```
/// use macos_resolver::ResolverConfig;
///
/// let config = ResolverConfig::new("myapp.local", "127.0.0.1", 5553)
///     .with_search_order(10);
///
/// assert_eq!(config.domain, "myapp.local");
/// assert_eq!(config.port, 5553);
/// assert_eq!(config.search_order, 10);
/// ```
#[derive(Debug, Clone)]
pub struct ResolverConfig {
    /// Domain suffix (e.g., `"myapp.local"`).
    /// Becomes the filename under `/etc/resolver/`.
    pub domain: String,

    /// Nameserver IP address (e.g., `"127.0.0.1"`).
    pub nameserver: String,

    /// DNS port. Standard DNS uses 53; custom resolvers typically use a
    /// high port (e.g., 5553) to avoid conflicts.
    pub port: u16,

    /// Search order — lower values are tried first.
    pub search_order: u32,
}

impl ResolverConfig {
    /// Creates a new resolver config with `search_order = 1`.
    #[must_use]
    pub fn new(domain: impl Into<String>, nameserver: impl Into<String>, port: u16) -> Self {
        Self {
            domain: domain.into(),
            nameserver: nameserver.into(),
            port,
            search_order: 1,
        }
    }

    /// Overrides the search order.
    #[must_use]
    pub const fn with_search_order(mut self, order: u32) -> Self {
        self.search_order = order;
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_sets_defaults() {
        let c = ResolverConfig::new("test.local", "127.0.0.1", 5553);
        assert_eq!(c.domain, "test.local");
        assert_eq!(c.nameserver, "127.0.0.1");
        assert_eq!(c.port, 5553);
        assert_eq!(c.search_order, 1);
    }

    #[test]
    fn with_search_order() {
        let c = ResolverConfig::new("x.local", "127.0.0.1", 53).with_search_order(10);
        assert_eq!(c.search_order, 10);
    }
}
