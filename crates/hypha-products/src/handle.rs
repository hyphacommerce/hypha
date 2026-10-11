use crate::error::{HandleError, ProductError};

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Handle(String);

impl Handle {
    pub fn new(s: &str) -> Result<Self, ProductError> {
        if s.is_empty() {
            return Err(ProductError::InvalidHandle(HandleError::Empty));
        }
        if let Some(c) = s
            .chars()
            .find(|c| !(c.is_ascii_lowercase() || c.is_ascii_digit() || *c == '-'))
        {
            return Err(ProductError::InvalidHandle(HandleError::InvalidChar(c)));
        }

        if s.starts_with('-') || s.ends_with('-') {
            return Err(ProductError::InvalidHandle(HandleError::EdgeDash));
        }

        if s.contains("--") {
            return Err(ProductError::InvalidHandle(HandleError::DoubleDash));
        }

        Ok(Self(s.to_string()))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accepts_valid_handle() {
        let valid_handle = "blue-shirt-42";
        assert!(Handle::new(valid_handle).is_ok())
    }

    #[test]
    fn rejects_empty_string() {
        let empty = "";
        assert_eq!(
            Handle::new(empty),
            Err(ProductError::InvalidHandle(HandleError::Empty))
        )
    }

    #[test]
    fn rejects_uppercase_letters() {
        let upper = "Blue-Shirt";
        assert_eq!(
            Handle::new(upper),
            Err(ProductError::InvalidHandle(HandleError::InvalidChar('B')))
        )
    }

    #[test]
    fn rejects_spaces() {
        let space = " ";
        assert_eq!(
            Handle::new(space),
            Err(ProductError::InvalidHandle(HandleError::InvalidChar(' ')))
        )
    }
    #[test]
    fn rejects_edge_dash() {
        for handle in ["-blue", "blue-"] {
            assert_eq!(
                Handle::new(handle),
                Err(ProductError::InvalidHandle(HandleError::EdgeDash)),
                "handle {handle:?}"
            )
        }
    }
}
