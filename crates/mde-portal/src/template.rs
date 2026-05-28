//! HYP-16 (Portal-48 retarget) — template → `hyprctl --batch` emitter.
//!
//! A workspace template (a [`TemplateSpec`]: one workspace id + an
//! ordered list of app launch commands) is applied as a single
//! Hyprland `--batch` dispatch so the whole layout lands in one
//! compositor pass with no intermediate flicker.
//!
//! This module owns the pure serialization half — [`batch_payload`]
//! turns a `TemplateSpec` into the argument string that gets handed
//! to `hyprctl --batch "<payload>"`. The actual one-shot IPC call
//! (the shell-out, bench-gated per HYP-16) is the caller's job; the
//! payload string is the testable contract.
//!
//! Hyprland's `--batch` separates dispatch commands with ` ; `. The
//! first command switches to (creating if absent) the target
//! workspace, then one `dispatch exec` per app launches it there.

use mde_card::TemplateSpec;

/// Build the `hyprctl --batch` payload that applies `spec`:
///
/// ```text
/// dispatch workspace <id> ; dispatch exec <app1> ; dispatch exec <app2> ; …
/// ```
///
/// Returns just the `dispatch workspace …` switch when the template
/// has no apps (so applying an empty template still focuses the
/// workspace rather than emitting an empty/no-op batch).
#[must_use]
pub fn batch_payload(spec: &TemplateSpec) -> String {
    let mut cmds: Vec<String> = Vec::with_capacity(spec.apps.len() + 1);
    cmds.push(format!("dispatch workspace {}", spec.workspace));
    for app in &spec.apps {
        cmds.push(format!("dispatch exec {app}"));
    }
    cmds.join(" ; ")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn spec(workspace: i32, apps: &[&str]) -> TemplateSpec {
        TemplateSpec {
            workspace,
            apps: apps.iter().map(|s| (*s).to_string()).collect(),
        }
    }

    #[test]
    fn empty_template_emits_workspace_switch_only() {
        assert_eq!(batch_payload(&spec(3, &[])), "dispatch workspace 3");
    }

    #[test]
    fn single_app_appends_one_exec() {
        assert_eq!(
            batch_payload(&spec(3, &["firefox"])),
            "dispatch workspace 3 ; dispatch exec firefox"
        );
    }

    #[test]
    fn multi_app_preserves_order() {
        assert_eq!(
            batch_payload(&spec(5, &["foot", "firefox", "org.mde.voice.hud"])),
            "dispatch workspace 5 ; dispatch exec foot ; \
             dispatch exec firefox ; dispatch exec org.mde.voice.hud"
        );
    }

    #[test]
    fn workspace_switch_is_always_first() {
        let payload = batch_payload(&spec(2, &["kitty"]));
        assert!(payload.starts_with("dispatch workspace 2 ; "));
    }

    #[test]
    fn exec_commands_pass_through_verbatim() {
        // The exec arg is handed to Hyprland as-is; multi-word
        // commands + flags survive (hyprland-rs / hyprctl own any
        // socket-level quoting at call time).
        assert_eq!(
            batch_payload(&spec(1, &["/usr/bin/foo --flag bar"])),
            "dispatch workspace 1 ; dispatch exec /usr/bin/foo --flag bar"
        );
    }
}
