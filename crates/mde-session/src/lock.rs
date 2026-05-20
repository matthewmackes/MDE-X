//! v2.0.0 Phase D.4 — swaylock integration.
//!
//! Runs the configured screen-lock command when
//! `dev.mackes.MDE.Session.Lock` fires. The command is read from the
//! `$MDE_LOCK_CMD` env var with a sensible swaylock default.

/// Default lock command. swaylock --color black is the established
/// minimum (`pam_unix` for the password, no extra fanfare). Operators
/// who want a fancier screensaver set `$MDE_LOCK_CMD` to their own
/// invocation.
pub const DEFAULT_LOCK_CMD: &str = "swaylock --color 000000";

/// Resolve the lock command from `$MDE_LOCK_CMD` (or the legacy
/// `$MACKES_LOCK_CMD` via the Phase 0.6 shim), defaulting to
/// [`DEFAULT_LOCK_CMD`].
#[must_use]
pub fn lock_command_string() -> String {
    mackesd_core::env_with_legacy_fallback("MDE_LOCK_CMD", "MACKES_LOCK_CMD")
        .filter(|s| !s.trim().is_empty())
        .unwrap_or_else(|| DEFAULT_LOCK_CMD.to_owned())
}

/// Spawn the lock command via `sh -c` so the env-var can include
/// shell flags (e.g. `swaylock -f --color 000000`). Returns Err on
/// spawn failure or non-zero exit.
///
/// # Errors
/// Returns whatever the shell + lock command surface.
pub async fn run_lock_command() -> anyhow::Result<()> {
    let cmd = lock_command_string();
    let out = tokio::process::Command::new("sh")
        .args(["-c", &cmd])
        .output()
        .await
        .map_err(|e| anyhow::anyhow!("lock: spawn `sh -c '{cmd}'` failed: {e}"))?;
    if !out.status.success() {
        anyhow::bail!(
            "lock: `{cmd}` exited {}: {}",
            out.status.code().map_or("?".to_string(), |c| c.to_string()),
            String::from_utf8_lossy(&out.stderr).trim()
        );
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Mutex, OnceLock};
    static ENV_LOCK: OnceLock<Mutex<()>> = OnceLock::new();
    fn with_env<R>(key: &str, value: Option<&str>, body: impl FnOnce() -> R) -> R {
        let lock = ENV_LOCK.get_or_init(|| Mutex::new(()));
        let _g = lock.lock().unwrap_or_else(|e| e.into_inner());
        let prev = std::env::var_os(key);
        match value {
            Some(v) => std::env::set_var(key, v),
            None    => std::env::remove_var(key),
        }
        let r = body();
        match prev {
            Some(v) => std::env::set_var(key, v),
            None    => std::env::remove_var(key),
        }
        r
    }

    #[test]
    fn default_lock_cmd_is_swaylock_with_black_bg() {
        assert_eq!(DEFAULT_LOCK_CMD, "swaylock --color 000000");
    }

    #[test]
    fn lock_command_string_returns_default_when_env_unset() {
        with_env("MDE_LOCK_CMD", None, || {
            with_env("MACKES_LOCK_CMD", None, || {
                assert_eq!(lock_command_string(), DEFAULT_LOCK_CMD);
            });
        });
    }

    #[test]
    fn lock_command_string_honors_mde_env_var() {
        with_env("MDE_LOCK_CMD", Some("xtrlock"), || {
            with_env("MACKES_LOCK_CMD", None, || {
                assert_eq!(lock_command_string(), "xtrlock");
            });
        });
    }

    #[test]
    fn lock_command_string_falls_back_to_legacy_macros_env_var() {
        with_env("MDE_LOCK_CMD", None, || {
            with_env("MACKES_LOCK_CMD", Some("i3lock"), || {
                assert_eq!(lock_command_string(), "i3lock");
            });
        });
    }

    #[test]
    fn lock_command_string_treats_whitespace_as_unset() {
        with_env("MDE_LOCK_CMD", Some("   "), || {
            with_env("MACKES_LOCK_CMD", None, || {
                assert_eq!(lock_command_string(), DEFAULT_LOCK_CMD);
            });
        });
    }
}
