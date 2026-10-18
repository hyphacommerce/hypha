//! The build's templates (design note 0028, NT5, NT6): written in React Email in
//! `packages/emails`, built to Jinja into `defaults/` (committed) and embedded here. A
//! store uses them until it edits a template, so improved defaults reach every store
//! that kept them.

use crate::{TemplateKey, TemplateSources};

/// One key's default sources, as built.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DefaultTemplate {
    /// Empty for the layout.
    pub subject: &'static str,
    pub html: &'static str,
    /// `None` for the layout.
    pub text: Option<&'static str>,
}

impl DefaultTemplate {
    pub fn sources(&self) -> TemplateSources {
        TemplateSources::trusted(self.subject, self.html, self.text)
    }
}

/// `defaults/<key>.subject.j2`, `.html.j2` and `.txt.j2`. A subject file's final newline
/// is harmless: a rendered subject is made one line.
macro_rules! message {
    ($key:literal) => {
        DefaultTemplate {
            subject: include_str!(concat!("../defaults/", $key, ".subject.j2")),
            html: include_str!(concat!("../defaults/", $key, ".html.j2")),
            text: Some(include_str!(concat!("../defaults/", $key, ".txt.j2"))),
        }
    };
}

/// The default sources of `key`.
pub fn default_template(key: TemplateKey) -> DefaultTemplate {
    match key {
        TemplateKey::Layout => DefaultTemplate {
            subject: "",
            html: include_str!("../defaults/layout.html.j2"),
            text: None,
        },
        TemplateKey::OrderConfirmation => message!("order_confirmation"),
        TemplateKey::OrderShipped => message!("order_shipped"),
        TemplateKey::OrderTrackingUpdated => message!("order_tracking_updated"),
        TemplateKey::OrderCancelled => message!("order_cancelled"),
        TemplateKey::StaffNewOrder => message!("staff_new_order"),
        TemplateKey::StaffInvite => message!("staff_invite"),
        TemplateKey::StaffPasswordReset => message!("staff_password_reset"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_default_keeps_the_rules_a_saved_template_must() {
        for key in TemplateKey::ALL {
            let d = default_template(key);
            let checked = TemplateSources::new(
                key,
                d.subject.to_string(),
                d.html.to_string(),
                d.text.map(str::to_string),
            );
            assert!(checked.is_ok(), "{key}: {checked:?}");
        }
    }

    #[test]
    fn every_message_extends_the_layout() {
        for key in TemplateKey::ALL.into_iter().filter(|k| !k.is_layout()) {
            assert!(
                default_template(key)
                    .html
                    .trim_start()
                    .starts_with(r#"{% extends "layout" %}"#),
                "{key}"
            );
        }
    }
}
