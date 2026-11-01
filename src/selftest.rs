//! `--selftest IMAGE X Y [DPI]`: runs the hotkey pipeline on a screenshot instead of the screen.
//! Prints the report and writes it to selftest_result.txt (a GUI exe has no console);
//! on Windows it also round-trips the word through the clipboard.

use std::path::Path;
use std::time::Instant;

use hover_copy::bgr::BgrImage;
use hover_copy::ocr::Ocr;

pub fn run(args: &[String]) -> i32 {
    let (report, code) = match report(args) {
        Ok(r) => (r, 0),
        Err(e) => (format!("error: {e}\n"), 1),
    };
    print!("{report}");
    let _ = std::fs::write("selftest_result.txt", &report);
    code
}

fn report(args: &[String]) -> Result<String, String> {
    let (path, x, y, dpi) = match args {
        [path, x, y] => (path, x, y, "96"),
        [path, x, y, dpi] => (path, x, y, dpi.as_str()),
        _ => return Err("usage: HoverCopy --selftest IMAGE X Y [DPI]".into()),
    };
    let number = |s: &str| s.parse::<i32>().map_err(|_| format!("bad number «{s}»"));
    let cursor = (number(x)?, number(y)?);
    let dpi = number(dpi)?.clamp(48, 480) as u32;
    let shot = BgrImage::load(Path::new(path))?;

    let started = Instant::now();
    let mut ocr = Ocr::new().map_err(|e| format!("OCR init: {e}"))?;
    let load_ms = started.elapsed().as_millis();
    let started = Instant::now();
    let out = hover_copy::find_in_image(&mut ocr, &shot, cursor, dpi).map_err(|e| format!("OCR: {e}"))?;
    let find_ms = started.elapsed().as_millis();

    let mut r = format!("word: {}\nload_ms: {load_ms}\nfind_ms: {find_ms}\n", out.word.as_deref().unwrap_or("NONE"));
    #[cfg(windows)]
    if let Some(word) = &out.word {
        r += &format!("clipboard: {}\n", crate::win::clipboard_roundtrip(word));
    }
    r += &format!("line: {:?}\nread: {:?}\nlines found: {}\n", out.line, out.read, out.lines.len());
    Ok(r)
}
