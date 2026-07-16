//! Bynk project configuration (`bynk.toml`).
//!
//! Parses the project's `bynk.toml` if one exists at the project root. All
//! fields have sensible defaults so an absent or minimal config is fine.

use std::path::Path;

use bynk_fmt::{FormatOptions, IndentStyle};
use serde::Deserialize;

/// Slice A: only the sections the **server** owns. `[paths]` is deliberately
/// absent — the project's trees are the *compiler's* to resolve
/// (`bynk_ide::AnalysisRoots::Project` → `read_project_paths`), and this file
/// re-deriving them is precisely the defect slice A removes. `[project]` is
/// absent because nothing read it.
#[derive(Debug, Deserialize, Default)]
struct RawConfig {
    #[serde(default)]
    fmt: FmtSection,
    #[serde(default)]
    lsp: LspSection,
}

#[derive(Debug, Deserialize, Clone)]
struct FmtSection {
    #[serde(default = "default_indent")]
    pub indent: String,
    #[serde(default)]
    pub indent_width: Option<u8>,
    #[serde(default = "default_max_line_width")]
    pub max_line_width: u32,
    #[serde(default = "default_trailing_comma")]
    pub trailing_comma: bool,
}

impl Default for FmtSection {
    fn default() -> Self {
        Self {
            indent: default_indent(),
            indent_width: None,
            max_line_width: default_max_line_width(),
            trailing_comma: default_trailing_comma(),
        }
    }
}

fn default_indent() -> String {
    "tab".into()
}
fn default_max_line_width() -> u32 {
    100
}
fn default_trailing_comma() -> bool {
    true
}

#[derive(Debug, Deserialize, Clone)]
struct LspSection {
    #[serde(default = "default_diagnostics_mode")]
    pub diagnostics_mode: String,
    #[serde(default = "default_diagnostics_debounce_ms")]
    pub diagnostics_debounce_ms: u64,
}

impl Default for LspSection {
    fn default() -> Self {
        Self {
            diagnostics_mode: default_diagnostics_mode(),
            diagnostics_debounce_ms: default_diagnostics_debounce_ms(),
        }
    }
}

fn default_diagnostics_mode() -> String {
    "live".into()
}
fn default_diagnostics_debounce_ms() -> u64 {
    300
}

/// Effective **server** configuration with all defaults resolved.
///
/// Slice A: this no longer carries a source root. It used to reduce `[paths]
/// include` to a single `src_dir` string — dropping every root but the first
/// and ignoring `exclude` — which made the LSP analyse a different project than
/// `bynkc` compiles. The trees now come from the compiler's own discovery; what
/// is left here is what the *server* owns: formatting and the diagnostics mode.
#[derive(Debug, Clone)]
pub struct ProjectConfig {
    pub indent: IndentStyle,
    pub max_line_width: u32,
    pub trailing_comma: bool,
    pub diagnostics_mode: DiagnosticsMode,
    pub diagnostics_debounce_ms: u64,
}

impl Default for ProjectConfig {
    fn default() -> Self {
        Self {
            indent: IndentStyle::Tab,
            max_line_width: 100,
            trailing_comma: true,
            diagnostics_mode: DiagnosticsMode::Live,
            diagnostics_debounce_ms: 300,
        }
    }
}

impl ProjectConfig {
    pub fn format_options(&self) -> FormatOptions {
        FormatOptions {
            indent: self.indent,
            max_line_width: self.max_line_width,
            trailing_comma: self.trailing_comma,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DiagnosticsMode {
    Live,
    OnSave,
}

/// Load `bynk.toml` from the given project root.
pub fn load_config(root: &Path) -> Option<ProjectConfig> {
    let path = root.join("bynk.toml");
    let source = std::fs::read_to_string(&path).ok()?;
    let raw: RawConfig = toml::from_str(&source).ok()?;
    let indent = match raw.fmt.indent.as_str() {
        "tab" => IndentStyle::Tab,
        "spaces" => IndentStyle::Spaces(raw.fmt.indent_width.unwrap_or(2)),
        _ => IndentStyle::Tab,
    };
    let diagnostics_mode = match raw.lsp.diagnostics_mode.as_str() {
        "on_save" => DiagnosticsMode::OnSave,
        _ => DiagnosticsMode::Live,
    };
    Some(ProjectConfig {
        indent,
        max_line_width: raw.fmt.max_line_width,
        trailing_comma: raw.fmt.trailing_comma,
        diagnostics_mode,
        diagnostics_debounce_ms: raw.lsp.diagnostics_debounce_ms,
    })
}
