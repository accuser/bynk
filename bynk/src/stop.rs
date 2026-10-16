//! Stopping `bynk dev` from outside (#1955).
//!
//! Without a handler, a SIGTERM or SIGINT sent to `bynk dev` alone (by a CI
//! step, a test harness, an editor task) killed it on the spot, and its
//! wranglers and their `workerd`s ran on with their ports bound. A Ctrl-C in a
//! terminal never showed it, because the terminal signals the whole foreground
//! group, wranglers included.
//!
//! [`install`] replaces the default action with one that only records the
//! signal. `bynk dev`'s watch loop sees it on its next tick and stops its
//! contexts the way it does when one of them exits: SIGTERM to each wrangler,
//! then the sweep of what they leave behind ([`crate::sweep`]). The wranglers
//! stay in `bynk dev`'s process group, because the sweep finds orphaned
//! `workerd`s by that group and a Ctrl-C must still reach them all.
//!
//! The handler is one-shot (`SA_RESETHAND`): a second Ctrl-C while the
//! contexts are stopping takes the default action and ends `bynk dev` at once.
//!
//! Unix only. On Windows each wrangler's job object holds its tree (#1762), and
//! [`install`] does nothing.

use std::sync::atomic::{AtomicI32, Ordering};

/// The signal received since [`install`], or 0 for none.
static RECEIVED: AtomicI32 = AtomicI32::new(0);

/// Record SIGINT and SIGTERM instead of dying of them, for [`received`] to
/// report. Children spawned afterwards are unaffected: `exec` resets a caught
/// signal to its default action.
pub fn install() {
    #[cfg(unix)]
    for sig in [libc::SIGINT, libc::SIGTERM] {
        // SAFETY: `record` only stores to an atomic, which is
        // async-signal-safe, and `action` is fully initialised before use.
        unsafe {
            let mut action: libc::sigaction = std::mem::zeroed();
            action.sa_sigaction = record as extern "C" fn(libc::c_int) as libc::sighandler_t;
            action.sa_flags = libc::SA_RESTART | libc::SA_RESETHAND;
            libc::sigemptyset(&mut action.sa_mask);
            if libc::sigaction(sig, &action, std::ptr::null_mut()) != 0 {
                eprintln!(
                    "bynk dev: could not install a handler for signal {sig}; stopping \
                     `bynk dev` with it may leave its workerd processes running."
                );
            }
        }
    }
}

#[cfg(unix)]
extern "C" fn record(sig: libc::c_int) {
    RECEIVED.store(sig, Ordering::SeqCst);
}

/// The signal that asked `bynk dev` to stop, if one has arrived.
pub fn received() -> Option<i32> {
    match RECEIVED.load(Ordering::SeqCst) {
        0 => None,
        sig => Some(sig),
    }
}

/// Undo [`install`] and forget any recorded signal, so a test that installs
/// the handler leaves nothing behind in the shared test process.
#[cfg(test)]
pub(crate) fn reset() {
    #[cfg(unix)]
    for sig in [libc::SIGINT, libc::SIGTERM] {
        // SAFETY: restoring the default disposition.
        unsafe {
            libc::signal(sig, libc::SIG_DFL);
        }
    }
    RECEIVED.store(0, Ordering::SeqCst);
}

/// The exit byte for a session stopped by `sig`, as [`crate::shell::exit_status_byte`]
/// maps a child's signal death: SIGINT is a clean stop (it is what Ctrl-C
/// sends), anything else `128 + sig`, the shell convention.
pub fn exit_byte(sig: i32) -> u8 {
    // SIGINT is 2 on every Unix.
    if sig == 2 {
        0
    } else {
        128u8.saturating_add(sig.clamp(0, 127) as u8)
    }
}

/// How to name `sig` in `bynk dev`'s stop notice.
pub fn name(sig: i32) -> String {
    match sig {
        2 => "SIGINT".to_string(),
        15 => "SIGTERM".to_string(),
        n => format!("signal {n}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_signal_stop_exits_like_a_signalled_child() {
        assert_eq!(exit_byte(2), 0, "Ctrl-C is a clean stop");
        assert_eq!(exit_byte(15), 143);
    }
}
