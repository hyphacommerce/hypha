//! Error codes are a contract with clients: released ones are never renamed. Each of
//! core's codes is pinned here, so renaming one fails a test instead of a client.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::cast_possible_truncation,
    clippy::cast_possible_wrap,
    clippy::cast_sign_loss,
    reason = "a failing test panics, and fixtures cast small counters"
)]

use hypha_core::{
    AddressError, AppKey, AppKeyError, AuthError, CallbackUrlError, CountryError, Currency,
    CurrencyError, DescriptionError, EmailError, ErrorCode, HandleError, IdError, IdempotencyError,
    LanguageError, MoneyError, NotifyError, OperationIdError, PageError, QuantityError, RepoError,
    Scope, TextError, TimezoneError,
};

#[test]
fn cores_error_codes_keep_their_names() {
    let spore = AppKey::new("spore_stripe").unwrap();
    let codes: Vec<(&str, &str)> = vec![
        (
            "invalid_limit",
            code(PageError::LimitOutOfRange { max: 100 }),
        ),
        (
            "invalid_idempotency_key",
            code(IdempotencyError::InvalidKey),
        ),
        ("idempotency_key_reused", code(IdempotencyError::KeyReused)),
        (
            "description_too_long",
            code(DescriptionError::TooLong { max: 1 }),
        ),
        ("notification_failed", code(NotifyError(String::new()))),
        ("storage_error", code(RepoError::Backend(String::new()))),
        ("concurrent_modification", code(RepoError::StaleVersion)),
        ("conflict", code(RepoError::Duplicate("key"))),
        ("conflict", code(RepoError::Reference("key"))),
        (
            "invalid_callback_url",
            code(CallbackUrlError(String::new())),
        ),
        ("invalid_address", code(AddressError::Missing("line1"))),
        ("invalid_address", code(AddressError::TooLong("line1"))),
        (
            "unknown_country_code",
            code(AddressError::Country(CountryError::Unknown(
                "XX".to_string(),
            ))),
        ),
        ("invalid_quantity", code(QuantityError::Zero)),
        ("quantity_overflow", code(QuantityError::Overflow)),
        ("invalid_handle", code(HandleError::Empty)),
        ("invalid_handle", code(HandleError::InvalidChar(' '))),
        ("invalid_handle", code(HandleError::EdgeDash)),
        ("invalid_handle", code(HandleError::DoubleDash)),
        (
            "unknown_timezone",
            code(TimezoneError::Unknown(String::new())),
        ),
        (
            "invalid_operation_id",
            code(OperationIdError(String::new())),
        ),
        ("invalid_language", code(LanguageError::TooLong(99))),
        (
            "invalid_language",
            code(LanguageError::Invalid(String::new())),
        ),
        (
            "missing_scope",
            code(AuthError::MissingScope(Scope::ProductsRead)),
        ),
        (
            "app_not_authorized",
            code(AuthError::AppNotAuthorized(Scope::ProductsRead)),
        ),
        ("invalid_country_code", code(CountryError::InvalidLength(3))),
        ("invalid_country_code", code(CountryError::NotUppercase)),
        (
            "unknown_country_code",
            code(CountryError::Unknown(String::new())),
        ),
        ("invalid_app_key", code(AppKeyError::Invalid(String::new()))),
        (
            "invalid_app_key",
            code(AppKeyError::InvalidApp(String::new())),
        ),
        ("spore_key_reserved", code(AppKeyError::SporeKey(spore))),
        ("invalid_email", code(EmailError::Invalid(String::new()))),
        (
            "invalid_id",
            code(IdError::WrongPrefix { expected: "prod" }),
        ),
        ("invalid_id", code(IdError::InvalidUuid)),
        (
            "currency_mismatch",
            code(MoneyError::CurrencyMismatch(Currency::EUR, Currency::USD)),
        ),
        ("amount_overflow", code(MoneyError::Overflow)),
        ("negative_amount", code(MoneyError::Negative)),
        (
            "invalid_basis_points",
            code(MoneyError::InvalidBasisPoints(-10_001)),
        ),
        (
            "invalid_tax_rate",
            code(MoneyError::InvalidTaxRate(1_000_001)),
        ),
        ("field_required", code(TextError::Missing("name"))),
        ("field_too_long", code(TextError::TooLong("name"))),
        (
            "unknown_currency",
            code(CurrencyError::Unknown(String::new())),
        ),
    ];
    for (expected, code) in codes {
        assert_eq!(code, expected);
    }
}

fn code(error: impl ErrorCode) -> &'static str {
    error.code()
}
