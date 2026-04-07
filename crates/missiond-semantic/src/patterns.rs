//! External pattern configuration for PTY parsers.
//!
//! Loads regex patterns and character sets from YAML files, supporting hot-reload
//! when files change. This decouples parser logic from hardcoded patterns, allowing
//! pattern updates without recompiling MissionD.
//!
//! Pattern files are stored in `{mission_home}/pty-patterns/{engine}.yaml`.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, RwLock};
use std::time::SystemTime;

use anyhow::{Context, Result};
use once_cell::sync::Lazy;
use regex::Regex;
use serde::{Deserialize, Serialize};
use tracing::{debug, info, warn};

use missiond_shared::default_mission_home;
use missiond_shared::CliEngine;

// ========== YAML Schema ==========

/// Top-level pattern config for a CLI engine
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EnginePatternFile {
    pub engine: String,
    #[serde(default = "default_schema_version")]
    pub schema_version: u32,

    #[serde(default)]
    pub spinner: SpinnerPatterns,
    #[serde(default)]
    pub prompt: PromptPatterns,
    #[serde(default)]
    pub status_bar: StatusBarPatterns,
    #[serde(default)]
    pub confirm: ConfirmPatterns,
    #[serde(default)]
    pub tool_output: ToolOutputPatterns,
    #[serde(default)]
    pub state: StatePatterns,
    #[serde(default)]
    pub anchors: Vec<AnchorDef>,

    // Gemini-specific
    #[serde(default)]
    pub box_drawing: Option<BoxDrawingPatterns>,
    #[serde(default)]
    pub thinking: Option<ThinkingPatterns>,
    #[serde(default)]
    pub error: Option<ErrorPatterns>,
    #[serde(default)]
    pub footer: Option<FooterPatterns>,
    #[serde(default)]
    pub tool_exec: Option<ToolExecPatterns>,
    #[serde(default)]
    pub placeholder: Option<PlaceholderPatterns>,
}

fn default_schema_version() -> u32 {
    1
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct SpinnerPatterns {
    #[serde(default)]
    pub chars: Vec<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct PromptPatterns {
    #[serde(default)]
    pub input: Option<String>,
    #[serde(default)]
    pub with_text: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct StatusBarPatterns {
    #[serde(default)]
    pub pattern: Option<String>,
    #[serde(default)]
    pub phase_skip_words: Vec<String>,
    #[serde(default)]
    pub phase_keywords: HashMap<String, Vec<String>>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ConfirmPatterns {
    #[serde(default)]
    pub option_trigger: Option<String>,
    #[serde(default)]
    pub ask_user_trigger: Option<String>,
    #[serde(default)]
    pub ask_user_marker: Option<String>,
    #[serde(default)]
    pub cancel_marker: Option<String>,
    #[serde(default)]
    pub yes_no: Option<String>,
    #[serde(default)]
    pub option_line: Option<String>,
    #[serde(default)]
    pub tool_info: Option<String>,
    #[serde(default)]
    pub skill_confirm: Option<String>,
    #[serde(default)]
    pub builtin_type: Option<String>,
    #[serde(default)]
    pub builtin_call: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ToolOutputPatterns {
    #[serde(default)]
    pub header_box: Option<String>,
    #[serde(default)]
    pub header_inline: Option<String>,
    #[serde(default)]
    pub param_line: Option<String>,
    #[serde(default)]
    pub output_line: Option<String>,
    #[serde(default)]
    pub known_tools: Vec<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct StatePatterns {
    #[serde(default)]
    pub spinner_line: Option<String>,
    #[serde(default)]
    pub separator: Option<String>,
    #[serde(default)]
    pub bottom_bar_ignore: Vec<String>,
    #[serde(default)]
    pub error_marker: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AnchorDef {
    pub id: String,
    pub pattern: String,
    #[serde(default = "default_true")]
    pub required: bool,
}

fn default_true() -> bool {
    true
}

// Gemini-specific pattern groups
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct BoxDrawingPatterns {
    pub strip_pattern: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ThinkingPatterns {
    pub pattern: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ErrorPatterns {
    pub pattern: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct FooterPatterns {
    pub pattern: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ToolExecPatterns {
    pub pattern: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct PlaceholderPatterns {
    pub pattern: Option<String>,
}

// ========== Compiled Patterns ==========

/// Compiled pattern set for a single engine, ready for parser use.
#[derive(Debug)]
pub struct CompiledPatterns {
    /// Raw YAML data (for accessing non-regex fields)
    pub raw: EnginePatternFile,
    /// Compiled regex cache: key → Regex
    regexes: HashMap<String, Regex>,
    /// Spinner chars as a char slice (for fast lookup)
    pub spinner_chars: Vec<char>,
    /// Known tool names
    pub known_tools: Vec<String>,
    /// Compiled anchor patterns
    pub anchors: Vec<(String, Regex, bool)>,
}

impl CompiledPatterns {
    fn compile(raw: EnginePatternFile) -> Result<Self> {
        let mut regexes = HashMap::new();

        // Helper to compile and insert a regex
        let mut insert = |key: &str, pattern: &Option<String>| -> Result<()> {
            if let Some(p) = pattern {
                // Handle spinner_chars placeholder
                let expanded = if p.contains("{spinner_chars}") {
                    let chars_str: String = raw
                        .spinner
                        .chars
                        .iter()
                        .map(|c| regex::escape(c))
                        .collect::<Vec<_>>()
                        .join("");
                    p.replace("{spinner_chars}", &chars_str)
                } else {
                    p.clone()
                };
                let re = Regex::new(&expanded)
                    .with_context(|| format!("Failed to compile regex '{}': {}", key, expanded))?;
                regexes.insert(key.to_string(), re);
            }
            Ok(())
        };

        // Compile all pattern fields
        insert("prompt.input", &raw.prompt.input)?;
        insert("prompt.with_text", &raw.prompt.with_text)?;
        insert("status_bar.pattern", &raw.status_bar.pattern)?;
        insert("confirm.option_trigger", &raw.confirm.option_trigger)?;
        insert("confirm.ask_user_trigger", &raw.confirm.ask_user_trigger)?;
        insert("confirm.yes_no", &raw.confirm.yes_no)?;
        insert("confirm.option_line", &raw.confirm.option_line)?;
        insert("confirm.tool_info", &raw.confirm.tool_info)?;
        insert("confirm.skill_confirm", &raw.confirm.skill_confirm)?;
        insert("confirm.builtin_type", &raw.confirm.builtin_type)?;
        insert("confirm.builtin_call", &raw.confirm.builtin_call)?;
        insert("tool_output.header_box", &raw.tool_output.header_box)?;
        insert("tool_output.header_inline", &raw.tool_output.header_inline)?;
        insert("tool_output.param_line", &raw.tool_output.param_line)?;
        insert("tool_output.output_line", &raw.tool_output.output_line)?;
        insert("state.spinner_line", &raw.state.spinner_line)?;
        insert("state.separator", &raw.state.separator)?;

        // Gemini-specific
        if let Some(ref bd) = raw.box_drawing {
            insert("box_drawing.strip", &bd.strip_pattern)?;
        }
        if let Some(ref t) = raw.thinking {
            insert("thinking.pattern", &t.pattern)?;
        }
        if let Some(ref e) = raw.error {
            insert("error.pattern", &e.pattern)?;
        }
        if let Some(ref f) = raw.footer {
            insert("footer.pattern", &f.pattern)?;
        }
        if let Some(ref te) = raw.tool_exec {
            insert("tool_exec.pattern", &te.pattern)?;
        }
        if let Some(ref ph) = raw.placeholder {
            insert("placeholder.pattern", &ph.pattern)?;
        }

        // Spinner chars
        let spinner_chars: Vec<char> = raw
            .spinner
            .chars
            .iter()
            .filter_map(|s| s.chars().next())
            .collect();

        // Known tools
        let known_tools = raw.tool_output.known_tools.clone();

        // Compile anchors
        let mut anchors = Vec::new();
        for a in &raw.anchors {
            match Regex::new(&a.pattern) {
                Ok(re) => anchors.push((a.id.clone(), re, a.required)),
                Err(e) => warn!("Failed to compile anchor '{}': {}", a.id, e),
            }
        }

        Ok(Self {
            raw,
            regexes,
            spinner_chars,
            known_tools,
            anchors,
        })
    }

    /// Get a compiled regex by key (e.g. "prompt.input", "confirm.yes_no")
    pub fn regex(&self, key: &str) -> Option<&Regex> {
        self.regexes.get(key)
    }

    /// Check if a string contains any spinner char
    pub fn is_spinner_char(&self, c: char) -> bool {
        self.spinner_chars.contains(&c)
    }

    /// Check if a tool name is known
    pub fn is_known_tool(&self, name: &str) -> bool {
        self.known_tools.iter().any(|t| t == name)
    }

    /// Get phase skip words
    pub fn phase_skip_words(&self) -> &[String] {
        &self.raw.status_bar.phase_skip_words
    }

    /// Get phase keywords mapping
    pub fn phase_keywords(&self) -> &HashMap<String, Vec<String>> {
        &self.raw.status_bar.phase_keywords
    }

    /// Get bottom bar ignore patterns
    pub fn bottom_bar_ignore(&self) -> &[String] {
        &self.raw.state.bottom_bar_ignore
    }

    /// Get error marker
    pub fn error_marker(&self) -> Option<&str> {
        self.raw.state.error_marker.as_deref()
    }

    /// Get cancel marker for confirms
    pub fn cancel_marker(&self) -> Option<&str> {
        self.raw.confirm.cancel_marker.as_deref()
    }

    /// Get ask user marker
    pub fn ask_user_marker(&self) -> Option<&str> {
        self.raw.confirm.ask_user_marker.as_deref()
    }

    /// Check all required anchors against terminal text
    pub fn check_anchors(&self, text: &str) -> Vec<(String, bool)> {
        self.anchors
            .iter()
            .filter(|(_, _, required)| *required)
            .map(|(id, re, _)| (id.clone(), re.is_match(text)))
            .collect()
    }
}

// ========== PatternConfig (Global Registry) ==========

/// Global pattern config with hot-reload support.
pub struct PatternConfig {
    patterns: HashMap<CliEngine, Arc<CompiledPatterns>>,
    file_mtimes: HashMap<CliEngine, SystemTime>,
    patterns_dir: PathBuf,
}

impl PatternConfig {
    /// Load patterns from the default mission home directory.
    pub fn load_default() -> Result<Self> {
        let patterns_dir = default_mission_home().join("pty-patterns");
        Self::load_from(&patterns_dir)
    }

    /// Load patterns from a specific directory.
    pub fn load_from(patterns_dir: &Path) -> Result<Self> {
        let mut config = Self {
            patterns: HashMap::new(),
            file_mtimes: HashMap::new(),
            patterns_dir: patterns_dir.to_path_buf(),
        };

        // Ensure directory exists
        if !patterns_dir.exists() {
            std::fs::create_dir_all(patterns_dir)
                .with_context(|| format!("Failed to create {}", patterns_dir.display()))?;
        }

        // Load each engine
        for (engine, filename) in &[
            (CliEngine::ClaudeCode, "claude-code.yaml"),
            (CliEngine::Gemini, "gemini-cli.yaml"),
        ] {
            let path = patterns_dir.join(filename);
            if !path.exists() {
                // Write default patterns
                let default_content = default_patterns_yaml(*engine);
                std::fs::write(&path, &default_content)
                    .with_context(|| format!("Failed to write default {}", path.display()))?;
                info!("Created default pattern file: {}", path.display());
            }
            config.load_engine(*engine, &path)?;
        }

        Ok(config)
    }

    fn load_engine(&mut self, engine: CliEngine, path: &Path) -> Result<()> {
        let content = std::fs::read_to_string(path)
            .with_context(|| format!("Failed to read {}", path.display()))?;
        let raw: EnginePatternFile = serde_yaml::from_str(&content)
            .with_context(|| format!("Failed to parse {}", path.display()))?;
        let compiled = CompiledPatterns::compile(raw)
            .with_context(|| format!("Failed to compile patterns from {}", path.display()))?;

        let mtime = std::fs::metadata(path)?.modified()?;
        self.patterns.insert(engine, Arc::new(compiled));
        self.file_mtimes.insert(engine, mtime);

        debug!(
            "Loaded {} patterns ({} regexes)",
            engine,
            self.patterns[&engine].regexes.len()
        );
        Ok(())
    }

    /// Check file mtimes and reload if changed. Returns true if any file was reloaded.
    pub fn maybe_reload(&mut self) -> bool {
        let mut reloaded = false;
        let engines: Vec<(CliEngine, &str)> = vec![
            (CliEngine::ClaudeCode, "claude-code.yaml"),
            (CliEngine::Gemini, "gemini-cli.yaml"),
        ];

        for (engine, filename) in engines {
            let path = self.patterns_dir.join(filename);
            if !path.exists() {
                continue;
            }
            let Ok(meta) = std::fs::metadata(&path) else {
                continue;
            };
            let Ok(mtime) = meta.modified() else {
                continue;
            };
            if self.file_mtimes.get(&engine) == Some(&mtime) {
                continue;
            }
            match self.load_engine(engine, &path) {
                Ok(()) => {
                    info!("Hot-reloaded patterns for {}", engine);
                    reloaded = true;
                }
                Err(e) => {
                    warn!("Failed to reload patterns for {}: {}", engine, e);
                }
            }
        }
        reloaded
    }

    /// Get compiled patterns for an engine (returns Arc for lock-free sharing)
    pub fn get(&self, engine: CliEngine) -> Option<Arc<CompiledPatterns>> {
        self.patterns.get(&engine).cloned()
    }

    /// Get patterns directory
    pub fn patterns_dir(&self) -> &Path {
        &self.patterns_dir
    }
}

// ========== Global Singleton ==========

/// Global pattern config, lazily initialized and hot-reloadable.
static GLOBAL_PATTERNS: Lazy<Arc<RwLock<PatternConfig>>> = Lazy::new(|| {
    match PatternConfig::load_default() {
        Ok(config) => Arc::new(RwLock::new(config)),
        Err(e) => {
            warn!("Failed to load pattern config, using defaults: {}", e);
            // Create an empty config as fallback
            Arc::new(RwLock::new(PatternConfig {
                patterns: HashMap::new(),
                file_mtimes: HashMap::new(),
                patterns_dir: default_mission_home().join("pty-patterns"),
            }))
        }
    }
});

/// Get the global pattern config (read-only access)
pub fn global_patterns() -> Arc<RwLock<PatternConfig>> {
    GLOBAL_PATTERNS.clone()
}

/// Trigger hot-reload check on global patterns. Call periodically (e.g. every 10s).
pub fn maybe_reload_global_patterns() -> bool {
    match GLOBAL_PATTERNS.write() {
        Ok(mut config) => config.maybe_reload(),
        Err(e) => {
            warn!("Failed to acquire write lock for pattern reload: {}", e);
            false
        }
    }
}

// ========== Default YAML Content (embedded via include_str! equivalent) ==========

/// Get default compiled patterns for an engine (from embedded YAML).
/// Used as fallback when disk file is missing/broken, and in tests.
pub fn default_compiled(engine: CliEngine) -> Option<CompiledPatterns> {
    let yaml = default_patterns_yaml(engine);
    if yaml.is_empty() {
        return None;
    }
    let raw: EnginePatternFile = serde_yaml::from_str(&yaml).ok()?;
    CompiledPatterns::compile(raw).ok()
}

fn default_patterns_yaml(engine: CliEngine) -> String {
    match engine {
        CliEngine::ClaudeCode => DEFAULT_CLAUDE_CODE_YAML.to_string(),
        CliEngine::Gemini => DEFAULT_GEMINI_CLI_YAML.to_string(),
        _ => String::new(),
    }
}

const DEFAULT_CLAUDE_CODE_YAML: &str = r#"engine: claude_code
schema_version: 1

spinner:
  chars: ["·", "✻", "✽", "✶", "✳", "✢", "*"]

prompt:
  input: "^[❯>]\\s*"
  with_text: "^[❯>]\\s+\\S"

status_bar:
  pattern: "^([·✻✽✶✳✢*])\\s+(\\S+…?)\\s*\\((.+)\\)\\s*$"
  phase_skip_words: ["esc", "interrupt", "shift", "tab", "bypass"]
  phase_keywords:
    thinking: ["thinking", "thought"]
    tool_running: ["tool", "running"]

confirm:
  option_trigger: "(?mi)^[\\s❯>]*1\\.\\s*(Yes|Allow)"
  ask_user_trigger: "(?mi)^[\\s❯>]*1\\.\\s+\\S"
  ask_user_marker: "Enter to select"
  cancel_marker: "Esc to cancel"
  yes_no: "(?i)\\[Y/n\\]|\\(yes/no\\)|Allow\\?|Do you want to proceed"
  option_line: "^[\\s❯>]*(\\d+)\\.\\s*(.+)$"
  tool_info: "(\\S+)\\s*-\\s*(\\w+)\\s*\\(([^)]*)\\)(?:\\s*\\(MCP\\))?"
  skill_confirm: '(?i)Use skill "([^"]+)"'
  builtin_type: "(?m)^\\s*(Read|Write|Edit|MultiEdit|Bash|NotebookEdit)\\s+(file|command|files?)?\\s*$"
  builtin_call: "(?m)(Read|Write|Edit|Bash|Grep|Glob|Search|LSP|Agent|NotebookEdit)\\s*\\("

tool_output:
  header_box: "^⏺\\s+(\\w+)(?:\\s+\\(completed\\s+in\\s+([\\d.]+)s?\\))?$"
  header_inline: "^⏺\\s+(\\w+)\\((.*)\\)$"
  param_line: "^\\s*│\\s*(\\w+):\\s*(.+)$"
  output_line: "^\\s*⎿\\s*(.+)$"
  known_tools:
    - Bash
    - Read
    - Edit
    - Write
    - Glob
    - Grep
    - WebFetch
    - WebSearch
    - Task
    - LSP
    - NotebookEdit
    - Search
    - TodoRead
    - TodoWrite

state:
  spinner_line: "^\\s*[{spinner_chars}]\\s+\\S"
  separator: "^[─━═]+"
  bottom_bar_ignore: ["⏵⏵", "bypass permissions"]
  error_marker: "✖"

anchors:
  - id: bottom_bar
    pattern: "esc to interrupt"
    required: true
  - id: prompt_symbol
    pattern: "[❯>]"
    required: true
  - id: tool_marker
    pattern: "⏺"
    required: true
"#;

const DEFAULT_GEMINI_CLI_YAML: &str = r#"engine: gemini
schema_version: 1

spinner:
  chars: ["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏"]

prompt:
  input: "^\\s*>\\s"

box_drawing:
  strip_pattern: "[╭╮╰╯│─├┤┬┴┼┌┐└┘╌╍]"

thinking:
  pattern: "(?i)(Thinking\\s*\\.{0,3}|esc to cancel)"

error:
  pattern: "(?i)^Error:|error:"

footer:
  pattern: "/model\\s+\\S"

tool_exec:
  pattern: "(?i)(Executing|Running|✓|✗|⠏)\\s+\\w"

placeholder:
  pattern: "Type your message|@path/to/file"

anchors:
  - id: footer_model
    pattern: "/model"
    required: true
  - id: box_border
    pattern: "[╭╮╰╯]"
    required: true
"#;

// ========== Tests ==========

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn test_load_default_claude_code_yaml() {
        let raw: EnginePatternFile = serde_yaml::from_str(DEFAULT_CLAUDE_CODE_YAML).unwrap();
        assert_eq!(raw.engine, "claude_code");
        assert_eq!(raw.spinner.chars.len(), 7);
        assert!(raw.confirm.option_trigger.is_some());
        assert!(raw.tool_output.known_tools.len() >= 14);
        assert!(!raw.anchors.is_empty());
    }

    #[test]
    fn test_load_default_gemini_yaml() {
        let raw: EnginePatternFile = serde_yaml::from_str(DEFAULT_GEMINI_CLI_YAML).unwrap();
        assert_eq!(raw.engine, "gemini");
        assert_eq!(raw.spinner.chars.len(), 10);
        assert!(raw.box_drawing.is_some());
        assert!(raw.thinking.is_some());
    }

    #[test]
    fn test_compile_claude_code_patterns() {
        let raw: EnginePatternFile = serde_yaml::from_str(DEFAULT_CLAUDE_CODE_YAML).unwrap();
        let compiled = CompiledPatterns::compile(raw).unwrap();

        // Check regexes compiled
        assert!(compiled.regex("prompt.input").is_some());
        assert!(compiled.regex("status_bar.pattern").is_some());
        assert!(compiled.regex("confirm.yes_no").is_some());
        assert!(compiled.regex("state.spinner_line").is_some());

        // Spinner line should match
        let spinner_re = compiled.regex("state.spinner_line").unwrap();
        assert!(spinner_re.is_match("✳ Determining…"));
        assert!(spinner_re.is_match("  ✻ Reading..."));
        assert!(spinner_re.is_match("* Honking…"));
        assert!(!spinner_re.is_match("    ✻")); // no text after spinner

        // Spinner chars
        assert_eq!(compiled.spinner_chars.len(), 7);
        assert!(compiled.is_spinner_char('·'));
        assert!(compiled.is_spinner_char('*'));
        assert!(!compiled.is_spinner_char('⠋'));

        // Known tools
        assert!(compiled.is_known_tool("Bash"));
        assert!(compiled.is_known_tool("Read"));
        assert!(!compiled.is_known_tool("CustomTool"));

        // Anchors
        assert_eq!(compiled.anchors.len(), 3);
    }

    #[test]
    fn test_compile_gemini_patterns() {
        let raw: EnginePatternFile = serde_yaml::from_str(DEFAULT_GEMINI_CLI_YAML).unwrap();
        let compiled = CompiledPatterns::compile(raw).unwrap();

        assert!(compiled.regex("prompt.input").is_some());
        assert!(compiled.regex("box_drawing.strip").is_some());
        assert!(compiled.regex("thinking.pattern").is_some());

        // Braille spinner chars
        assert_eq!(compiled.spinner_chars.len(), 10);
        assert!(compiled.is_spinner_char('⠋'));
        assert!(!compiled.is_spinner_char('✳'));
    }

    #[test]
    fn test_load_from_directory() {
        let dir = TempDir::new().unwrap();
        let config = PatternConfig::load_from(dir.path()).unwrap();

        // Should have created default files and loaded them
        assert!(config.get(CliEngine::ClaudeCode).is_some());
        assert!(config.get(CliEngine::Gemini).is_some());

        // Files should exist
        assert!(dir.path().join("claude-code.yaml").exists());
        assert!(dir.path().join("gemini-cli.yaml").exists());
    }

    #[test]
    fn test_hot_reload() {
        let dir = TempDir::new().unwrap();
        let mut config = PatternConfig::load_from(dir.path()).unwrap();

        // No change → no reload
        assert!(!config.maybe_reload());

        // Modify file with a delay to ensure mtime difference
        std::thread::sleep(std::time::Duration::from_secs(1));
        let path = dir.path().join("claude-code.yaml");
        let mut content: EnginePatternFile =
            serde_yaml::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
        content.spinner.chars.push("⊛".to_string());
        std::fs::write(&path, serde_yaml::to_string(&content).unwrap()).unwrap();

        // Should detect change and reload
        assert!(config.maybe_reload());
        let compiled = config.get(CliEngine::ClaudeCode).unwrap();
        assert_eq!(compiled.spinner_chars.len(), 8);
        assert!(compiled.is_spinner_char('⊛'));
    }

    #[test]
    fn test_anchor_check() {
        let raw: EnginePatternFile = serde_yaml::from_str(DEFAULT_CLAUDE_CODE_YAML).unwrap();
        let compiled = CompiledPatterns::compile(raw).unwrap();

        // Text with all anchors
        let text = "❯ \n⏺ Bash\nesc to interrupt";
        let results = compiled.check_anchors(text);
        assert!(results.iter().all(|(_, matched)| *matched));

        // Text missing tool_marker
        let text = "❯ \nesc to interrupt";
        let results = compiled.check_anchors(text);
        let tool_anchor = results.iter().find(|(id, _)| id == "tool_marker");
        assert_eq!(tool_anchor.unwrap().1, false);
    }

    #[test]
    fn test_status_bar_regex_matches_both_formats() {
        let raw: EnginePatternFile = serde_yaml::from_str(DEFAULT_CLAUDE_CODE_YAML).unwrap();
        let compiled = CompiledPatterns::compile(raw).unwrap();
        let re = compiled.regex("status_bar.pattern").unwrap();

        // Legacy format
        assert!(re.is_match("· Precipitating… (esc to interrupt · thinking)"));
        // v2.x format
        assert!(re.is_match("✢ Undulating… (3m 2s · ↓ 2.8k tokens · thinking)"));
        // ASCII spinner
        assert!(re.is_match("* Honking… (3s · thinking)"));
    }
}
