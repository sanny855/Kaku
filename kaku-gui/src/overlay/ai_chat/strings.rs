//! User-visible strings for the Cmd+L AI overlay.
//!
//! Centralized so a brand / wording / locale change is a single-file edit
//! instead of a `grep` across `mod.rs`. This is *not* an i18n framework; it
//! is just a one-stop shop. Keep entries narrow (labels, headers, toast
//! titles); long-form templates that interpolate values still live next to
//! their `format!` call sites.

/// Label printed at the top of a user-authored message.
///
/// Matches what `cmd_export` writes as `User:` on disk; the overlay prefers
/// the shorter "You" because horizontal space is tight.
pub(crate) const HEADER_USER: &str = "  You";

/// Label printed at the top of an assistant-authored message.
pub(crate) const HEADER_ASSISTANT: &str = "  AI";

/// Title shown by the system notification when an approval is required and
/// the Kaku window is unfocused.
pub(crate) const APPROVAL_NOTIFICATION_TITLE: &str = "Kaku AI 需要确认";
