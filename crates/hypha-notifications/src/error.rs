use std::fmt;

use hypha_core::{EmailError, ErrorCode, NotificationId, TextError};
use thiserror::Error;

use crate::{NotificationStatus, TemplateKey};

/// Which source of a template a problem is in. `Layout` is the shared frame a message's
/// HTML extends, when rendering finds the problem there.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum TemplatePart {
    Subject,
    Html,
    Text,
    Layout,
}

impl TemplatePart {
    /// The same string used in JSON (`details.part`) and as the template's name inside
    /// the renderer, so minijinja's errors name it too.
    pub fn as_str(self) -> &'static str {
        match self {
            TemplatePart::Subject => "subject",
            TemplatePart::Html => "html",
            TemplatePart::Text => "text",
            TemplatePart::Layout => "layout",
        }
    }
}

impl fmt::Display for TemplatePart {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// What minijinja reported about a source: where (the part, and the line when it knows
/// one, counted from 1) and what, in its words. Clients show the line in the editor
/// (`details.line`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TemplateProblem {
    pub part: TemplatePart,
    pub line: Option<usize>,
    pub message: String,
}

impl fmt::Display for TemplateProblem {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self.line {
            Some(line) => write!(f, "{} line {line}: {}", self.part, self.message),
            None => write!(f, "{}: {}", self.part, self.message),
        }
    }
}

/// Why a template can't be built or changed.
#[derive(Debug, Clone, Error, PartialEq, Eq)]
pub enum TemplateError {
    /// A source isn't valid Jinja.
    #[error("the template doesn't parse: {0}")]
    Invalid(TemplateProblem),
    /// Bytes, not characters: what storage and the provider pay for.
    #[error("the {part} is longer than {max} bytes")]
    TooLarge { part: TemplatePart, max: usize },
    /// A message needs a subject and an HTML body.
    #[error("the {0} is required")]
    Missing(TemplatePart),
    /// The layout is a frame: only its HTML is used.
    #[error("the layout has no {0}; only its HTML is used")]
    NotInLayout(TemplatePart),
    /// Turning off a template that can't be (`TemplateKey::can_disable`).
    #[error("the {0} template can't be disabled")]
    Required(TemplateKey),
    /// No template has this name.
    #[error("no template {0:?}")]
    UnknownKey(String),
}

impl ErrorCode for TemplateError {
    fn code(&self) -> &'static str {
        match self {
            TemplateError::Invalid(_) => "template_invalid",
            TemplateError::TooLarge { .. } => "template_too_large",
            TemplateError::Missing(_) => "template_part_required",
            TemplateError::NotInLayout(_) => "template_part_not_allowed",
            TemplateError::Required(_) => "template_required",
            TemplateError::UnknownKey(_) => "template_unknown",
        }
    }
}

/// Why a template didn't render (design note 0028, §2): what a preview shows and what a
/// save is refused with when the template fails against its sample.
#[derive(Debug, Clone, Error, PartialEq, Eq)]
pub enum RenderError {
    /// A syntax error, a variable the context lacks (undefined is strict), a filter
    /// given the wrong value.
    #[error("the template doesn't render: {0}")]
    Invalid(TemplateProblem),
    /// It ran out of fuel: a loop that would run too long. Each render gets
    /// `render::FUEL`.
    #[error("the {0} takes too long to render")]
    TooComplex(TemplatePart),
}

impl RenderError {
    /// The problem with its line, for `details`; `None` for `TooComplex`.
    pub fn problem(&self) -> Option<&TemplateProblem> {
        match self {
            RenderError::Invalid(problem) => Some(problem),
            RenderError::TooComplex(_) => None,
        }
    }
}

impl ErrorCode for RenderError {
    fn code(&self) -> &'static str {
        match self {
            RenderError::Invalid(_) => "template_invalid",
            RenderError::TooComplex(_) => "template_too_complex",
        }
    }
}

/// Why the notification settings can't be built or changed.
#[derive(Debug, Clone, Error, PartialEq, Eq)]
pub enum SettingsError {
    #[error(transparent)]
    Text(#[from] TextError),
    #[error(transparent)]
    Email(#[from] EmailError),
    #[error("{0:?} isn't a colour like #1a2b3c")]
    InvalidColour(String),
    /// Not an http(s) URL with `{order}` in it (and only `{order}` and `{token}` as
    /// placeholders).
    #[error("{0:?} isn't an http(s) URL with {{order}} in it")]
    InvalidOrderUrl(String),
    #[error("at most {max} staff recipients")]
    TooManyRecipients { max: usize },
    #[error("the footer is longer than {max} characters")]
    FooterTooLong { max: usize },
}

impl ErrorCode for SettingsError {
    fn code(&self) -> &'static str {
        match self {
            SettingsError::Text(e) => e.code(),
            SettingsError::Email(e) => e.code(),
            SettingsError::InvalidColour(_) => "invalid_colour",
            SettingsError::InvalidOrderUrl(_) => "invalid_order_url",
            SettingsError::TooManyRecipients { .. } => "too_many_recipients",
            SettingsError::FooterTooLong { .. } => "footer_too_long",
        }
    }
}

/// Why a message's record can't be built or moved.
#[derive(Debug, Clone, Error, PartialEq, Eq)]
pub enum NotificationError {
    #[error(transparent)]
    Text(#[from] TextError),
    /// A move its status doesn't allow, e.g. sending a skipped message.
    #[error("notification {notification} is {from}; it can't become {to}")]
    InvalidTransition {
        notification: NotificationId,
        from: NotificationStatus,
        to: NotificationStatus,
    },
    /// Bad stored data: a sent message without its time, a provider's ID on one that
    /// wasn't sent.
    #[error("notification {notification}: {reason}")]
    Inconsistent {
        notification: NotificationId,
        reason: &'static str,
    },
}

impl ErrorCode for NotificationError {
    fn code(&self) -> &'static str {
        match self {
            NotificationError::Text(e) => e.code(),
            NotificationError::InvalidTransition { .. } => "invalid_notification_transition",
            NotificationError::Inconsistent { .. } => "notification_inconsistent",
        }
    }
}
