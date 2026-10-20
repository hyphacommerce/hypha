use hypha_core::{AppKey, Currency, ErrorCode, MoneyError, PaymentId, PaymentMethodId, TextError};
use thiserror::Error;

use crate::{PaymentAction, PaymentMethod, PaymentStatus, ProviderKey};

/// Why a payment method or a payment can't be built or changed, or a provider refused.
#[derive(Debug, Clone, Error, PartialEq, Eq)]
pub enum PaymentError {
    #[error(transparent)]
    Text(#[from] TextError),
    #[error(transparent)]
    Money(#[from] MoneyError),
    /// Not `manual` or `app_<name>` (lowercase ASCII letters, digits and `_`).
    #[error("payment provider key {0:?} isn't `manual` or `app_<name>`")]
    InvalidProviderKey(String),
    /// No such provider: not one this build has, nor an installed, enabled app that
    /// implements `payments.start`.
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
    /// The provider's app gave no usable answer at an extension point (design note
    /// 0029, §3: `app_unavailable`, with the app and the point); why is in its call log.
    #[error("{app} is unavailable for {point}: {reason}")]
    AppUnavailable {
        app: AppKey,
        point: &'static str,
        reason: String,
    },
    /// The provider can't capture, void or refund (`manual`: staff record what
    /// happened instead).
    #[error("payment provider {provider} can't {action} payments")]
    ActionUnsupported {
        provider: ProviderKey,
        action: PaymentAction,
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
    /// A provider's report about another attempt than the payment's (the report names its
    /// attempt; design note 0030): a second attempt's money for a payment past pending
    /// (paid in an old tab: the payment collects once, so the provider gives that money
    /// back), or another attempt's decline, cancel or refund (nothing to give back).
    #[error("payment {payment} is {status} through {held:?}, not {reported:?}")]
    ReferenceMismatch {
        payment: PaymentId,
        status: PaymentStatus,
        held: String,
        reported: String,
    },
    #[error("the payment is in {expected}, but an amount is in {found}")]
    CurrencyMismatch { expected: Currency, found: Currency },
    /// A refund of nothing, of more than is left to give back, or one that doesn't
    /// leave the payment in the status it was reported as (`refunded` must give back
    /// everything left, `partially_refunded` less).
    #[error(
        "payment {payment}: a refund of {amount} doesn't fit (paid {paid}, refunded {refunded} so far, reported {reported})"
    )]
    InvalidRefund {
        payment: PaymentId,
        amount: i64,
        paid: i64,
        refunded: i64,
        reported: PaymentStatus,
    },
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
            PaymentError::AppUnavailable { .. } => "app_unavailable",
            PaymentError::ActionUnsupported { .. } => "payment_action_unsupported",
            PaymentError::MethodUnavailable(_) => "payment_method_unavailable",
            PaymentError::InstructionsTooLong => "instructions_too_long",
            PaymentError::InvalidTransition { .. } => "invalid_payment_transition",
            PaymentError::ReferenceMismatch { .. } => "payment_reference_mismatch",
            PaymentError::CurrencyMismatch { .. } => "currency_mismatch",
            PaymentError::InvalidRefund { .. } => "invalid_refund_amount",
            PaymentError::Inconsistent { .. } => "payment_inconsistent",
        }
    }
}
