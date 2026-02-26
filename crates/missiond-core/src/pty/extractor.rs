//! Incremental Extractor - Frame-by-frame diff tracking for terminal buffer
//!
//! Extracts stable text operations from terminal screen changes,
//! filtering out transient UI elements like spinners and status bars.

use alacritty_terminal::grid::Dimensions;
use alacritty_terminal::term::Term;
use once_cell::sync::Lazy;
use regex::Regex;

// ========== Patterns ==========

/// Spinner-only line (e.g., "· · ·")
static SPINNER_ONLY_PATTERN: Lazy<Regex> =
    Lazy::new(|| Regex::new(r"^[·✻✽✶✳✢⠐⠂⠈⠁⠉⠃⠋⠓⠒⠖⠦⠤]+$").unwrap());

/// Separator line (e.g., "────")
static SEPARATOR_PATTERN: Lazy<Regex> = Lazy::new(|| Regex::new(r"^[─━═]+$").unwrap());

/// Status bar pattern
static STATUSBAR_PATTERN: Lazy<Regex> =
    Lazy::new(|| Regex::new(r"(?i)esc to interrupt").unwrap());

/// Prompt-only line (e.g., "> " or "❯ ")
static PROMPT_ONLY_PATTERN: Lazy<Regex> = Lazy::new(|| Regex::new(r"^[❯>]\s*$").unwrap());

/// Spinner status line (e.g., "✳ Determining…", "· Processing…")
/// These lines are transient — the spinner character and timer update every frame.
static SPINNER_LINE_PATTERN: Lazy<Regex> =
    Lazy::new(|| Regex::new(r"^\s*[·✻✽✶✳✢]\s+\S").unwrap());

// ========== Types ==========

/// Data for a single terminal line
#[derive(Debug, Clone)]
pub struct LineData {
    pub text: String,
    pub is_wrapped: bool,
}

/// Snapshot of the terminal screen
#[derive(Debug, Clone)]
pub struct ScreenSnapshot {
    pub start_y: usize,
    pub end_y: usize,
    pub lines: Vec<LineData>,
    pub cursor_x: usize,
    pub cursor_y: usize,
    pub base_y: usize,
    pub timestamp: i64,
}

/// A stable text operation extracted from frame diff
#[derive(Debug, Clone)]
pub enum StableTextOp {
    /// New complete line
    Line {
        y: usize,
        text: String,
        is_wrapped: bool,
    },
    /// Line content replaced (e.g., spinner -> actual text)
    Replace {
        y: usize,
        text: String,
        is_wrapped: bool,
    },
    /// Text appended to existing line (streaming)
    Append { y: usize, text: String },
}

impl StableTextOp {
    pub fn y(&self) -> usize {
        match self {
            StableTextOp::Line { y, .. } => *y,
            StableTextOp::Replace { y, .. } => *y,
            StableTextOp::Append { y, .. } => *y,
        }
    }

    pub fn text(&self) -> &str {
        match self {
            StableTextOp::Line { text, .. } => text,
            StableTextOp::Replace { text, .. } => text,
            StableTextOp::Append { text, .. } => text,
        }
    }

    pub fn kind(&self) -> &'static str {
        match self {
            StableTextOp::Line { .. } => "line",
            StableTextOp::Replace { .. } => "replace",
            StableTextOp::Append { .. } => "append",
        }
    }

    pub fn is_wrapped(&self) -> bool {
        match self {
            StableTextOp::Line { is_wrapped, .. } => *is_wrapped,
            StableTextOp::Replace { is_wrapped, .. } => *is_wrapped,
            StableTextOp::Append { .. } => false,
        }
    }
}

/// Added line info
#[derive(Debug, Clone)]
pub struct AddedLine {
    pub y: usize,
    pub text: String,
    pub is_wrapped: bool,
}

/// Modified line info
#[derive(Debug, Clone)]
pub struct ModifiedLine {
    pub y: usize,
    pub old_text: String,
    pub new_text: String,
    pub is_wrapped: bool,
}

/// Frame delta containing all changes since last extraction
#[derive(Debug, Clone)]
pub struct FrameDelta {
    pub timestamp: i64,
    pub added_lines: Vec<AddedLine>,
    pub modified_lines: Vec<ModifiedLine>,
    pub scrolled_lines: i32,
    pub stable_ops: Vec<StableTextOp>,
    pub cursor_position: (usize, usize),
    pub window: (usize, usize),
}

// ========== Extractor ==========

/// Incremental extractor for terminal text
///
/// Tracks frame-by-frame changes in the terminal buffer and extracts
/// stable text operations, filtering out transient UI elements.
pub struct IncrementalExtractor {
    last_snapshot: Option<ScreenSnapshot>,
    window_lines: usize,
}

impl IncrementalExtractor {
    /// Create a new extractor
    ///
    /// # Arguments
    /// * `rows` - Terminal rows (used to calculate default window size)
    /// * `window_lines` - Optional custom window size (default: rows * 20, min 800)
    pub fn new(rows: usize, window_lines: Option<usize>) -> Self {
        let default_window = std::cmp::max(rows * 20, 800);
        Self {
            last_snapshot: None,
            window_lines: window_lines.unwrap_or(default_window),
        }
    }

    /// Extract frame delta since last call
    ///
    /// # Type Parameters
    /// * `T` - Term event listener type (from alacritty_terminal)
    pub fn extract<T>(&mut self, term: &Term<T>) -> FrameDelta {
        let current = self.capture_screen(term);
        let delta = self.compute_delta(self.last_snapshot.as_ref(), &current);
        self.last_snapshot = Some(current);
        delta
    }

    /// Reset state (call when session restarts)
    pub fn reset(&mut self) {
        self.last_snapshot = None;
    }

    /// Get current screen without updating state
    pub fn peek<T>(&self, term: &Term<T>) -> ScreenSnapshot {
        self.capture_screen(term)
    }

    /// Helper to get a line from snapshot by absolute Y
    fn get_snapshot_line(snap: &ScreenSnapshot, abs_y: usize) -> Option<&LineData> {
        if abs_y < snap.start_y || abs_y >= snap.end_y {
            return None;
        }
        snap.lines.get(abs_y - snap.start_y)
    }

    /// Capture current screen state
    fn capture_screen<T>(&self, term: &Term<T>) -> ScreenSnapshot {
        let grid = term.grid();
        let mut lines = Vec::new();

        let rows = grid.screen_lines();

        // Line(0) = top of visible screen, Line(rows-1) = bottom.
        // Capture only the bottom window_lines of the visible area.
        let start_y = if rows > self.window_lines {
            rows - self.window_lines
        } else {
            0
        };

        // Capture lines in the window
        for y in start_y..rows {
            let line_idx = alacritty_terminal::index::Line(y as i32);
            {
                let row = &grid[line_idx];
                // Skip wide-char spacer cells to avoid extra spaces between CJK characters.
                // Alacritty stores wide chars (CJK, emoji) across 2 cells:
                // cell[0] = the char (WIDE_CHAR flag), cell[1] = space (WIDE_CHAR_SPACER flag).
                let text: String = row
                    .into_iter()
                    .filter(|cell| {
                        !cell
                            .flags
                            .contains(alacritty_terminal::term::cell::Flags::WIDE_CHAR_SPACER)
                    })
                    .map(|cell| cell.c)
                    .collect();
                let text = text.trim_end().to_string();

                // Check if line is wrapped (continues from previous line)
                // In alacritty_terminal, wrapped lines have their first cell marked
                let is_wrapped = if row.len() > 0 {
                    row[alacritty_terminal::index::Column(0)]
                        .flags
                        .contains(alacritty_terminal::term::cell::Flags::WRAPLINE)
                } else {
                    false
                };

                lines.push(LineData { text, is_wrapped });
            }
        }

        let cursor = &term.grid().cursor;

        ScreenSnapshot {
            start_y,
            end_y: rows,
            lines,
            cursor_x: cursor.point.column.0,
            cursor_y: cursor.point.line.0 as usize,
            base_y: 0,
            timestamp: chrono::Utc::now().timestamp_millis(),
        }
    }

    /// Compute delta between two snapshots
    fn compute_delta(&self, prev: Option<&ScreenSnapshot>, curr: &ScreenSnapshot) -> FrameDelta {
        let mut added_lines = Vec::new();
        let mut modified_lines = Vec::new();
        let scrolled_lines: i32;

        match prev {
            None => {
                // First capture: all non-empty lines are new
                scrolled_lines = 0;
                for (local_y, line) in curr.lines.iter().enumerate() {
                    let y = curr.start_y + local_y;
                    if !line.text.trim().is_empty() {
                        added_lines.push(AddedLine {
                            y,
                            text: line.text.clone(),
                            is_wrapped: line.is_wrapped,
                        });
                    }
                }
            }
            Some(prev) => {
                // Calculate scroll amount
                scrolled_lines = curr.base_y as i32 - prev.base_y as i32;

                // Compare lines
                let start_y = std::cmp::min(prev.start_y, curr.start_y);
                let end_y = std::cmp::max(prev.end_y, curr.end_y);

                for y in start_y..end_y {
                    let prev_line = Self::get_snapshot_line(prev, y);
                    let curr_line = Self::get_snapshot_line(curr, y);
                    let prev_text = prev_line.map(|l| l.text.as_str()).unwrap_or("");
                    let curr_text = curr_line.map(|l| l.text.as_str()).unwrap_or("");
                    let curr_wrapped = curr_line.map(|l| l.is_wrapped).unwrap_or(false);

                    if prev_line.is_none() && curr_line.is_some() {
                        // New line entered the window
                        // Ignore top-expansion of the window to avoid replaying history
                        if y < prev.start_y {
                            continue;
                        }
                        if !curr_text.trim().is_empty() {
                            added_lines.push(AddedLine {
                                y,
                                text: curr_text.to_string(),
                                is_wrapped: curr_wrapped,
                            });
                        }
                        continue;
                    }

                    if prev_line.is_some()
                        && curr_line.is_some()
                        && prev_text != curr_text
                        && !curr_text.trim().is_empty()
                    {
                        modified_lines.push(ModifiedLine {
                            y,
                            old_text: prev_text.to_string(),
                            new_text: curr_text.to_string(),
                            is_wrapped: curr_wrapped,
                        });
                    }
                }
            }
        }

        // Extract stable operations
        let stable_ops = self.extract_stable_ops(&added_lines, &modified_lines);

        FrameDelta {
            timestamp: curr.timestamp,
            added_lines,
            modified_lines,
            scrolled_lines,
            stable_ops,
            cursor_position: (curr.cursor_x, curr.cursor_y),
            window: (curr.start_y, curr.end_y),
        }
    }

    /// Extract stable operations from added and modified lines
    fn extract_stable_ops(
        &self,
        added: &[AddedLine],
        modified: &[ModifiedLine],
    ) -> Vec<StableTextOp> {
        let mut ops = Vec::new();

        // Process added lines - these are complete new lines
        for line in added {
            let text = line.text.trim_end();
            if self.is_stable_line(text) {
                ops.push(StableTextOp::Line {
                    y: line.y,
                    text: text.to_string(),
                    is_wrapped: line.is_wrapped,
                });
            }
        }

        // Process modified lines - extract appended content
        for line in modified {
            let new_text = line.new_text.trim_end();
            let old_text = line.old_text.trim_end();

            // Case 1: old was unstable (spinner/status), new is stable -> full line
            if self.is_stable_line(new_text) && !self.is_stable_line(old_text) {
                ops.push(StableTextOp::Replace {
                    y: line.y,
                    text: new_text.to_string(),
                    is_wrapped: line.is_wrapped,
                });
                continue;
            }

            // Case 2: Both stable, extract changes
            if self.is_stable_line(new_text) && self.is_stable_line(old_text) {
                if new_text.starts_with(old_text) && new_text.len() > old_text.len() {
                    // Appended text (streaming)
                    let appended = &new_text[old_text.len()..];
                    if !appended.is_empty() {
                        ops.push(StableTextOp::Append {
                            y: line.y,
                            text: appended.to_string(),
                        });
                    }
                } else if new_text != old_text {
                    // Complete content replacement (different stable content)
                    ops.push(StableTextOp::Replace {
                        y: line.y,
                        text: new_text.to_string(),
                        is_wrapped: line.is_wrapped,
                    });
                }
            }
        }

        // Sort by y position, then by kind (line/replace before append)
        ops.sort_by(|a, b| {
            if a.y() != b.y() {
                return a.y().cmp(&b.y());
            }
            let order = |op: &StableTextOp| -> u8 {
                match op {
                    StableTextOp::Append { .. } => 2,
                    _ => 1,
                }
            };
            order(a).cmp(&order(b))
        });

        ops
    }

    /// Check if a line is stable (not transient UI)
    fn is_stable_line(&self, text: &str) -> bool {
        let trimmed = text.trim();
        if trimmed.is_empty() {
            return false;
        }

        // Filter out spinner-only lines (e.g., "·····")
        if SPINNER_ONLY_PATTERN.is_match(trimmed) {
            return false;
        }

        // Filter out spinner status lines (e.g., "✳ Determining…", "✢ Incubating… (3s · thinking)")
        // These are transient — spinner char and timer update every frame.
        if SPINNER_LINE_PATTERN.is_match(trimmed) {
            return false;
        }

        // Filter out separator lines
        if SEPARATOR_PATTERN.is_match(trimmed) {
            return false;
        }

        // Filter out status bar
        if STATUSBAR_PATTERN.is_match(trimmed) {
            return false;
        }

        // Filter out prompt-only lines
        if PROMPT_ONLY_PATTERN.is_match(trimmed) {
            return false;
        }

        true
    }
}

// ========== TextAssembler ==========

/// Assembles streaming text from stable operations
///
/// Handles newlines and wrapped lines correctly to produce
/// coherent text output.
pub struct TextAssembler {
    buffer: String,
}

impl Default for TextAssembler {
    fn default() -> Self {
        Self::new()
    }
}

impl TextAssembler {
    /// Create a new text assembler
    pub fn new() -> Self {
        Self {
            buffer: String::new(),
        }
    }

    /// Reset the assembler
    pub fn reset(&mut self) {
        self.buffer.clear();
    }

    /// Apply a stable operation and return the chunk added
    pub fn apply(&mut self, op: &StableTextOp) -> String {
        if op.text().is_empty() {
            return String::new();
        }

        match op {
            StableTextOp::Append { text, .. } => {
                self.buffer.push_str(text);
                text.clone()
            }
            StableTextOp::Line { text, is_wrapped, .. }
            | StableTextOp::Replace { text, is_wrapped, .. } => {
                let needs_newline =
                    !self.buffer.is_empty() && !self.buffer.ends_with('\n') && !is_wrapped;

                let chunk = if needs_newline {
                    format!("\n{}", text)
                } else {
                    text.clone()
                };

                self.buffer.push_str(&chunk);
                chunk
            }
        }
    }

    /// Apply multiple operations and return total chunk added
    pub fn apply_all(&mut self, ops: &[StableTextOp]) -> String {
        let mut appended = String::new();
        for op in ops {
            appended.push_str(&self.apply(op));
        }
        appended
    }

    /// Get the final assembled text
    pub fn finalize(&self) -> String {
        self.buffer.clone()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_text_assembler_basic() {
        let mut assembler = TextAssembler::new();

        // First line
        let op1 = StableTextOp::Line {
            y: 0,
            text: "Hello".to_string(),
            is_wrapped: false,
        };
        assert_eq!(assembler.apply(&op1), "Hello");

        // Append to same line
        let op2 = StableTextOp::Append {
            y: 0,
            text: " World".to_string(),
        };
        assert_eq!(assembler.apply(&op2), " World");

        // New line
        let op3 = StableTextOp::Line {
            y: 1,
            text: "Second line".to_string(),
            is_wrapped: false,
        };
        assert_eq!(assembler.apply(&op3), "\nSecond line");

        assert_eq!(assembler.finalize(), "Hello World\nSecond line");
    }

    #[test]
    fn test_text_assembler_wrapped_line() {
        let mut assembler = TextAssembler::new();

        let op1 = StableTextOp::Line {
            y: 0,
            text: "This is a very long line that".to_string(),
            is_wrapped: false,
        };
        assembler.apply(&op1);

        // Wrapped continuation - should NOT add newline
        let op2 = StableTextOp::Line {
            y: 1,
            text: " continues here".to_string(),
            is_wrapped: true,
        };
        assert_eq!(assembler.apply(&op2), " continues here");

        assert_eq!(
            assembler.finalize(),
            "This is a very long line that continues here"
        );
    }

    #[test]
    fn test_stable_line_detection() {
        let extractor = IncrementalExtractor::new(30, None);

        // Stable lines
        assert!(extractor.is_stable_line("Hello world"));
        assert!(extractor.is_stable_line("  Some code  "));
        assert!(extractor.is_stable_line("> user input here"));

        // Unstable lines
        assert!(!extractor.is_stable_line("·····"));
        assert!(!extractor.is_stable_line("────────"));
        assert!(!extractor.is_stable_line("Press esc to interrupt"));
        assert!(!extractor.is_stable_line("> "));
        assert!(!extractor.is_stable_line("❯ "));
        assert!(!extractor.is_stable_line(""));
        assert!(!extractor.is_stable_line("   "));

        // Spinner status lines (transient — animating every frame)
        assert!(!extractor.is_stable_line("✳ Determining…"));
        assert!(!extractor.is_stable_line("✢ Incubating… (3s · thinking)"));
        assert!(!extractor.is_stable_line("· Contemplating…"));
        assert!(!extractor.is_stable_line("  ✻ Processing…"));

        // Response lines starting with ⏺ should still be stable
        assert!(extractor.is_stable_line("⏺ Hello, I am Claude."));
        assert!(extractor.is_stable_line("⏺ Bash(command=\"ls\")"));
    }
}
