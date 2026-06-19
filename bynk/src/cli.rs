//! The `bynk` driver command-line interface.
//!
//! Kept thin: the driver shells `bynkc` and the Node toolchain. v0.46 ships a
//! single subcommand, `doctor`; `new` and `dev` join it in later slices.

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
