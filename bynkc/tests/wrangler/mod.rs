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
