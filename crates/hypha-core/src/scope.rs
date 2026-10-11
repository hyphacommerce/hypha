use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Scope {
    #[serde(rename = "products:read")]
    ProductsRead,
    #[serde(rename = "products:write")]
    ProductsWrite,
    #[serde(rename = "markets:read")]
    MarketsRead,
    #[serde(rename = "markets:write")]
    MarketsWrite,
}

impl Scope {
    pub const ALL: [Scope; 4] = [
        Scope::ProductsRead,
        Scope::ProductsWrite,
        Scope::MarketsRead,
        Scope::MarketsWrite,
    ];
}

#[cfg(test)]
mod test {
    use super::*;

    #[test]
    fn uses_colon_names_in_json() {
        assert_eq!(
            serde_json::to_string(&Scope::ProductsWrite).unwrap(),
            "\"products:write\""
        );
        assert_eq!(
            serde_json::from_str::<Scope>("\"markets:read\"").unwrap(),
            Scope::MarketsRead
        );
    }
}
