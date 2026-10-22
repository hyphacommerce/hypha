//! Customer lists: the summary a table shows, what a list is narrowed to, and how it is
//! ordered.

use hypha_core::{
    CustomerGroupId, CustomerId, Direction, Email, FieldFilter, SortKey, Stamps, optional_text,
};

use crate::{Customer, CustomerError};

/// A customer in a list: without addresses, which only the customer's page shows.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CustomerSummary {
    pub id: CustomerId,
    pub email: Email,
    pub first_name: Option<String>,
    pub last_name: Option<String>,
    pub phone: Option<String>,
    pub address_count: usize,
    pub stamps: Option<Stamps>,
}

impl Customer {
    pub fn summary(&self) -> CustomerSummary {
        CustomerSummary {
            id: self.id(),
            email: self.email().clone(),
            first_name: self.first_name().map(str::to_string),
            last_name: self.last_name().map(str::to_string),
            phone: self.phone().map(str::to_string),
            address_count: self.addresses().len(),
            stamps: hypha_core::Stamped::stamps(self),
        }
    }
}

/// A customer list's filter, checked.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct CustomerFilter {
    q: Option<String>,
    fields: Vec<FieldFilter>,
    group: Option<CustomerGroupId>,
}

impl CustomerFilter {
    /// `q`, text the email, first or last name or phone contains (ignoring case),
    /// follows the shared text rule; blank is no filter.
    pub fn new(q: Option<String>) -> Result<Self, CustomerError> {
        Ok(Self {
            q: optional_text("q", q)?,
            fields: Vec::new(),
            group: None,
        })
    }

    /// Only customers in `group`. `matches` doesn't see it: adapters apply it to the
    /// membership.
    pub fn in_group(mut self, group: Option<CustomerGroupId>) -> Self {
        self.group = group;
        self
    }

    pub fn group(&self) -> Option<CustomerGroupId> {
        self.group
    }

    /// Filters on custom fields, checked against their definitions by `hypha-custom-fields`
    /// already; every one must match. `matches` doesn't see them: adapters apply them to
    /// the stored values.
    pub fn with_fields(mut self, fields: Vec<FieldFilter>) -> Self {
        self.fields = fields;
        self
    }

    pub fn fields(&self) -> &[FieldFilter] {
        &self.fields
    }

    pub fn q(&self) -> Option<&str> {
        self.q.as_deref()
    }

    /// Whether `customer` is in the list. Adapters that filter in storage must agree
    /// (the repository contract checks).
    pub fn matches(&self, customer: &Customer) -> bool {
        self.q.as_ref().is_none_or(|q| {
            let q = q.to_lowercase();
            let has = |text: &str| text.to_lowercase().contains(&q);
            has(customer.email().as_str())
                || customer.first_name().is_some_and(has)
                || customer.last_name().is_some_and(has)
                || customer.phone().is_some_and(has)
        })
    }
}

/// How a customer list is ordered; ties go by ID in the same direction. A name is the first
/// name, a space and the last name, missing parts empty, compared as `SortKey::text` does.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum CustomerSort {
    #[default]
    Newest,
    Oldest,
    EmailAsc,
    EmailDesc,
    NameAsc,
    NameDesc,
}

impl CustomerSort {
    pub fn direction(self) -> Direction {
        match self {
            CustomerSort::Oldest | CustomerSort::EmailAsc | CustomerSort::NameAsc => {
                Direction::Ascending
            }
            CustomerSort::Newest | CustomerSort::EmailDesc | CustomerSort::NameDesc => {
                Direction::Descending
            }
        }
    }

    pub fn key(self, summary: &CustomerSummary) -> SortKey {
        match self {
            CustomerSort::Newest | CustomerSort::Oldest => SortKey::Id,
            CustomerSort::EmailAsc | CustomerSort::EmailDesc => {
                SortKey::text(summary.email.as_str())
            }
            CustomerSort::NameAsc | CustomerSort::NameDesc => SortKey::text(&format!(
                "{} {}",
                summary.first_name.as_deref().unwrap_or(""),
                summary.last_name.as_deref().unwrap_or("")
            )),
        }
    }

    /// Whether `key` is the kind this sort uses: a cursor made for another sort doesn't.
    pub fn fits(self, key: &SortKey) -> bool {
        match self {
            CustomerSort::Newest | CustomerSort::Oldest => key == &SortKey::Id,
            _ => matches!(key, SortKey::Text(_)),
        }
    }
}

#[cfg(test)]
mod tests {
    use hypha_core::TextError;
    use uuid::Uuid;

    use super::*;

    #[test]
    fn a_name_key_is_first_then_last_name() {
        let summary = CustomerSummary {
            id: CustomerId::from_uuid(Uuid::from_u128(1)),
            email: Email::new("ana@example.hr").unwrap(),
            first_name: Some("Ana".to_string()),
            last_name: None,
            phone: None,
            address_count: 0,
            stamps: None,
        };
        assert_eq!(CustomerSort::NameAsc.key(&summary), SortKey::text("ana "));
        assert_eq!(
            CustomerSort::EmailDesc.key(&summary),
            SortKey::text("ana@example.hr")
        );
    }

    #[test]
    fn text_follows_the_text_rule() {
        assert_eq!(
            CustomerFilter::new(Some("x".repeat(256))),
            Err(CustomerError::Text(TextError::TooLong("q")))
        );
    }

    #[test]
    fn each_sort_fits_only_its_own_key() {
        assert!(CustomerSort::Oldest.fits(&SortKey::Id));
        assert!(CustomerSort::NameAsc.fits(&SortKey::text("a")));
        assert!(!CustomerSort::EmailAsc.fits(&SortKey::Id));
    }
}
