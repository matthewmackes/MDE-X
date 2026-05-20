//! Phase D.2 — pure-logic core of the MDE logout / restart /
//! shutdown confirmation dialog.
//!
//! The Iced GUI lives in `bin/mde-logout-dialog`. This library
//! holds:
//!
//!   * [`Action`] — the three things the dialog can do.
//!   * [`Choice`] — the cancel/confirm result the GUI returns.
//!   * [`exit_code`] — the integer the binary exits with so the
//!     parent (`mde-session`) can map confirmation back to a
//!     system action.
//!   * [`title`] / [`primary_button_label`] — locked copy. Pulled
//!     from the design spec; locked so the wording doesn't drift
//!     and changes show up in a diff.
//!
//! Keeping the core dep-free means session.rs can `cargo test` it
//! in milliseconds without spinning up Iced + wgpu + the Wayland
//! event loop.

#![warn(missing_docs)]
#![forbid(unsafe_code)]

/// The user-facing action the dialog asks the user to confirm. One
/// of: log out of the current MDE session, restart the machine, or
/// shut the machine down.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Action {
    /// End the current MDE session (signals SIGTERM to mde-session
    /// which then exits, ending the user's Wayland session).
    Logout,
    /// Reboot the machine via `systemctl reboot`.
    Restart,
    /// Power off the machine via `systemctl poweroff`.
    Shutdown,
}

impl Action {
    /// Stable kebab-case identifier — what the binary takes on
    /// `--action` and what scripts grep for.
    #[must_use]
    pub const fn slug(self) -> &'static str {
        match self {
            Self::Logout => "logout",
            Self::Restart => "restart",
            Self::Shutdown => "shutdown",
        }
    }

    /// Parse a slug back to an action. Returns `None` on any
    /// unrecognised input so callers can fall back to printing the
    /// usage banner.
    #[must_use]
    pub fn from_slug(slug: &str) -> Option<Self> {
        match slug {
            "logout" => Some(Self::Logout),
            "restart" => Some(Self::Restart),
            "shutdown" => Some(Self::Shutdown),
            _ => None,
        }
    }
}

/// What the user picked. The binary translates this to an exit
/// code (see [`exit_code`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Choice {
    /// User pressed the primary button (Log out / Restart / Shut
    /// down). The parent should act.
    Confirm,
    /// User pressed Cancel, hit Escape, or closed the window. The
    /// parent should do nothing.
    Cancel,
}

/// Exit code the binary returns. `0` = confirmed (parent acts);
/// `10` = cancelled (parent does nothing). The 10 was picked
/// because it sits well clear of the systemd / shell conventional
/// codes (0, 1, 2, 130 = SIGINT, 143 = SIGTERM, …).
#[must_use]
pub const fn exit_code(choice: Choice) -> i32 {
    match choice {
        Choice::Confirm => 0,
        Choice::Cancel => 10,
    }
}

/// Dialog window title for the given action. Locked copy.
#[must_use]
pub const fn title(action: Action) -> &'static str {
    match action {
        Action::Logout => "Log out of MDE?",
        Action::Restart => "Restart this computer?",
        Action::Shutdown => "Shut down this computer?",
    }
}

/// Body paragraph — explains what'll happen on Confirm. Locked
/// copy that the designer signed off on.
#[must_use]
pub const fn body(action: Action) -> &'static str {
    match action {
        Action::Logout =>
            "Your current MDE session will end and unsaved work will be lost.",
        Action::Restart =>
            "Your MDE session will end and the computer will restart. Unsaved work will be lost.",
        Action::Shutdown =>
            "Your MDE session will end and the computer will turn off. Unsaved work will be lost.",
    }
}

/// Primary button label.
#[must_use]
pub const fn primary_button_label(action: Action) -> &'static str {
    match action {
        Action::Logout => "Log out",
        Action::Restart => "Restart",
        Action::Shutdown => "Shut down",
    }
}

/// Cancel button label — same for every action.
#[must_use]
pub const fn cancel_button_label() -> &'static str {
    "Cancel"
}

/// systemctl subcommand the parent should run on confirm. `None`
/// for Logout (the parent uses SIGTERM-to-mde-session instead).
#[must_use]
pub const fn systemctl_subcommand(action: Action) -> Option<&'static str> {
    match action {
        Action::Logout => None,
        Action::Restart => Some("reboot"),
        Action::Shutdown => Some("poweroff"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn slug_round_trips_for_every_action() {
        for a in [Action::Logout, Action::Restart, Action::Shutdown] {
            assert_eq!(Action::from_slug(a.slug()), Some(a), "round-trip {a:?}");
        }
    }

    #[test]
    fn slug_rejects_garbage() {
        assert!(Action::from_slug("").is_none());
        assert!(Action::from_slug("Logout").is_none());
        assert!(Action::from_slug("shutdown ").is_none());
        assert!(Action::from_slug("garbage").is_none());
    }

    #[test]
    fn exit_codes_are_disjoint() {
        assert_eq!(exit_code(Choice::Confirm), 0);
        assert_eq!(exit_code(Choice::Cancel), 10);
    }

    #[test]
    fn titles_mention_what_they_promise() {
        // Lock-check: title literals must mention the user-facing
        // action so a screen-reader user knows what the dialog is
        // asking before scanning the body.
        assert!(title(Action::Logout).contains("Log out"));
        assert!(title(Action::Restart).contains("Restart"));
        assert!(title(Action::Shutdown).contains("Shut down"));
    }

    #[test]
    fn body_warns_about_unsaved_work_for_every_action() {
        for a in [Action::Logout, Action::Restart, Action::Shutdown] {
            assert!(
                body(a).contains("Unsaved work will be lost")
                    || body(a).contains("unsaved work will be lost"),
                "body({a:?}) must warn about unsaved work"
            );
        }
    }

    #[test]
    fn primary_button_labels_are_specific() {
        // Locked: button label MUST repeat the verb (Apple HIG +
        // GNOME HIG both require this). "OK" is forbidden.
        for a in [Action::Logout, Action::Restart, Action::Shutdown] {
            let l = primary_button_label(a);
            assert!(!l.eq_ignore_ascii_case("ok"), "primary label {l:?} must be specific");
            assert!(!l.is_empty());
        }
    }

    #[test]
    fn cancel_label_is_cancel() {
        // Locked copy — must stay "Cancel" across every action.
        assert_eq!(cancel_button_label(), "Cancel");
    }

    #[test]
    fn systemctl_subcommand_matches_the_action() {
        assert_eq!(systemctl_subcommand(Action::Logout), None);
        assert_eq!(systemctl_subcommand(Action::Restart), Some("reboot"));
        assert_eq!(systemctl_subcommand(Action::Shutdown), Some("poweroff"));
    }
}
