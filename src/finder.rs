//! Finds the word under the cursor: the detected text line at the cursor is read whole and split
//! into words. Nothing here depends on the kind of window or its layout.

use crate::bgr::{BgrImage, Rect};
use crate::ocr::Recognized;

/// Capture around the cursor, in 96-DPI pixels; scaled with the monitor DPI.
pub const CAPTURE_W: i32 = 640;
pub const CAPTURE_H: i32 = 192;
/// Detection upscale at 96 DPI: UI text of 11–16 px is detected best at about twice that size.
/// Dividing by the DPI factor keeps the model input at 1280×384 on every monitor.
pub const DET_SCALE_96: f32 = 2.0;

/// The cursor may miss a line by this share of its height and still pick it.
const LINE_TOLERANCE: f32 = 0.5;

// Word boundaries. Neither the blank gap between glyphs nor the model's space guess separates
// words alone: in tests/fixtures letters in monospace fonts or around "1" and "." leave gaps up
// to 0.32 line heights, bold text on busy backgrounds has spaces down to 0.14, and the model
// drops many spaces. The rules below combine both; tests/fixtures.rs shows what they still miss.
// Gaps are in line heights.
/// A space the model wrote counts if there is at least this much blank between the glyphs.
const GAP_FOR_WRITTEN_SPACE: f32 = 0.1;
/// A likely space: the model rates a space at least this high and the gap is this wide.
const SPACE_PROB: f32 = 0.05;
const GAP_FOR_LIKELY_SPACE: f32 = 0.2;
/// A gap this wide is a space whatever the model says.
const GAP_ALONE: f32 = 0.35;
/// Ideographs are square glyphs set evenly, so a smaller extra gap between two of them already
/// separates words (the model never writes spaces between them).
const GAP_ALONE_CJK: f32 = 0.25;
/// Full-width punctuation separates Chinese and Japanese words, which have no spaces.
const CJK_SEPARATORS: &[char] = &['，', '。', '、', '；', '：', '！', '？', '（', '）', '【', '】', '《', '》', '「', '」', '『', '』'];
/// The detected box can cut off underscores and descenders: recognize a little more below and
/// above it, never into the neighbouring line.
const EXTEND_DOWN: f32 = 0.2;
const EXTEND_UP: f32 = 0.1;
/// Below this contrast (0–255) between text and background columns, gaps are not measured.
const MIN_CONTRAST: f32 = 24.0;

/// Punctuation trimmed from the ends of a word, the way double-click selection does.
const EDGE_PUNCTUATION: &[char] = &[
    ',', '.', ';', ':', '!', '?', '"', '\'', '`', '«', '»', '“', '”', '„', '‘', '’', '(', ')', '[', ']', '{', '}', '<', '>',
    '|', '¦', '•', '·', '…', '，', '。', '、', '；', '：', '！', '？', '（', '）', '【', '】', '《', '》', '「', '」',
];

/// Capture rectangle around the cursor (kept on the screen) and the detection scale for it.
pub fn capture_region(screen: Rect, cx: i32, cy: i32, dpi: u32) -> (Rect, f32) {
    let k = dpi.max(48) as f32 / 96.0;
    let w = ((CAPTURE_W as f32 * k).round() as i32).clamp(1, screen.w.max(1));
    let h = ((CAPTURE_H as f32 * k).round() as i32).clamp(1, screen.h.max(1));
    let x = (cx - w / 2).clamp(screen.x, (screen.right() - w).max(screen.x));
    let y = (cy - h / 2).clamp(screen.y, (screen.bottom() - h).max(screen.y));
    (Rect { x, y, w, h }, DET_SCALE_96 / k)
}

/// The line under the cursor: one that contains it (the vertically closest if lines overlap),
/// otherwise the nearest one within LINE_TOLERANCE of its height.
pub fn line_at(lines: &[Rect], cx: i32, cy: i32) -> Option<Rect> {
    let (x, y) = (cx as f32, cy as f32);
    lines
        .iter()
        .map(|r| (r, distance(r, x, y), (r.y as f32 + r.h as f32 / 2.0 - y).abs()))
        .filter(|(r, d, _)| *d <= LINE_TOLERANCE * r.h as f32)
        .min_by(|a, b| a.1.total_cmp(&b.1).then(a.2.total_cmp(&b.2)))
        .map(|(r, ..)| *r)
}

/// `line` grown by EXTEND_UP/EXTEND_DOWN of its height, but not into other lines or off the image.
pub fn extend_line(line: Rect, lines: &[Rect], img: &BgrImage) -> Rect {
    let overlaps = |o: &&Rect| **o != line && o.x < line.right() && line.x < o.right();
    let above = lines.iter().filter(overlaps).map(|o| o.bottom()).filter(|&b| b <= line.y).max().unwrap_or(0);
    let below = lines.iter().filter(overlaps).map(|o| o.y).filter(|&t| t >= line.bottom()).min().unwrap_or(img.height as i32);
    let top = (line.y - (line.h as f32 * EXTEND_UP) as i32).max(above);
    let bottom = (line.bottom() + (line.h as f32 * EXTEND_DOWN) as i32).min(below);
    Rect { y: top, h: bottom - top, ..line }
}

/// The word nearest to `cx` in `rec`, read from `extended` (see `extend_line`) around the
/// detected `line`, without edge punctuation. Gaps are measured in heights of `line`: the
/// thresholds above were measured on detected boxes.
pub fn word_at(img: &BgrImage, line: Rect, extended: Rect, rec: &Recognized, cx: i32) -> String {
    let blanks = BlankColumns::measure(img, extended, line.h);
    // Characters other than spaces; a written space is remembered on the next character.
    let mut words: Vec<(String, f32, f32)> = Vec::new(); // text, first x, last x
    let mut prev: Option<(usize, &str, f32)> = None; // index, text and x of the previous character
    let mut after_separator = false;
    for (i, (ch, x)) in rec.chars.iter().enumerate() {
        if ch.trim().is_empty() {
            continue;
        }
        // Full-width punctuation, or ASCII punctuation the model wrote after an ideograph.
        let after_cjk = prev.is_some_and(|(_, pch, _)| is_cjk(pch));
        if ch.chars().all(|c| CJK_SEPARATORS.contains(&c) || (after_cjk && EDGE_PUNCTUATION.contains(&c))) {
            after_separator = true;
            prev = Some((i, ch, *x));
            continue;
        }
        let new_word = match prev {
            None => true,
            Some(_) if after_separator => true,
            Some((p, pch, px)) => {
                let written = i > p + 1;
                let prob = rec.space_between[p..i].iter().fold(0f32, |m, &v| m.max(v));
                let alone = if is_cjk(pch) && is_cjk(ch) { GAP_ALONE_CJK } else { GAP_ALONE };
                match blanks.as_ref().map(|b| b.widest(px, *x)) {
                    Some(gap) => (written && gap >= GAP_FOR_WRITTEN_SPACE) || (prob >= SPACE_PROB && gap >= GAP_FOR_LIKELY_SPACE) || gap >= alone,
                    None => written, // no contrast to measure gaps on: trust the model
                }
            }
        };
        if new_word {
            words.push((ch.clone(), *x, *x));
        } else if let Some(last) = words.last_mut() {
            last.0.push_str(ch);
            last.2 = *x;
        }
        prev = Some((i, ch, *x));
        after_separator = false;
    }
    let cx = cx as f32;
    words
        .into_iter()
        .min_by(|a, b| span_distance(a, cx).total_cmp(&span_distance(b, cx)))
        .map(|(w, ..)| clean(&w))
        .unwrap_or_default()
}

/// Strips punctuation and spaces from the ends, keeping inner ones ("1,250", "mr.robot").
pub fn clean(text: &str) -> String {
    text.trim_matches(|c: char| c.is_whitespace() || EDGE_PUNCTUATION.contains(&c)).to_owned()
}

/// Which columns of a line are blank. A glyph column mixes ink and background, so its luminance
/// range over the line's rows is large; a blank one is flat whatever the colours (light or dark
/// theme, gradient).
struct BlankColumns {
    line: Rect,
    /// Gaps are reported in units of this height.
    unit: i32,
    blank: Vec<bool>,
}

impl BlankColumns {
    /// `None` when text and background columns cannot be told apart (busy background).
    fn measure(img: &BgrImage, line: Rect, unit: i32) -> Option<Self> {
        let ranges: Vec<f32> = (line.x..line.right())
            .map(|x| {
                let (mut lo, mut hi) = (f32::MAX, f32::MIN);
                for y in line.y..line.bottom() {
                    let p = &img.data[(y as usize * img.width + x as usize) * 3..][..3];
                    let lum = 0.114 * p[0] as f32 + 0.587 * p[1] as f32 + 0.299 * p[2] as f32;
                    (lo, hi) = (lo.min(lum), hi.max(lum));
                }
                hi - lo
            })
            .collect();
        let mut sorted = ranges.clone();
        sorted.sort_by(f32::total_cmp);
        let (background, text) = (sorted[sorted.len() / 10], sorted[sorted.len() * 9 / 10]);
        if text - background < MIN_CONTRAST {
            return None;
        }
        let threshold = background + 0.3 * (text - background);
        Some(Self { line, unit: unit.max(1), blank: ranges.iter().map(|&r| r <= threshold).collect() })
    }

    /// Widest blank run between two x positions, in units.
    fn widest(&self, x0: f32, x1: f32) -> f32 {
        let from = (x0.max(0.0) as i32 - self.line.x).max(0) as usize;
        let to = ((x1 as i32 - self.line.x).max(0) as usize).min(self.blank.len());
        let (mut best, mut run) = (0, 0);
        for &blank in self.blank.get(from..to).unwrap_or(&[]) {
            run = if blank { run + 1 } else { 0 };
            best = best.max(run);
        }
        best as f32 / self.unit as f32
    }
}

/// Chinese, Japanese or Korean script.
fn is_cjk(text: &str) -> bool {
    text.chars().all(|c| matches!(c, '\u{3040}'..='\u{30FF}' | '\u{3400}'..='\u{4DBF}' | '\u{4E00}'..='\u{9FFF}' | '\u{AC00}'..='\u{D7AF}' | '\u{F900}'..='\u{FAFF}' | '\u{20000}'..='\u{2FFFF}'))
        && !text.is_empty()
}

fn span_distance(word: &(String, f32, f32), x: f32) -> f32 {
    (word.1 - x).max(0.0).max(x - word.2)
}

fn distance(r: &Rect, x: f32, y: f32) -> f32 {
    let dx = (r.x as f32 - x).max(0.0).max(x - r.right() as f32);
    let dy = (r.y as f32 - y).max(0.0).max(y - r.bottom() as f32);
    dx.hypot(dy)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rect(x: i32, y: i32, w: i32, h: i32) -> Rect {
        Rect { x, y, w, h }
    }

    #[test]
    fn capture_region_scales_with_dpi_and_stays_on_screen() {
        let screen = rect(-1920, 0, 1920, 1080);
        assert_eq!(capture_region(screen, -960, 540, 96), (rect(-1280, 444, 640, 192), 2.0));
        assert_eq!(capture_region(screen, -1915, 5, 144), (rect(-1920, 0, 960, 288), 2.0 / 1.5));
        assert_eq!(capture_region(screen, -1, 1079, 96).0, rect(-640, 888, 640, 192));
    }

    #[test]
    fn line_at_prefers_the_line_under_the_cursor() {
        let lines = [rect(10, 10, 100, 20), rect(10, 32, 100, 20), rect(300, 10, 50, 20)];
        assert_eq!(line_at(&lines, 50, 40), Some(lines[1]));
        assert_eq!(line_at(&lines, 50, 25), Some(lines[0]));
        assert_eq!(line_at(&lines, 50, 58), Some(lines[1])); // just below, within tolerance
        assert_eq!(line_at(&lines, 200, 20), None); // between lines horizontally
        assert_eq!(line_at(&lines, 50, 70), None);
    }

    /// Dark-on-light glyph blocks: "ab" 3 px apart, then "cd:" after a 9 px gap. Line height 36,
    /// so the gaps are 0.08 and 0.25 line heights.
    fn line_image() -> (BgrImage, Rect) {
        let (w, h) = (60usize, 36usize);
        let mut img = BgrImage { width: w, height: h, data: vec![230; w * h * 3] };
        for (x0, x1) in [(5, 10), (13, 18), (27, 32), (35, 40), (42, 43)] {
            for x in x0..x1 {
                for y in 5..31 {
                    img.data[(y * w + x) * 3..][..3].copy_from_slice(&[20, 20, 20]);
                }
            }
        }
        (img, rect(0, 0, 60, 36))
    }

    fn recognized(chars: &[(&str, f32)], space_between: &[f32]) -> Recognized {
        Recognized {
            text: chars.iter().map(|(c, _)| *c).collect(),
            score: 1.0,
            chars: chars.iter().map(|(c, x)| (c.to_string(), *x)).collect(),
            space_between: space_between.to_vec(),
        }
    }

    const CHARS: [(&str, f32); 5] = [("a", 7.0), ("b", 15.0), ("c", 29.0), ("d", 37.0), (":", 42.0)];

    #[test]
    fn splits_where_gap_and_model_agree() {
        let (img, line) = line_image();
        // The model dropped the space but rated it 0.3 over a 0.25 gap.
        let rec = recognized(&CHARS, &[0.0, 0.3, 0.0, 0.0]);
        assert_eq!(word_at(&img, line, line, &rec, 8), "ab");
        assert_eq!(word_at(&img, line, line, &rec, 30), "cd");
        assert_eq!(word_at(&img, line, line, &rec, 21), "ab"); // in the gap: the nearest word
        // Same gap, but the model sees no space there: one word (like "1" in "River1" or "x.y").
        let rec = recognized(&CHARS, &[0.0, 0.0, 0.0, 0.0]);
        assert_eq!(word_at(&img, line, line, &rec, 30), "abcd");
    }

    #[test]
    fn written_space_without_a_gap_is_ignored() {
        let (img, line) = line_image();
        // A space read between "a" and "b", only 0.08 line heights of blank apart.
        let rec = recognized(&[("a", 7.0), (" ", 11.0), ("b", 15.0)], &[0.0, 0.0]);
        assert_eq!(word_at(&img, line, line, &rec, 8), "ab");
    }

    #[test]
    fn flat_line_trusts_written_spaces_only() {
        let img = BgrImage { width: 30, height: 10, data: vec![100; 30 * 10 * 3] };
        let rec = recognized(&[("a", 2.0), ("b", 8.0), (" ", 12.0), ("c", 18.0)], &[0.9, 0.0, 0.0]);
        assert_eq!(word_at(&img, rect(0, 0, 30, 10), rect(0, 0, 30, 10), &rec, 3), "ab");
        assert_eq!(word_at(&img, rect(0, 0, 30, 10), rect(0, 0, 30, 10), &rec, 20), "c");
    }

    #[test]
    fn cjk_punctuation_separates_words() {
        let img = BgrImage { width: 50, height: 10, data: vec![100; 50 * 10 * 3] };
        let line = rect(0, 0, 50, 10);
        for comma in ["，", ","] {
            let rec = recognized(&[("你", 5.0), ("好", 15.0), (comma, 22.0), ("世", 30.0), ("界", 40.0)], &[0.0; 4]);
            assert_eq!(word_at(&img, line, line, &rec, 8), "你好");
            assert_eq!(word_at(&img, line, line, &rec, 35), "世界");
        }
        // An ASCII comma between digits stays inside the word.
        let rec = recognized(&[("1", 5.0), (",", 10.0), ("2", 15.0), ("5", 20.0)], &[0.0; 3]);
        assert_eq!(word_at(&img, line, line, &rec, 12), "1,25");
    }

    #[test]
    fn extend_line_stops_at_neighbours() {
        let img = BgrImage { width: 300, height: 100, data: vec![0; 300 * 100 * 3] };
        let lines = [rect(0, 0, 100, 20), rect(0, 24, 100, 20), rect(200, 30, 50, 20)];
        assert_eq!(extend_line(lines[1], &lines, &img), rect(0, 22, 100, 26)); // up to the line above
        assert_eq!(extend_line(lines[0], &lines, &img), rect(0, 0, 100, 24)); // down to the line below
    }

    #[test]
    fn clean_trims_edge_punctuation_only() {
        assert_eq!(clean(" (user_42), "), "user_42");
        assert_eq!(clean("«1,250»"), "1,250");
        assert_eq!(clean("mr.robot."), "mr.robot");
        assert_eq!(clean("-x_"), "-x_");
        assert_eq!(clean("你好世界："), "你好世界");
    }
}
