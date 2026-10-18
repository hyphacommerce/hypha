use hypha_core::{Currency, ErrorCode, MoneyError, PaymentId, PaymentMethodId, TextError};
use thiserror::Error;

use crate::{PaymentMethod, PaymentStatus, ProviderKey};

/// Why a payment method or a payment can't be built or changed, or a provider refused.
#[derive(Debug, Clone, Error, PartialEq, Eq)]
pub enum PaymentError {
    #[error(transparent)]
    Text(#[from] TextError),
    #[error(transparent)]
    Money(#[from] MoneyError),
    /// Not `manual` or `spore_<name>` (lowercase ASCII letters, digits and `_`).
    #[error("payment provider key {0:?} isn't `manual` or `spore_<name>`")]
    InvalidProviderKey(String),
    /// No provider with this key in this build (a spore that was removed).
    #[error("no payment provider {0}")]
    UnknownProvider(ProviderKey),
    /// The provider's check of a method's settings failed.
    #[error("settings for payment provider {provider}: {reason}")]
    InvalidSettings {
        provider: ProviderKey,
        reason: String,
    },
    /// The provider couldn't start collecting a payment (it was down, it declined).
    #[error("payment provider {provider} failed: {reason}")]
    ProviderFailed {
        provider: ProviderKey,
        reason: String,
    },
    /// An open payment's method is gone, and its provider (not `manual`) needs the
    /// method's settings to go on collecting it.
    #[error("payment method {0} is gone; its provider can't go on without its settings")]
    MethodUnavailable(PaymentMethodId),
    #[error("instructions are longer than {max} characters", max = PaymentMethod::MAX_INSTRUCTIONS_LEN)]
    InstructionsTooLong,
    /// A move the payment's status doesn't allow (PA7), e.g. paying a voided payment.
    #[error("payment {payment} is {from}; it can't become {to}")]
    InvalidTransition {
        payment: PaymentId,
        from: PaymentStatus,
        to: PaymentStatus,
    },
    #[error("the payment is in {expected}, but an amount is in {found}")]
    CurrencyMismatch { expected: Currency, found: Currency },
    /// Bad stored data: a negative amount, more refunded than paid, a status without its
    /// time.
    #[error("payment {payment}: {reason}")]
    Inconsistent {
        payment: PaymentId,
        reason: &'static str,
    },
}

impl ErrorCode for PaymentError {
    fn code(&self) -> &'static str {
        match self {
            PaymentError::Text(e) => e.code(),
            PaymentError::Money(e) => e.code(),
            PaymentError::InvalidProviderKey(_) => "invalid_payment_provider",
            PaymentError::UnknownProvider(_) => "payment_provider_unknown",
            PaymentError::InvalidSettings { .. } => "invalid_payment_settings",
            PaymentError::ProviderFailed { .. } => "payment_provider_failed",
            PaymentError::MethodUnavailable(_) => "payment_method_unavailable",
            PaymentError::InstructionsTooLong => "instructions_too_long",
            PaymentError::InvalidTransition { .. } => "invalid_payment_transition",
            PaymentError::CurrencyMismatch { .. } => "currency_mismatch",
            PaymentError::Inconsistent { .. } => "payment_inconsistent",
        }
    }
}
