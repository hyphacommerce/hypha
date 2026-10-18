//! An order's payment status (design note 0027, PA7): derived from its payments by
//! `hypha-payments` and stored on the order by `hypha-orders`. Here, in core, because
//! both leaves name it and neither depends on the other.

use std::{fmt, str::FromStr};

/// Where an order's money is (design note 0007, O3; 0027, PA3). The full set is fixed
/// now so clients can rely on it; manual payments use `Pending`, `Paid` and `Voided`,
/// the rest wait for providers that move money that way.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum FinancialStatus {
    /// Nothing paid or authorized yet (or no payment at all).
    Pending,
    /// Authorizations cover the total; nothing captured yet.
    Authorized,
    /// Some of the total is paid, not all of it.
    PartiallyPaid,
    /// Paid amounts cover the total.
    Paid,
    /// Some of what was paid was given back.
    PartiallyRefunded,
    /// Everything paid was given back.
    Refunded,
    /// Every payment was voided before money moved.
    Voided,
}

impl FinancialStatus {
    /// Every status, in the order above.
    pub const ALL: [FinancialStatus; 7] = [
        FinancialStatus::Pending,
        FinancialStatus::Authorized,
        FinancialStatus::PartiallyPaid,
        FinancialStatus::Paid,
        FinancialStatus::PartiallyRefunded,
        FinancialStatus::Refunded,
        FinancialStatus::Voided,
    ];

    /// The same string used in JSON and storage, e.g. "partially_paid".
    pub fn as_str(&self) -> &'static str {
        match self {
            FinancialStatus::Pending => "pending",
            FinancialStatus::Authorized => "authorized",
            FinancialStatus::PartiallyPaid => "partially_paid",
            FinancialStatus::Paid => "paid",
            FinancialStatus::PartiallyRefunded => "partially_refunded",
            FinancialStatus::Refunded => "refunded",
            FinancialStatus::Voided => "voided",
        }
    }
}

impl fmt::Display for FinancialStatus {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl FromStr for FinancialStatus {
    /// The text that didn't match a status.
    type Err = String;

    fn from_str(s: &str) -> Result<Self, String> {
        FinancialStatus::ALL
            .into_iter()
            .find(|status| status.as_str() == s)
            .ok_or_else(|| s.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_status_parses_back_from_its_string() {
        for status in FinancialStatus::ALL {
            assert_eq!(status.as_str().parse(), Ok(status));
            assert_eq!(status.to_string(), status.as_str());
        }
        assert_eq!("Paid".parse::<FinancialStatus>(), Err("Paid".to_string()));
    }
}
