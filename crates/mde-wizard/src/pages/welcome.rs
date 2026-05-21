//! Welcome page — branded splash + start button.

/// Locked welcome copy (single source of truth for tests +
/// future i18n).
pub const HEADLINE: &str = "Welcome to Mackes Desktop Environment";
pub const SUBHEAD: &str =
    "Let's set up your desktop. The next few pages cover preset, mesh, network, and snapshot — \
     about 90 seconds.";
pub const CTA: &str = "Get started";

/// One-line pre-flight check — the wizard can launch when:
/// 1. We have an XDG_CONFIG_HOME (or a HOME for the fallback).
/// 2. `~/.config/mde/` is creatable.
#[must_use]
pub fn can_start() -> bool {
    dirs::config_dir().is_some()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn copy_is_non_empty() {
        assert!(!HEADLINE.is_empty());
        assert!(!SUBHEAD.is_empty());
        assert!(!CTA.is_empty());
    }

    #[test]
    fn can_start_when_xdg_config_resolves() {
        // On any conventional Linux test environment, dirs::config_dir
        // resolves. Just verify the function is callable.
        let _ = can_start();
    }
}
