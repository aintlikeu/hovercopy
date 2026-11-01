//! Word under the cursor on synthetic screenshots (tests/fixtures, made by tools/make_fixtures.py):
//! words in several fonts and sizes on light, dark, coloured, gradient, noisy and photo-like
//! backgrounds. Run with `cargo test --profile fast -- --nocapture` to see the per-background table.
//! Debug builds of tract are far too slow for this test.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::thread;

use hover_copy::bgr::BgrImage;
use hover_copy::ocr::Ocr;

/// Share of probes that must come out exactly, per background: what this version achieves.
/// The misses: the model itself misreads 11–16 px text on noise (underscores dropped, a lost
/// digit), "1" as "l" on the photo and a "." in 11 px narrow font; 11 px monospace and serif
/// fonts get a word split at "-" or between letters; at 11 px a single space between Chinese
/// words is not seen. Cyrillic is not in the model's alphabet and is listed only to show it.
const REQUIRED: &[(&str, f32)] = &[
    ("light", 1.0),
    ("dark", 1.0),
    ("dark-muted", 0.95),
    ("blue", 0.95),
    ("yellow", 0.96),
    ("gradient-h", 1.0),
    ("gradient-v", 1.0),
    ("noise", 0.82),
    ("photo", 0.96),
    ("low-contrast", 1.0),
    ("cjk", 0.85),
    ("cyrillic", 0.0),
];

struct Probe {
    image: String,
    cursor: (i32, i32),
    expected: Option<String>,
    label: String,
}

fn fixtures() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures")
}

fn probes() -> Vec<Probe> {
    let tsv = std::fs::read_to_string(fixtures().join("words.tsv")).unwrap();
    let mut probes = Vec::new();
    let mut images = Vec::new();
    for row in tsv.lines().skip(1) {
        let f: Vec<&str> = row.split('\t').collect();
        let n = |i: usize| f[i].parse::<f32>().unwrap();
        let (x0, y0, x1, y1) = (n(1), n(2), n(3), n(4));
        let label = format!("{} {} {}px", f[6], f[7], f[8]);
        for (part, fx) in [("centre", 0.5), ("left", 0.2)] {
            let cursor = ((x0 + (x1 - x0) * fx) as i32, ((y0 + y1) / 2.0) as i32);
            probes.push(Probe { image: f[0].into(), cursor, expected: Some(f[5].into()), label: format!("{label} {part}") });
        }
        if !images.contains(&f[0]) {
            images.push(f[0]);
        }
    }
    // Blank spots right of the text: nothing to copy.
    for image in images {
        for y in [8, 200] {
            probes.push(Probe { image: image.into(), cursor: (745, y), expected: None, label: format!("{image} blank") });
        }
    }
    probes
}

#[test]
fn words_on_varied_backgrounds() {
    let probes = probes();
    let mut images = BTreeMap::new();
    for p in &probes {
        images.entry(p.image.clone()).or_insert_with(|| BgrImage::load(&fixtures().join(&p.image)).unwrap());
    }
    let workers = thread::available_parallelism().map_or(2, |n| n.get().min(4));
    let chunk = probes.len().div_ceil(workers);
    let got: Vec<Option<String>> = thread::scope(|s| {
        let handles: Vec<_> = probes
            .chunks(chunk)
            .map(|part| {
                let images = &images;
                s.spawn(move || {
                    let mut ocr = Ocr::new().unwrap();
                    part.iter().map(|p| hover_copy::find_in_image(&mut ocr, &images[&p.image], p.cursor, 96).unwrap().word).collect::<Vec<_>>()
                })
            })
            .collect();
        handles.into_iter().flat_map(|h| h.join().unwrap()).collect()
    });

    let mut stats: BTreeMap<String, (usize, usize)> = BTreeMap::new();
    let mut failures = Vec::new();
    for (p, got) in probes.iter().zip(&got) {
        let category = p.image.trim_end_matches(".png").to_owned();
        let entry = stats.entry(category).or_default();
        entry.1 += 1;
        if *got == p.expected {
            entry.0 += 1;
        } else {
            failures.push(format!("{} at {:?}: expected {:?}, got {:?}", p.label, p.cursor, p.expected, got));
        }
    }
    println!("{:<14} {:>9}", "background", "correct");
    for (category, (ok, total)) in &stats {
        println!("{category:<14} {ok:>4}/{total:<4}");
    }
    println!("failures:\n{}", failures.join("\n"));

    let mut below = Vec::new();
    for (category, required) in REQUIRED {
        let (ok, total) = stats[*category];
        if (ok as f32) < required * total as f32 {
            below.push(format!("{category}: {ok}/{total}, required {:.0}%", required * 100.0));
        }
    }
    assert!(below.is_empty(), "below the required share:\n{}", below.join("\n"));
}
