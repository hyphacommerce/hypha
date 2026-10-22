/// Categories' tunable values: from `[categories]` in `hypha.config.toml`, defaulting here.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CategorySettings {
    /// How many levels categories nest, 1 to 10. Lowering it leaves deeper categories
    /// where they are; it applies to creating and moving.
    pub max_depth: u8,
}

impl CategorySettings {
    pub const MAX_DEPTH_LIMIT: u8 = 10;
}

impl Default for CategorySettings {
    fn default() -> Self {
        Self { max_depth: 5 }
    }
}
