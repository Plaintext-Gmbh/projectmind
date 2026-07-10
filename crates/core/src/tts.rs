// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! OS text-to-speech for the presenter narrator (Cockpit 2.6, #162).
//!
//! The presenter's `n` key toggles a narrator that reads the current step's
//! text aloud through whatever speech synthesiser the OS already ships:
//!
//! - macOS: the built-in `say` command.
//! - Linux: `espeak` (or `espeak-ng`, which installs an `espeak` shim).
//!
//! There is intentionally **no bundled speech engine** — that would be a huge
//! dependency for a niche feature. When the OS binary is missing the narrator
//! is a silent no-op that reports [`SpeakOutcome::Unavailable`] so the GUI can
//! show a one-line hint instead of failing.
//!
//! # LLM-generated narration
//!
//! The spec also calls for an *optional* LLM-generated narration path (the MCP
//! client supplies richer prose than the step's authored text). That is a
//! frontend concern: the presenter simply speaks whatever narration string it
//! is handed, so an MCP client that has pre-generated narration just passes
//! that text in instead of the authored `step.narration`. This module stays
//! agnostic about where the words came from — see the PR for the documented
//! wiring.

use std::process::Command;

/// Which TTS backend a platform uses, resolved by [`backend`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Backend {
    /// macOS `say`.
    Say,
    /// Linux `espeak` / `espeak-ng`.
    Espeak,
    /// No known backend for this platform.
    None,
}

impl Backend {
    /// The executable name to spawn, or `None` for [`Backend::None`].
    #[must_use]
    pub fn program(self) -> Option<&'static str> {
        match self {
            Backend::Say => Some("say"),
            Backend::Espeak => Some("espeak"),
            Backend::None => None,
        }
    }
}

/// The TTS backend for the current platform (compile-time resolved).
#[must_use]
pub fn backend() -> Backend {
    if cfg!(target_os = "macos") {
        Backend::Say
    } else if cfg!(target_os = "linux") {
        Backend::Espeak
    } else {
        Backend::None
    }
}

/// Result of a [`speak`] request.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SpeakOutcome {
    /// The synthesiser was spawned for the given non-empty text.
    Spoken,
    /// Nothing to say (empty/whitespace text) — a benign no-op.
    Empty,
    /// No TTS binary is available on this system. Carries a human hint
    /// naming the missing program so the GUI can surface it once.
    Unavailable(String),
}

/// Speak `text` aloud through the OS synthesiser, returning immediately.
///
/// The child process is spawned detached (fire-and-forget) so toggling the
/// narrator never blocks the UI thread. Empty text is a no-op; a missing
/// binary yields [`SpeakOutcome::Unavailable`] rather than an error.
///
/// # Errors
///
/// Returns an error only when the backend exists but the OS refuses to spawn
/// it for a reason other than "not found" (e.g. a permission problem).
pub fn speak(text: &str) -> std::io::Result<SpeakOutcome> {
    if text.trim().is_empty() {
        return Ok(SpeakOutcome::Empty);
    }
    let Some(program) = backend().program() else {
        return Ok(SpeakOutcome::Unavailable(
            "no OS speech engine on this platform".to_string(),
        ));
    };
    speak_with(program, &speak_args(program, text))
}

/// Build the argument vector for `program` speaking `text`. Kept pure and
/// separate from spawning so it is unit-testable.
#[must_use]
pub fn speak_args(program: &str, text: &str) -> Vec<String> {
    match program {
        // espeak reads the phrase from a trailing positional arg. `--` guards
        // against a phrase that starts with a dash being read as a flag.
        "espeak" => vec!["--".to_string(), text.to_string()],
        // `say` also takes the phrase positionally; no flag needed.
        _ => vec![text.to_string()],
    }
}

/// Spawn `program` with `args`, mapping a missing binary to
/// [`SpeakOutcome::Unavailable`] instead of an error.
fn speak_with(program: &str, args: &[String]) -> std::io::Result<SpeakOutcome> {
    use std::process::Stdio;
    match Command::new(program)
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
    {
        Ok(_child) => Ok(SpeakOutcome::Spoken),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(SpeakOutcome::Unavailable(
            format!("`{program}` not found — install it for the presenter narrator"),
        )),
        Err(e) => Err(e),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_text_is_a_noop() {
        assert_eq!(speak("").unwrap(), SpeakOutcome::Empty);
        assert_eq!(speak("   \n\t ").unwrap(), SpeakOutcome::Empty);
    }

    #[test]
    fn backend_matches_platform() {
        let b = backend();
        if cfg!(target_os = "macos") {
            assert_eq!(b, Backend::Say);
            assert_eq!(b.program(), Some("say"));
        } else if cfg!(target_os = "linux") {
            assert_eq!(b, Backend::Espeak);
            assert_eq!(b.program(), Some("espeak"));
        } else {
            assert_eq!(b, Backend::None);
            assert_eq!(b.program(), None);
        }
    }

    #[test]
    fn espeak_args_guard_leading_dash() {
        let args = speak_args("espeak", "-danger");
        assert_eq!(args, vec!["--".to_string(), "-danger".to_string()]);
    }

    #[test]
    fn say_args_are_positional() {
        assert_eq!(speak_args("say", "hello"), vec!["hello".to_string()]);
    }

    #[test]
    fn missing_binary_reports_unavailable() {
        // A program that certainly does not exist maps to Unavailable, never
        // an Err — the narrator degrades to a no-op with a hint.
        let out = speak_with("projectmind-no-such-tts-binary", &["hi".to_string()]).unwrap();
        match out {
            SpeakOutcome::Unavailable(msg) => assert!(msg.contains("not found")),
            other => panic!("expected Unavailable, got {other:?}"),
        }
    }
}
