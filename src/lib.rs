//! Word-under-cursor recognition, shared by the Windows app, `--selftest` and the tests.

pub mod bgr;
pub mod finder;
pub mod ocr;
pub mod settings;

use bgr::{BgrImage, Rect};
use ocr::{Ocr, TEXT_SCORE};

pub type Result<T> = tract_onnx::prelude::TractResult<T>;

#[derive(Debug, Default)]
pub struct Outcome {
    pub word: Option<String>,
    // What led to it, for debug output.
    pub lines: Vec<Rect>,
    pub line: Option<Rect>,
    pub read: Option<(String, f32)>,
}

/// Reads the word under the cursor in a capture around it.
/// `cursor` is relative to `capture`; `scale` comes from `finder::capture_region`.
pub fn find_word(ocr: &mut Ocr, capture: &BgrImage, cursor: (i32, i32), scale: f32) -> Result<Outcome> {
    let mut out = Outcome { lines: ocr.detect(capture, scale)?, ..Outcome::default() };
    let Some(line) = finder::line_at(&out.lines, cursor.0, cursor.1) else {
        return Ok(out);
    };
    let extended = finder::extend_line(line, &out.lines, capture);
    let rec = ocr.recognize(capture, extended)?;
    if rec.score >= TEXT_SCORE {
        out.word = Some(finder::word_at(capture, line, extended, &rec, cursor.0)).filter(|w| !w.is_empty());
    }
    (out.line, out.read) = (Some(line), Some((rec.text, rec.score)));
    Ok(out)
}

/// What the app does on a hotkey press, applied to a screenshot taken at `dpi`.
pub fn find_in_image(ocr: &mut Ocr, shot: &BgrImage, cursor: (i32, i32), dpi: u32) -> Result<Outcome> {
    let screen = Rect { x: 0, y: 0, w: shot.width as i32, h: shot.height as i32 };
    let (region, scale) = finder::capture_region(screen, cursor.0, cursor.1, dpi);
    let capture = shot.crop(region);
    find_word(ocr, &capture, (cursor.0 - region.x, cursor.1 - region.y), scale)
}
