use hypha_core::{OptionId, OptionValueId};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OptionValue {
    id: OptionValueId,
    label: String,
}

impl OptionValue {
    pub fn new(id: OptionValueId, label: String) -> Self {
        Self { id, label }
    }

    pub fn id(&self) -> OptionValueId {
        self.id
    }
    pub fn label(&self) -> &str {
        &self.label
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProductOption {
    id: OptionId,
    name: String,
    values: Vec<OptionValue>,
}

impl ProductOption {
    pub fn new(id: OptionId, name: String, values: Vec<OptionValue>) -> Self {
        Self { id, name, values }
    }

    pub fn id(&self) -> OptionId {
        self.id
    }

    pub fn name(&self) -> &str {
        &self.name
    }

    pub fn values(&self) -> &[OptionValue] {
        &self.values
    }

    pub fn has_value(&self, id: OptionValueId) -> bool {
        self.values.iter().any(|v| v.id() == id)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use uuid::Uuid;

    #[test]
    fn has_value_finds_own_values_only() {
        let small = OptionValueId::from_uuid(Uuid::from_u128(1));
        let other = OptionValueId::from_uuid(Uuid::from_u128(2));

        let size = ProductOption::new(
            OptionId::from_uuid(Uuid::from_u128(10)),
            "Size".to_string(),
            vec![OptionValue::new(small, "S".to_string())],
        );

        assert!(size.has_value(small));
        assert!(!size.has_value(other))
    }
}
