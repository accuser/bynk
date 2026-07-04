//! Shelling the resolved `bynkc` — shared by the commands that delegate to it.
//!
//! `bynk test` always delegates (it orchestrates external `tsc`/`node` anyway,
//! and delegating through the driver's resolution is the #487/#486 win); `bynk
//! check`/`fmt` delegate only under a `BYNK_BYNKC` override, so a
//! developer-pinned compiler still governs the result. All three inherit stdio
//! and propagate the child's exit code through [`exit_byte`].

use std::ffi::OsStr;
use std::path::Path;
use std::process::{Command, ExitCode};

/// Map a child exit code to a process exit byte. A `None` code means the child
/// was terminated by a signal (e.g. the Ctrl-C the terminal also delivered to
/// us) — treat that as a clean stop rather than a driver failure.
pub fn exit_byte(code: Option<i32>) -> u8 {
    code.unwrap_or(0).clamp(0, 255) as u8
}

/// Shell `bynkc <args>` at `bynkc`, inheriting stdio, and return its exit code.
/// A spawn failure (a missing or unexecutable binary) is surfaced as a driver
/// error naming the path, so a bad `BYNK_BYNKC` override is diagnosable.
pub fn delegate<I, S>(bynkc: &Path, args: I) -> ExitCode
where
    I: IntoIterator<Item = S>,
    S: AsRef<OsStr>,
{
    let mut cmd = Command::new(bynkc);
    cmd.args(args);
    match cmd.status() {
        Ok(s) => ExitCode::from(exit_byte(s.code())),
        Err(e) => {
            eprintln!("bynk: could not run bynkc ({}): {e}", bynkc.display());
            ExitCode::FAILURE
        }
    }
}
