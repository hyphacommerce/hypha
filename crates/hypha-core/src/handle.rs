use std::fmt;

use thiserror::Error;

/// What a product or category is addressed by in URLs (`/products/blue-shirt`):
/// lowercase ASCII letters, digits and single dashes, not at either end.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Handle(String);

#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum HandleError {
    #[error("handle is empty")]
    Empty,
    #[error("invalid character {0:?}")]
    InvalidChar(char),
    #[error("handle can't start or end with '-'")]
    EdgeDash,
    #[error("handle can't contain '--'")]
    DoubleDash,
}

impl crate::ErrorCode for HandleError {
    fn code(&self) -> &'static str {
        match self {
            HandleError::Empty
            | HandleError::InvalidChar(_)
            | HandleError::EdgeDash
            | HandleError::DoubleDash => "invalid_handle",
        }
    }
}

impl Handle {
    pub fn new(s: &str) -> Result<Self, HandleError> {
        if s.is_empty() {
            return Err(HandleError::Empty);
        }
        if let Some(c) = s
            .chars()
            .find(|c| !(c.is_ascii_lowercase() || c.is_ascii_digit() || *c == '-'))
        {
            return Err(HandleError::InvalidChar(c));
        }

        if s.starts_with('-') || s.ends_with('-') {
            return Err(HandleError::EdgeDash);
        }

        if s.contains("--") {
            return Err(HandleError::DoubleDash);
        }

        Ok(Self(s.to_string()))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for Handle {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accepts_valid_handle() {
        let valid_handle = "blue-shirt-42";
        assert!(Handle::new(valid_handle).is_ok());
    }

    #[test]
    fn rejects_empty_string() {
        let empty = "";
        assert_eq!(Handle::new(empty), Err(HandleError::Empty));
    }

    #[test]
    fn rejects_uppercase_letters() {
        let upper = "Blue-Shirt";
        assert_eq!(Handle::new(upper), Err(HandleError::InvalidChar('B')));
    }

    #[test]
    fn rejects_spaces() {
        let space = " ";
        assert_eq!(Handle::new(space), Err(HandleError::InvalidChar(' ')));
    }
    #[test]
    fn rejects_edge_dash() {
        for handle in ["-blue", "blue-"] {
            assert_eq!(
                Handle::new(handle),
                Err(HandleError::EdgeDash),
                "handle {handle:?}"
            );
        }
    }
}
