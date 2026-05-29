//! Interactive confirmation prompts + TTY detection (INST-4 picker,
//! INST-5 typed-`NUKE` confirm).

use std::io::{self, BufRead, IsTerminal, Write};

use crate::profile::Profile;

/// Whether stdin is a terminal. Scripted / piped runs are not, and
/// must supply `--profile` + `--yes` rather than being prompted.
#[must_use]
pub fn stdin_is_tty() -> bool {
    io::stdin().is_terminal()
}

/// Prompt for a literal string (e.g. `NUKE`) and return whether the
/// operator typed it exactly. Reads one line from `reader`; the public
/// [`require_typed`] wraps stdin.
///
/// # Errors
/// Propagates IO errors from reading the line / writing the prompt.
pub fn require_typed_from<R: BufRead, W: Write>(
    reader: &mut R,
    writer: &mut W,
    expected: &str,
    prompt: &str,
) -> io::Result<bool> {
    write!(writer, "{prompt}")?;
    writer.flush()?;
    let mut line = String::new();
    let read = reader.read_line(&mut line)?;
    if read == 0 {
        // EOF (e.g. closed stdin) counts as "not confirmed".
        return Ok(false);
    }
    Ok(line.trim() == expected)
}

/// Prompt stdin for the literal `expected` word.
///
/// # Errors
/// Propagates IO errors.
pub fn require_typed(expected: &str, prompt: &str) -> io::Result<bool> {
    let stdin = io::stdin();
    let mut locked = stdin.lock();
    let mut out = io::stdout();
    require_typed_from(&mut locked, &mut out, expected, prompt)
}

/// Render the profile menu to `writer`, read a 1/2/3 (or Enter for the
/// default) choice from `reader`, and return the selected [`Profile`].
///
/// `default` is used on a bare Enter; when `None`, Enter re-prompts.
///
/// # Errors
/// Propagates IO errors.
pub fn pick_profile_from<R: BufRead, W: Write>(
    reader: &mut R,
    writer: &mut W,
    default: Option<Profile>,
) -> io::Result<Profile> {
    let choices = Profile::all();
    loop {
        writeln!(writer, "Select an install profile:")?;
        for (i, p) in choices.iter().enumerate() {
            let marker = if Some(*p) == default { " (default)" } else { "" };
            writeln!(writer, "  [{}] {}{} — {}", i + 1, p, marker, p.describe())?;
        }
        match default {
            Some(d) => write!(writer, "Profile [1/2/3, Enter={d}]: ")?,
            None => write!(writer, "Profile [1/2/3]: ")?,
        }
        writer.flush()?;

        let mut line = String::new();
        if reader.read_line(&mut line)? == 0 {
            // EOF without a choice and no default: cannot proceed.
            return Err(io::Error::new(
                io::ErrorKind::UnexpectedEof,
                "no profile selected and stdin closed",
            ));
        }
        let trimmed = line.trim();
        if trimmed.is_empty() {
            if let Some(d) = default {
                return Ok(d);
            }
            writeln!(writer, "  no default — please type 1, 2, or 3.")?;
            continue;
        }
        match trimmed {
            "1" => return Ok(Profile::Lighthouse),
            "2" => return Ok(Profile::Headless),
            "3" => return Ok(Profile::Full),
            other => writeln!(writer, "  not a choice: {other:?} — type 1, 2, or 3.")?,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    #[test]
    fn typed_match_succeeds() {
        let mut input = Cursor::new(b"NUKE\n".to_vec());
        let mut out = Vec::new();
        let ok = require_typed_from(&mut input, &mut out, "NUKE", "type NUKE: ").unwrap();
        assert!(ok);
    }

    #[test]
    fn typed_mismatch_fails() {
        let mut input = Cursor::new(b"nuke\n".to_vec());
        let mut out = Vec::new();
        let ok = require_typed_from(&mut input, &mut out, "NUKE", "p").unwrap();
        assert!(!ok);
    }

    #[test]
    fn typed_eof_is_not_confirmed() {
        let mut input = Cursor::new(Vec::new());
        let mut out = Vec::new();
        let ok = require_typed_from(&mut input, &mut out, "NUKE", "p").unwrap();
        assert!(!ok);
    }

    #[test]
    fn picker_numeric_choice() {
        let mut input = Cursor::new(b"2\n".to_vec());
        let mut out = Vec::new();
        let p = pick_profile_from(&mut input, &mut out, None).unwrap();
        assert_eq!(p, Profile::Headless);
    }

    #[test]
    fn picker_enter_takes_default() {
        let mut input = Cursor::new(b"\n".to_vec());
        let mut out = Vec::new();
        let p = pick_profile_from(&mut input, &mut out, Some(Profile::Full)).unwrap();
        assert_eq!(p, Profile::Full);
    }

    #[test]
    fn picker_reprompts_then_accepts() {
        let mut input = Cursor::new(b"x\n1\n".to_vec());
        let mut out = Vec::new();
        let p = pick_profile_from(&mut input, &mut out, None).unwrap();
        assert_eq!(p, Profile::Lighthouse);
    }
}
