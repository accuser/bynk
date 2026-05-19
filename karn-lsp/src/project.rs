//! Karn project configuration (`karn.toml`).
//!
//! Parses the project's `karn.toml` if one exists at the project root. All
//! fields have sensible defaults so an absent or minimal config is fine.

use std::path::Path;

use karnc::fmt::{FormatOptions, IndentStyle};
use serde::Deserialize;

#[derive(Debug, Deserialize, Default)]
struct RawConfig {
    #[serde(default)]
    project: ProjectSection,
    #[serde(default)]
    paths: PathsSection,
    #[serde(default)]
    fmt: FmtSection,
    #[serde(default)]
    lsp: LspSection,
}

#[derive(Debug, Deserialize, Default, Clone)]
struct ProjectSection {
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub version: Option<String>,
}

#[derive(Debug, Deserialize, Clone)]
struct PathsSection {
    #[serde(default = "default_src")]
    pub src: String,
    #[serde(default = "default_out")]
    pub out: String,
}

impl Default for PathsSection {
    fn default() -> Self {
        Self {
            src: default_src(),
            out: default_out(),
        }
    }
}

fn default_src() -> String {
    "src".into()
}
fn default_out() -> String {
    "out".into()
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

/// Effective project configuration with all defaults resolved.
#[derive(Debug, Clone)]
pub struct ProjectConfig {
    #[allow(dead_code)]
    pub project_name: Option<String>,
    #[allow(dead_code)]
    pub project_version: Option<String>,
    pub src_dir: String,
    #[allow(dead_code)]
    pub out_dir: String,
    pub indent: IndentStyle,
    pub max_line_width: u32,
    pub trailing_comma: bool,
    #[allow(dead_code)]
    pub diagnostics_mode: DiagnosticsMode,
    pub diagnostics_debounce_ms: u64,
}

impl Default for ProjectConfig {
    fn default() -> Self {
        Self {
            project_name: None,
            project_version: None,
            src_dir: "src".into(),
            out_dir: "out".into(),
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

#[allow(dead_code)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DiagnosticsMode {
    Live,
    OnSave,
}

/// Load `karn.toml` from the given project root.
pub fn load_config(root: &Path) -> Option<ProjectConfig> {
    let path = root.join("karn.toml");
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
        project_name: raw.project.name,
        project_version: raw.project.version,
        src_dir: raw.paths.src,
        out_dir: raw.paths.out,
        indent,
        max_line_width: raw.fmt.max_line_width,
        trailing_comma: raw.fmt.trailing_comma,
        diagnostics_mode,
        diagnostics_debounce_ms: raw.lsp.diagnostics_debounce_ms,
    })
}
