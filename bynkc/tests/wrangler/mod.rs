//! The one wrangler package spec the workerd smokes provision, shared so CI's
//! pre-warm step and the tests cannot name different ones.
//!
//! Each workerd smoke runs `npx -y <spec> dev`, and they do it concurrently:
//! `events_boundary_workerd.rs`'s two tests run on two threads under
//! `cargo test`, nextest runs every test as its own process in one pool, and
//! `events_ordering_workerd.rs` starts two workers at once inside a single
//! test. Concurrent `npx -y <spec>` invocations all resolve to the SAME
//! `~/.npm/_npx/<hash>` directory — the hash is of the package spec, not the
//! argv — and npm does not lock it. On a cold cache they race: the v0.245.0
//! release run had npm tear down its own half-extracted tree (`ENOTEMPTY` on
//! rmdir, then a fan of `TAR_ENTRY_ERROR ENOENT`), so no wrangler ever served
//! and both boundary tests burned the full 180s boot window before failing
//! under `BYNK_REQUIRE_WORKERD`.
//!
//! The fix is a CI step that provisions the spec once, serially, before the
//! suite runs — after which every test finds a complete cache and installs
//! nothing. That only works while the step names the spec the tests actually
//! run, which is what `wrangler_prewarm.rs` guards: a bump to `wrangler@5` here
//! that forgets the workflows would silently restore the race, visible only as
//! an intermittent red.

/// The `npx` package spec every workerd smoke provisions.
pub const SPEC: &str = "wrangler@4";

/// A spawned `npx -y wrangler@4 dev`, stopped together with **every process
/// under it** when it drops (#1686).
///
/// Signalling the child alone is not enough, whichever signal is used. The
/// child is `npx`, and the server is four processes further down: `npx` runs
/// `sh -c wrangler`, which runs wrangler's `node` launcher, which runs
/// wrangler's CLI, which runs two `workerd serve`s. Neither SIGKILL
/// (`Child::kill`) nor SIGTERM sent to `npx` reaches them. Both were tried, one
/// per smoke, and both left the whole chain re-parented to init with its ports
/// still bound. Over a day of local runs that was 172 processes, and the next
/// boot failed with `bind(): Address already in use`, which reads as a
/// regression in whatever was under test.
///
/// So the child gets a process group of its own, and the descendants inherit
/// it (wrangler never moves them out). Teardown signals the group. SIGTERM goes
/// first, because wrangler traps it and stops its `workerd`s cleanly. SIGKILL
/// follows only for what is still running after the grace period.
///
/// Off unix there are no process groups, and this falls back to
/// `Child::kill`, as before.
#[allow(dead_code)] // `wrangler_prewarm.rs` takes only `SPEC` from this module.
pub struct Wrangler {
    child: std::process::Child,
    stopped: bool,
}

#[allow(dead_code)]
impl Wrangler {
    /// How long wrangler gets to stop its own `workerd`s after SIGTERM.
    pub const GRACE: std::time::Duration = std::time::Duration::from_secs(10);

    /// Spawn `cmd` as the leader of a new process group.
    pub fn spawn(cmd: &mut std::process::Command) -> std::io::Result<Self> {
        #[cfg(unix)]
        std::os::unix::process::CommandExt::process_group(cmd, 0);
        Ok(Wrangler {
            child: cmd.spawn()?,
            stopped: false,
        })
    }

    /// The spawned child, for reading its piped output. Read a pipe only
    /// after [`Wrangler::stop`]: every process in the group holds the write
    /// end, so a read before then blocks for as long as they run.
    pub fn child(&mut self) -> &mut std::process::Child {
        &mut self.child
    }

    /// Stop the whole group, giving wrangler [`Wrangler::GRACE`] to do it
    /// cleanly. Idempotent; `Drop` calls it.
    pub fn stop(&mut self) {
        self.stop_within(Self::GRACE);
    }

    /// [`Wrangler::stop`] with an explicit grace period, so a test can reach
    /// the SIGKILL escalation without waiting the full ten seconds.
    pub fn stop_within(&mut self, grace: std::time::Duration) {
        if std::mem::replace(&mut self.stopped, true) {
            return;
        }
        #[cfg(unix)]
        {
            // The leader's pid is the group id: `process_group(0)` made it so.
            let group = format!("-{}", self.child.id());
            signal_group("TERM", &group);
            let deadline = std::time::Instant::now() + grace;
            // Reap the leader first: a zombie leader still counts as a member,
            // so the group would never look empty until it is reaped.
            while self.child.try_wait().is_ok_and(|s| s.is_none())
                && std::time::Instant::now() < deadline
            {
                std::thread::sleep(std::time::Duration::from_millis(50));
            }
            if self.child.try_wait().is_ok_and(|s| s.is_some()) {
                while signal_group("0", &group) && std::time::Instant::now() < deadline {
                    std::thread::sleep(std::time::Duration::from_millis(100));
                }
            }
            // A no-op if the group is already empty.
            signal_group("KILL", &group);
        }
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

impl Drop for Wrangler {
    fn drop(&mut self) {
        self.stop();
    }
}

/// Send `signal` to the process group `group` (written `-<pgid>`) with
/// `kill(1)`, since std has no way to signal a group. `true` when some member
/// received it. With signal `0` nothing is sent, so the result says whether
/// the group still has members.
#[cfg(unix)]
fn signal_group(signal: &str, group: &str) -> bool {
    std::process::Command::new("kill")
        // `--` stops `kill` from reading the group's leading `-` as an option.
        .args([&format!("-{signal}"), "--", group])
        .stderr(std::process::Stdio::null())
        .status()
        .is_ok_and(|s| s.success())
}

/// The pids of the live processes whose working directory is `dir` or is
/// under it. Run this before removing `dir`: once it is gone, the kernel
/// reports each such cwd with a ` (deleted)` suffix and they no longer match.
#[cfg(target_os = "linux")]
#[allow(dead_code)]
pub fn processes_under(dir: &std::path::Path) -> Vec<u32> {
    let Ok(entries) = std::fs::read_dir("/proc") else {
        return Vec::new();
    };
    entries
        .filter_map(|e| e.ok()?.file_name().to_str()?.parse::<u32>().ok())
        .filter(|pid| {
            std::fs::read_link(format!("/proc/{pid}/cwd")).is_ok_and(|cwd| cwd.starts_with(dir))
        })
        .collect()
}

#[cfg(all(test, target_os = "linux"))]
mod tests {
    use super::{Wrangler, processes_under};
    use std::process::{Command, Stdio};
    use std::time::Duration;

    /// Run `script` under a `Wrangler` guard in a fresh directory, wait until
    /// it has started its background `sleep`s, then stop it within `grace` and
    /// return what survived. The `sleep`s stand in for wrangler's `workerd`s:
    /// grandchildren the child never signals itself.
    fn survivors(tag: &str, script: &str, grace: Duration) -> Vec<u32> {
        let dir =
            std::env::temp_dir().join(format!("bynk-wrangler-guard-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        // Null stdio: a stranded `sleep` holding the harness's output pipe
        // would hang the run instead of failing the assertion below.
        let mut guard = Wrangler::spawn(
            Command::new("sh")
                .args(["-c", script])
                .current_dir(&dir)
                .stdout(Stdio::null())
                .stderr(Stdio::null()),
        )
        .expect("sh spawns");
        // The `sh` and its two `sleep`s.
        let started = std::time::Instant::now();
        while processes_under(&dir).len() < 3 {
            assert!(
                started.elapsed() < Duration::from_secs(10),
                "the script never started its background processes"
            );
            std::thread::sleep(Duration::from_millis(20));
        }
        guard.stop_within(grace);
        let left = processes_under(&dir);
        let _ = std::fs::remove_dir_all(&dir);
        left
    }

    /// The #1686 regression: before, signalling the child stranded its
    /// grandchildren. They must go with it.
    #[test]
    fn stopping_takes_the_grandchildren_with_it() {
        let left = survivors("term", "sleep 300 & sleep 300 & wait", Wrangler::GRACE);
        assert!(left.is_empty(), "processes survived the guard: {left:?}");
    }

    /// A group that ignores SIGTERM is still stopped, by SIGKILL once the
    /// grace period is over. The `sleep`s inherit the ignored disposition.
    #[test]
    fn a_group_that_ignores_sigterm_is_killed_after_the_grace_period() {
        let left = survivors(
            "kill",
            "trap '' TERM; sleep 300 & sleep 300 & wait",
            Duration::from_millis(300),
        );
        assert!(left.is_empty(), "processes survived SIGKILL: {left:?}");
    }
}
