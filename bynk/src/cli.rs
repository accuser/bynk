//! The `bynk` driver command-line interface.
//!
//! The developer front-end: `doctor` / `new` / `dev`, plus the everyday
//! `check` / `fmt` / `test` (v0.138, #487). `check` and `fmt` run the linked
//! pipeline in-process; `test` delegates to the driver-resolved `bynkc`. The
//! flag surfaces mirror `bynkc`'s so the two are drop-in equivalent.

use std::path::PathBuf;

use clap::{Parser, Subcommand, ValueEnum};

use crate::doctor::{Capability, DoctorOptions};
use crate::report::Format;

#[derive(Parser, Debug)]
#[command(name = "bynk", version, about = "The Bynk driver", long_about = None)]
pub struct Cli {
    #[command(subcommand)]
    pub command: Command,
}

#[derive(Subcommand, Debug)]
pub enum Command {
    /// Check whether your machine is ready to compile, test, and deploy Bynk —
    /// and print the exact remedy for anything missing.
    ///
    /// Bare `bynk doctor` is informational: it surveys every capability and
    /// exits 0 unless `bynkc` itself is unusable. `--only <capability>` gates on
    /// one capability (exits non-zero if its tools are missing); `--strict`
    /// turns every warning into a failure, for CI.
    Doctor {
        /// Project directory to inspect (for project-local `node_modules/.bin`
        /// resolution). Defaults to the current directory.
        #[arg(default_value = ".")]
        input: PathBuf,
        /// Scope the check — and the exit code — to one capability.
        #[arg(long, value_enum)]
        only: Option<CapabilityArg>,
        /// Treat every warning (optional gaps, npx provisionability, minor
        /// version skew) as a failure. For an all-green CI gate.
        #[arg(long)]
        strict: bool,
        /// Output format. `human` (default) is a grouped table; `short` and
        /// `json` are the stable scriptable surface.
        #[arg(long, value_enum, default_value = "human")]
        format: FormatArg,
    },
    /// Build the project and serve it locally with `wrangler dev`, rebuilding
    /// on save — one step in place of the manual compile + `cd` + `wrangler
    /// dev` recipe.
    ///
    /// Compiles into a managed `.bynk/dev/` build dir and runs one `wrangler
    /// dev` per context from inside its worker dir, in local mode (Miniflare) —
    /// no namespace provisioning needed. Every context is served by default and
    /// the service bindings between them are wired (#552), so a cross-context
    /// call resolves locally; `--context` narrows to a subset. While serving,
    /// `.bynk` sources are watched (#524): saving a file rebuilds in place and
    /// the running workers hot-reload; a failing rebuild reports errors and
    /// keeps serving the last good build. Everything after `--` is forwarded to
    /// `wrangler dev` verbatim.
    Dev {
        /// Project directory to serve from (anywhere inside the project; the
        /// root is found by walking up for `bynk.toml`). Defaults to `.`.
        #[arg(default_value = ".")]
        path: PathBuf,
        /// Which context to serve, repeatable. Omit to serve every context in
        /// the project with the service bindings between them wired (#552);
        /// pass one or more to narrow to a subset. Accepts the dotted name or
        /// its dasherised worker-dir form.
        #[arg(long = "context", value_name = "NAME")]
        contexts: Vec<String>,
        /// First port of the per-context allocation (context *i* gets
        /// `--base-port` + *i*, in sorted order). Defaults to wrangler's 8787.
        /// A single context left on the default keeps `-- --port N` working.
        #[arg(long, value_name = "PORT")]
        base_port: Option<u16>,
        /// Serve with the V8 inspector enabled (slice 3, ADR 0104): `wrangler dev`
        /// starts with `--inspector-port` so a JavaScript debugger can attach.
        /// Breakpoints set in `.bynk` sources resolve through the emitted source
        /// maps, composed into the worker bundle. Prints the inspector URL on start.
        #[arg(long)]
        inspect: bool,
        /// First inspector port for `--inspect`, allocated per context exactly
        /// as `--base-port` is (default 9229).
        #[arg(long, default_value_t = 9229)]
        inspect_port: u16,
        /// Which `bynk.deploy.lock` environment `-- --remote` reads the KV id
        /// from. `dev` never provisions, so this only selects among what
        /// `bynk deploy --env NAME` already recorded — irrelevant without
        /// `--remote`. Omit for today's single, unqualified default.
        #[arg(long, default_value = "default", value_name = "NAME")]
        env: String,
        /// Arguments after `--`, forwarded to `wrangler dev` (e.g. `-- --remote`).
        /// Ports are the driver's to allocate: use `--base-port` / `--inspect-port`.
        #[arg(last = true)]
        wrangler_args: Vec<String>,
    },
    /// Provision each context's Cloudflare resources and deploy its Worker.
    /// The whole project ships in one command, in Service-Binding dependency
    /// order — Cloudflare rejects a Worker uploaded before a Worker it binds
    /// to. The generated configuration remains disposable: Cloudflare ids live
    /// in the committed `bynk.deploy.lock`.
    Deploy {
        /// Project directory to deploy from. Defaults to the current directory.
        #[arg(default_value = ".")]
        path: PathBuf,
        /// Deploy this context alone, assuming the contexts it consumes are
        /// already live; a dependency that has never been deployed is reported
        /// rather than pushed into. Accepts the dotted name or its dasherised
        /// worker-dir form. Omit to deploy the whole project in order.
        #[arg(long, value_name = "NAME")]
        context: Option<String>,
        /// Target environment. Selects the `bynk.deploy.lock` section and,
        /// for any value other than `default`, synthesises an environment-
        /// scoped Wrangler config section (KV, queues, Service Bindings all
        /// qualified) since Cloudflare does not inherit bindings into a named
        /// environment. Omit to deploy today's single, unqualified default.
        #[arg(long, default_value = "default", value_name = "NAME")]
        env: String,
        /// Print the provisioning and deploy plan without changing Cloudflare
        /// or writing `bynk.deploy.lock`.
        #[arg(long, visible_alias = "plan")]
        dry_run: bool,
        /// Plan output format. `short` is line-oriented; `json` is for CI.
        #[arg(long, value_enum, default_value = "short")]
        format: DeployFormatArg,
        /// Skip the confirmation required before creating a namespace or
        /// publishing a Worker. Required for non-interactive automation.
        #[arg(long)]
        yes: bool,
        /// Read secret values from a dotenv-style `NAME=value` file. Supplies
        /// both names and values; never committed, never persisted. Values move
        /// to `wrangler secret put` and are dropped.
        #[arg(long, value_name = "PATH")]
        secrets_file: Option<PathBuf>,
        /// Set this named secret, taking its value from the environment (or a
        /// prompt). Repeatable. Use for a `bynk.Secrets` name, whose spelling
        /// the compiler cannot know — an actor's declared `auth` secret needs no
        /// flag. The environment is never scanned for names.
        #[arg(long = "secret", value_name = "NAME")]
        secrets: Vec<String>,
        /// Overwrite a secret that is already set. The default sets only the
        /// missing ones, so a re-deploy does not cut a fresh Cloudflare secret
        /// version for every secret every time.
        #[arg(long)]
        force: bool,
        /// Arguments after `--`, forwarded to `wrangler deploy` verbatim.
        #[arg(last = true)]
        wrangler_args: Vec<String>,
    },
    /// Scaffold a new project: a complete, runnable single-context HTTP service
    /// you can serve immediately with `bynk dev`.
    ///
    /// Writes a `bynk.toml`, a `.gitignore`, and `src/<name>.bynk` into a new
    /// directory. Pure offline file-writing — it shells nothing and needs no
    /// toolchain, so you can run it before `bynkc`, Node, or `wrangler` are
    /// installed. The project name defaults to the target directory's final
    /// component; `--name` overrides it and must be a legal Bynk identifier.
    New {
        /// Directory to create for the new project (e.g. `hello` or `./hello`).
        path: PathBuf,
        /// Project name / context identifier. Defaults to PATH's final
        /// component; must be a legal Bynk identifier (a letter followed by
        /// letters, digits, or underscores).
        #[arg(long)]
        name: Option<String>,
    },
    /// Type-check a `.bynk` file or project without writing output — the
    /// `bynkc check` behaviour through the driver's compiler resolution (v0.138).
    ///
    /// Runs the compiler pipeline in-process (no `bynkc` binary required); with
    /// `BYNK_BYNKC` set, the pinned compiler is shelled instead so an
    /// externally-managed `bynkc` still governs the result.
    Check {
        /// Input `.bynk` file or project root. Defaults to the current directory.
        #[arg(default_value = ".")]
        input: PathBuf,
        /// Diagnostic output format. `rich` (default) is the ariadne
        /// source-context rendering; `short` emits one terse
        /// `path:line:col: severity[category]: message` line per diagnostic,
        /// for tooling (the VS Code problem-matcher, CI, scripts).
        #[arg(long, value_enum, default_value = "rich")]
        format: CheckFormatArg,
    },
    /// Format `.bynk` source files in place — the `bynkc fmt` behaviour through
    /// the driver (v0.138). Passing `-` reads from stdin and writes to stdout.
    ///
    /// Runs the formatter in-process (no `bynkc` binary required); with
    /// `BYNK_BYNKC` set, the pinned compiler is shelled instead.
    Fmt {
        /// Files to format. Use `-` for stdin → stdout.
        inputs: Vec<PathBuf>,
        /// Check formatting without writing changes. Exits non-zero if any
        /// file is not already canonical.
        #[arg(long)]
        check: bool,
    },
    /// Discover and run test declarations in a project — the `bynkc test`
    /// behaviour through the driver (v0.138).
    ///
    /// Delegates to the `bynkc` the driver resolves (`BYNK_BYNKC` → PATH →
    /// sibling-of-`bynk`), so an editor or developer inherits the driver's
    /// richer compiler resolution instead of locating `bynkc` themselves.
    /// Requires `tsc` (with Node.js) or `tsx` on PATH, exactly as `bynkc test`.
    Test {
        /// Input project root directory. Defaults to the current directory.
        #[arg(default_value = ".")]
        input: PathBuf,
        /// Where to write compiled TypeScript test runner modules.
        /// Defaults to `<input>/out`.
        #[arg(short, long)]
        output: Option<PathBuf>,
        /// Skip the runner invocation. With `--format rich` this emits the
        /// generated test files; with `--format json` it emits a discovery
        /// document listing every suite and case without running them.
        #[arg(long)]
        no_run: bool,
        /// Output format. `rich` (default) is the grouped ✓ / ✗ human output;
        /// `json` is a single pinned JSON document of results, for tooling.
        #[arg(long, value_enum, default_value = "rich")]
        format: TestFormatArg,
        /// Compile a debug build and launch the test runner under Node's
        /// inspector (`node --inspect-brk`), printing the inspector URL for a
        /// JavaScript debugger to attach. Requires Node ≥ 22.18 (or ≥ 23.6
        /// unflagged). Does not run `tsc`.
        #[arg(long)]
        inspect: bool,
        /// The root seed for generative `property` tests, as hex (e.g.
        /// `0x5f3a`). A failing property prints the seed it used; re-running
        /// with `--seed <hex>` reproduces that run byte-for-byte.
        #[arg(long)]
        seed: Option<String>,
        /// Run only test cases whose name matches `<name>`, skipping the rest —
        /// the filter behind the editor's per-case `▷ Run Test` lens. No effect
        /// with `--no-run`.
        #[arg(long, value_name = "NAME")]
        case: Option<String>,
    },
}

/// `bynk check --format` selector, mirroring `bynkc`'s `DiagFormat` (rich/short)
/// so the two commands are drop-in equivalent.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Default, ValueEnum)]
pub enum CheckFormatArg {
    /// Ariadne rendering with full source context (the default).
    #[default]
    Rich,
    /// One terse `path:line:col: severity[category]: message` line per
    /// diagnostic — for the VS Code problem-matcher, CI, and scripts.
    Short,
}

/// `bynk test --format` selector, mirroring `bynkc`'s `TestFormat` (rich/json).
#[derive(Copy, Clone, Debug, PartialEq, Eq, Default, ValueEnum)]
pub enum TestFormatArg {
    /// The grouped `✓ / ✗` human output (the default).
    #[default]
    Rich,
    /// A single pinned JSON document of results, for tooling and CI.
    Json,
}

impl CheckFormatArg {
    /// The `bynkc check --format` token this maps to when the pinned compiler
    /// is shelled under `BYNK_BYNKC`.
    pub fn as_bynkc_arg(self) -> &'static str {
        match self {
            CheckFormatArg::Rich => "rich",
            CheckFormatArg::Short => "short",
        }
    }
}

impl TestFormatArg {
    /// The `bynkc test --format` token this maps to when `bynk test` shells the
    /// resolved compiler.
    pub fn as_bynkc_arg(self) -> &'static str {
        match self {
            TestFormatArg::Rich => "rich",
            TestFormatArg::Json => "json",
        }
    }
}

/// `--only` selector. Mirrors [`Capability`] minus the internal distinctions.
#[derive(Copy, Clone, Debug, PartialEq, Eq, ValueEnum)]
pub enum CapabilityArg {
    /// `bynkc` compile / check / fmt.
    Compile,
    /// `bynk test` — Node + tsc|tsx.
    Test,
    /// dev / deploy to Cloudflare — Node + wrangler.
    Deploy,
    /// Editor support — bynkc-lsp.
    Editor,
    /// Build Bynk from source — a Rust toolchain.
    Build,
}

impl From<CapabilityArg> for Capability {
    fn from(a: CapabilityArg) -> Self {
        match a {
            CapabilityArg::Compile => Capability::Compile,
            CapabilityArg::Test => Capability::Test,
            CapabilityArg::Deploy => Capability::Deploy,
            CapabilityArg::Editor => Capability::Editor,
            CapabilityArg::Build => Capability::BuildFromSource,
        }
    }
}

#[derive(Copy, Clone, Debug, PartialEq, Eq, Default, ValueEnum)]
pub enum FormatArg {
    #[default]
    Human,
    Short,
    Json,
}

/// Scriptable output choices for `bynk deploy`'s plan.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Default, ValueEnum)]
pub enum DeployFormatArg {
    #[default]
    Short,
    Json,
}

impl From<FormatArg> for Format {
    fn from(f: FormatArg) -> Self {
        match f {
            FormatArg::Human => Format::Human,
            FormatArg::Short => Format::Short,
            FormatArg::Json => Format::Json,
        }
    }
}

/// Build the [`DoctorOptions`] from the parsed flags.
pub fn doctor_options(only: Option<CapabilityArg>, strict: bool) -> DoctorOptions {
    DoctorOptions {
        only: only.map(Into::into),
        strict,
    }
}
