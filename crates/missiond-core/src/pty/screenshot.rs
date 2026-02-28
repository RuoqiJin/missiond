//! PTY Screenshot - Render terminal grid to PNG
//!
//! Captures the alacritty_terminal grid with colors and renders to an image.
//! Two-phase design: capture (under term mutex, fast) + render (no lock, slow).

use std::path::{Path, PathBuf};

use ab_glyph::{Font, FontRef, PxScale, ScaleFont};
use alacritty_terminal::grid::Dimensions;
use alacritty_terminal::index::Line;
use alacritty_terminal::term::cell::Flags;
use alacritty_terminal::term::Term;
use alacritty_terminal::vte::ansi::{Color, NamedColor};
use image::{ImageBuffer, Rgba, RgbaImage};
use once_cell::sync::OnceCell;
use tracing::warn;

/// Cell dimensions in pixels
const CELL_WIDTH: u32 = 10;
const CELL_HEIGHT: u32 = 22;
const FONT_SIZE: f32 = 18.0;
/// Vertical offset for glyph baseline within cell
const BASELINE_OFFSET: f32 = 17.0;
/// Minimum alpha for glyph rendering (boost thin strokes)
const MIN_GLYPH_ALPHA: u8 = 60;

/// Font search paths (macOS) - prefer Regular weight over Light
const FONT_PATHS: &[(&str, u32)] = &[
    ("/System/Library/Fonts/Menlo.ttc", 0),       // Menlo Regular (TTC index 0)
    ("/System/Library/Fonts/SFNSMono.ttf", 0),     // SF Mono Light (fallback)
];

// ========== Captured Data ==========

/// Cell data extracted from alacritty grid (no alacritty types leak out)
#[derive(Clone)]
struct CapturedCell {
    c: char,
    fg: [u8; 3],
    bg: [u8; 3],
    is_wide: bool,
    is_spacer: bool,
}

/// Full grid capture
pub struct CapturedGrid {
    rows: usize,
    cols: usize,
    cells: Vec<Vec<CapturedCell>>,
}

// ========== Color Resolution ==========

/// Default terminal background (dark theme)
const DEFAULT_BG: [u8; 3] = [30, 30, 46];
/// Default terminal foreground (light gray)
const DEFAULT_FG: [u8; 3] = [205, 214, 244];

/// Resolve alacritty Color to RGB bytes
fn resolve_color(color: Color, is_fg: bool) -> [u8; 3] {
    match color {
        Color::Spec(rgb) => [rgb.r, rgb.g, rgb.b],
        Color::Indexed(idx) => indexed_to_rgb(idx),
        Color::Named(name) => named_to_rgb(name, is_fg),
    }
}

/// xterm-256 ANSI 16 color palette
fn named_to_rgb(name: NamedColor, _is_fg: bool) -> [u8; 3] {
    match name {
        // Standard 8 colors
        NamedColor::Black => [0, 0, 0],
        NamedColor::Red => [204, 0, 0],
        NamedColor::Green => [78, 154, 6],
        NamedColor::Yellow => [196, 160, 0],
        NamedColor::Blue => [52, 101, 164],
        NamedColor::Magenta => [117, 80, 123],
        NamedColor::Cyan => [6, 152, 154],
        NamedColor::White => [211, 215, 207],
        // Bright colors
        NamedColor::BrightBlack => [85, 87, 83],
        NamedColor::BrightRed => [239, 41, 41],
        NamedColor::BrightGreen => [138, 226, 52],
        NamedColor::BrightYellow => [252, 233, 79],
        NamedColor::BrightBlue => [114, 159, 207],
        NamedColor::BrightMagenta => [173, 127, 168],
        NamedColor::BrightCyan => [52, 226, 226],
        NamedColor::BrightWhite => [238, 238, 236],
        // Special
        NamedColor::Foreground | NamedColor::BrightForeground => DEFAULT_FG,
        NamedColor::Background => DEFAULT_BG,
        NamedColor::Cursor => [248, 248, 242],
        // Dim variants (approximate)
        NamedColor::DimBlack => [40, 40, 40],
        NamedColor::DimRed => [153, 0, 0],
        NamedColor::DimGreen => [58, 115, 4],
        NamedColor::DimYellow => [147, 120, 0],
        NamedColor::DimBlue => [39, 76, 123],
        NamedColor::DimMagenta => [88, 60, 92],
        NamedColor::DimCyan => [4, 114, 115],
        NamedColor::DimWhite => [158, 161, 155],
        NamedColor::DimForeground => [158, 161, 155],
    }
}

/// 256-color indexed palette
fn indexed_to_rgb(idx: u8) -> [u8; 3] {
    match idx {
        // 0-15: same as named ANSI colors
        0 => [0, 0, 0],
        1 => [204, 0, 0],
        2 => [78, 154, 6],
        3 => [196, 160, 0],
        4 => [52, 101, 164],
        5 => [117, 80, 123],
        6 => [6, 152, 154],
        7 => [211, 215, 207],
        8 => [85, 87, 83],
        9 => [239, 41, 41],
        10 => [138, 226, 52],
        11 => [252, 233, 79],
        12 => [114, 159, 207],
        13 => [173, 127, 168],
        14 => [52, 226, 226],
        15 => [238, 238, 236],
        // 16-231: 6x6x6 color cube
        16..=231 => {
            let idx = idx - 16;
            let r = (idx / 36) as u8;
            let g = ((idx % 36) / 6) as u8;
            let b = (idx % 6) as u8;
            let to_val = |v: u8| if v == 0 { 0 } else { 55 + v * 40 };
            [to_val(r), to_val(g), to_val(b)]
        }
        // 232-255: grayscale ramp
        232..=255 => {
            let v = 8 + (idx - 232) * 10;
            [v, v, v]
        }
    }
}

// ========== Grid Capture ==========

/// Capture grid data from a locked Term (fast, minimal time under mutex)
pub fn capture_grid<T>(term: &Term<T>) -> CapturedGrid {
    let grid = term.grid();
    let cols = grid.columns();
    let rows = grid.screen_lines();

    let mut cells = Vec::with_capacity(rows);
    // Line(0) = first visible line, Line(rows-1) = last visible line
    for y in 0..rows {
        let line = Line(y as i32);
        let row = &grid[line];
        let mut row_cells = Vec::with_capacity(cols);
        for cell in row.into_iter().take(cols) {
            let flags = cell.flags;
            let inverse = flags.contains(Flags::INVERSE);

            let (raw_fg, raw_bg) = (cell.fg, cell.bg);
            let (fg, bg) = if inverse {
                (
                    resolve_color(raw_bg, true),
                    resolve_color(raw_fg, false),
                )
            } else {
                (
                    resolve_color(raw_fg, true),
                    resolve_color(raw_bg, false),
                )
            };

            row_cells.push(CapturedCell {
                c: cell.c,
                fg,
                bg,
                is_wide: flags.contains(Flags::WIDE_CHAR),
                is_spacer: flags.contains(Flags::WIDE_CHAR_SPACER),
            });
        }
        cells.push(row_cells);
    }

    CapturedGrid { rows, cols, cells }
}

// ========== Font Loading ==========

static FONT_DATA: OnceCell<(Vec<u8>, u32)> = OnceCell::new();

fn load_font_data() -> (&'static [u8], u32) {
    let (data, idx) = FONT_DATA.get_or_init(|| {
        for (path, index) in FONT_PATHS {
            if let Ok(data) = std::fs::read(path) {
                tracing::info!(font_path = path, ttc_index = index, size = data.len(), "Loaded monospace font");
                return (data, *index);
            }
        }
        warn!("No system monospace font found, using fallback rendering");
        (Vec::new(), 0)
    });
    (data.as_slice(), *idx)
}

// ========== Rendering ==========

/// Render captured grid to PNG bytes
pub fn render_to_png(grid: &CapturedGrid) -> anyhow::Result<Vec<u8>> {
    let img_width = grid.cols as u32 * CELL_WIDTH;
    let img_height = grid.rows as u32 * CELL_HEIGHT;

    if img_width == 0 || img_height == 0 {
        anyhow::bail!("Empty grid");
    }

    let mut img: RgbaImage = ImageBuffer::new(img_width, img_height);

    // Fill with default background
    for pixel in img.pixels_mut() {
        *pixel = Rgba([DEFAULT_BG[0], DEFAULT_BG[1], DEFAULT_BG[2], 255]);
    }

    // Try to load font (supports TTC via index)
    let (font_bytes, ttc_index) = load_font_data();
    let font = if !font_bytes.is_empty() {
        // Try with TTC index first, then plain slice
        FontRef::try_from_slice_and_index(font_bytes, ttc_index)
            .or_else(|_| FontRef::try_from_slice(font_bytes))
            .map(|f| {
                tracing::debug!("Font parsed successfully");
                f
            })
            .map_err(|e| {
                warn!("Failed to parse font: {e}");
                e
            })
            .ok()
    } else {
        None
    };

    let scaled_font = font.as_ref().map(|f| f.as_scaled(PxScale::from(FONT_SIZE)));

    let mut char_count = 0u32;
    let mut glyph_ok = 0u32;
    let mut glyph_miss = 0u32;
    let mut fallback_count = 0u32;

    for (row_idx, row) in grid.cells.iter().enumerate() {
        for (col_idx, cell) in row.iter().enumerate() {
            if cell.is_spacer {
                continue;
            }

            let x_start = col_idx as u32 * CELL_WIDTH;
            let y_start = row_idx as u32 * CELL_HEIGHT;
            let cell_w = if cell.is_wide {
                CELL_WIDTH * 2
            } else {
                CELL_WIDTH
            };

            // Fill background
            for dy in 0..CELL_HEIGHT {
                for dx in 0..cell_w {
                    let px = x_start + dx;
                    let py = y_start + dy;
                    if px < img_width && py < img_height {
                        img.put_pixel(px, py, Rgba([cell.bg[0], cell.bg[1], cell.bg[2], 255]));
                    }
                }
            }

            // Render character
            if cell.c == ' ' || cell.c == '\0' {
                continue;
            }

            char_count += 1;
            if row_idx < 15 && char_count <= 30 {
                tracing::debug!(
                    row = row_idx, col = col_idx,
                    char = ?cell.c, fg = ?cell.fg, bg = ?cell.bg,
                    wide = cell.is_wide,
                    "Cell data"
                );
            }

            if let Some(ref sf) = scaled_font {
                render_glyph(&mut img, sf, cell, x_start, y_start, cell_w, &mut glyph_ok, &mut glyph_miss);
            } else {
                fallback_count += 1;
                render_block_fallback(&mut img, cell, x_start, y_start, cell_w);
            }
        }
    }

    tracing::info!(
        rows = grid.rows, cols = grid.cols,
        non_space_chars = char_count,
        glyph_rendered = glyph_ok,
        glyph_missed = glyph_miss,
        block_fallback = fallback_count,
        has_font = scaled_font.is_some(),
        "Screenshot render stats"
    );

    // Encode to PNG
    let mut buf = std::io::Cursor::new(Vec::new());
    img.write_to(&mut buf, image::ImageFormat::Png)?;

    Ok(buf.into_inner())
}

/// Render a glyph using ab_glyph
fn render_glyph<F: Font>(
    img: &mut RgbaImage,
    sf: &ab_glyph::PxScaleFont<&F>,
    cell: &CapturedCell,
    x_start: u32,
    y_start: u32,
    _cell_w: u32,
    ok_count: &mut u32,
    miss_count: &mut u32,
) {
    let glyph_id = sf.font().glyph_id(cell.c);
    let glyph = glyph_id.with_scale_and_position(
        PxScale::from(FONT_SIZE),
        ab_glyph::point(x_start as f32 + 1.0, y_start as f32 + BASELINE_OFFSET),
    );

    if let Some(outlined) = sf.font().outline_glyph(glyph) {
        *ok_count += 1;
        let fg = cell.fg;
        outlined.draw(|px, py, coverage| {
            if px < img.width() && py < img.height() {
                let raw_alpha = (coverage * 255.0) as u8;
                if raw_alpha > 0 {
                    // Boost thin strokes: apply minimum alpha for visible pixels
                    let alpha = raw_alpha.max(MIN_GLYPH_ALPHA);
                    let existing = img.get_pixel(px, py);
                    let blended = alpha_blend(fg, [existing[0], existing[1], existing[2]], alpha);
                    img.put_pixel(px, py, Rgba([blended[0], blended[1], blended[2], 255]));
                }
            }
        });
    } else {
        *miss_count += 1;
        render_block_fallback(img, cell, x_start, y_start, CELL_WIDTH);
    }
}

/// Fallback: draw a small colored rectangle for characters without glyphs
fn render_block_fallback(
    img: &mut RgbaImage,
    cell: &CapturedCell,
    x_start: u32,
    y_start: u32,
    cell_w: u32,
) {
    let margin = 2u32;
    for dy in margin..(CELL_HEIGHT - margin) {
        for dx in margin..(cell_w - margin) {
            let px = x_start + dx;
            let py = y_start + dy;
            if px < img.width() && py < img.height() {
                img.put_pixel(
                    px,
                    py,
                    Rgba([cell.fg[0], cell.fg[1], cell.fg[2], 180]),
                );
            }
        }
    }
}

/// Alpha blend foreground onto background
fn alpha_blend(fg: [u8; 3], bg: [u8; 3], alpha: u8) -> [u8; 3] {
    let a = alpha as u16;
    let inv_a = 255 - a;
    [
        ((fg[0] as u16 * a + bg[0] as u16 * inv_a) / 255) as u8,
        ((fg[1] as u16 * a + bg[1] as u16 * inv_a) / 255) as u8,
        ((fg[2] as u16 * a + bg[2] as u16 * inv_a) / 255) as u8,
    ]
}

// ========== File Output ==========

/// Render grid and save to file, return path
pub fn save_screenshot(grid: &CapturedGrid, output_dir: &Path, slot_id: &str) -> anyhow::Result<PathBuf> {
    std::fs::create_dir_all(output_dir)?;

    let timestamp = chrono::Utc::now().timestamp_millis();
    let filename = format!("{}-{}.png", slot_id, timestamp);
    let path = output_dir.join(&filename);

    let png_bytes = render_to_png(grid)?;
    std::fs::write(&path, &png_bytes)?;

    // Cleanup old screenshots (keep last 20 per slot)
    cleanup_old(output_dir, slot_id, 20);

    Ok(path)
}

/// Remove old screenshots beyond the keep limit
fn cleanup_old(dir: &Path, slot_id: &str, keep: usize) {
    let prefix = format!("{}-", slot_id);
    let mut files: Vec<_> = std::fs::read_dir(dir)
        .into_iter()
        .flatten()
        .filter_map(|e| e.ok())
        .filter(|e| {
            e.file_name()
                .to_string_lossy()
                .starts_with(&prefix)
                && e.file_name().to_string_lossy().ends_with(".png")
        })
        .collect();

    if files.len() <= keep {
        return;
    }

    files.sort_by_key(|e| e.metadata().ok().and_then(|m| m.modified().ok()));
    for f in &files[..files.len() - keep] {
        std::fs::remove_file(f.path()).ok();
    }
}
