/// Optimistic-concurrency version of a stored aggregate.
///
/// `Version::NEW` (0) means "never stored". A repository's `insert` stores version 1
/// and each successful `update` adds one. An `update` from an older version fails with
/// `RepoError::StaleVersion` instead of overwriting someone else's change.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Version(u64);

impl Version {
    pub const NEW: Version = Version(0);

    /// For repositories rebuilding an aggregate from storage.
    pub fn new(value: u64) -> Self {
        Self(value)
    }

    pub fn value(&self) -> u64 {
        self.0
    }

    pub fn next(self) -> Version {
        Version(self.0.saturating_add(1))
    }
}

/// Aggregate roots that repositories store with optimistic concurrency.
pub trait Versioned {
    fn version(&self) -> Version;

    /// For repositories only: records the version a successful write produced.
    /// Domain code never calls this; the version carries no business meaning.
    fn set_version(&mut self, version: Version);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_is_zero_and_next_counts_up() {
        assert_eq!(Version::NEW.value(), 0);
        assert_eq!(Version::NEW.next(), Version::new(1));
        assert_eq!(Version::new(41).next().value(), 42);
    }
}
