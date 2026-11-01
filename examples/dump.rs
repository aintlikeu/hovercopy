//! Prints every text line OCR reads on whole images (detection scale as at 96 DPI).
//! `cargo run --profile fast --example dump -- image.png ...`

use std::path::Path;
use std::time::Instant;

use hover_copy::bgr::BgrImage;
use hover_copy::ocr::Ocr;

fn main() {
    let mut ocr = Ocr::new().expect("models");
    for path in std::env::args().skip(1) {
        let img = BgrImage::load(Path::new(&path)).expect("image");
        let started = Instant::now();
        let lines = ocr.read(&img, 2.0).expect("ocr");
        println!("== {path}: {} lines, {:?}", lines.len(), started.elapsed());
        for l in lines {
            let r = l.rect;
            println!("  [{:4},{:4} .. {:4},{:4}] {:.2}  {}", r.x, r.y, r.right(), r.bottom(), l.score, l.text);
        }
    }
}
