//! PP-OCRv4 text detection + recognition on tract (pure Rust, no DLLs).
//!
//! Mirrors the pipeline of RapidOCR 1.4.4, which the results were checked against, except that
//! boxes are axis-aligned rectangles: screen text is never rotated, so OpenCV is not needed.

use std::collections::HashMap;
use std::io::Cursor;

use tract_onnx::prelude::*;

use crate::bgr::{resize_normalized, BgrImage, Rect};

const DET_MODEL: &[u8] = include_bytes!("../models/det.onnx");
const REC_MODEL: &[u8] = include_bytes!("../models/rec.onnx");
const KEYS: &str = include_str!("../models/keys.txt");

// RapidOCR config.yaml values.
const DET_THRESH: f32 = 0.3;
const DET_BOX_THRESH: f32 = 0.5;
const DET_UNCLIP_RATIO: f32 = 1.6;
const DET_MIN_SIZE: f32 = 3.0;
const REC_HEIGHT: usize = 48;
const REC_MIN_WIDTH: usize = 320;
pub const TEXT_SCORE: f32 = 0.5;

/// tract optimizes a model for one input shape (~150 ms), so plans are cached per shape.
const PLAN_CACHE_LIMIT: usize = 8;

type Plan = std::sync::Arc<TypedRunnableModel>;

#[derive(Clone, Debug, PartialEq)]
pub struct Line {
    pub rect: Rect,
    pub text: String,
    pub score: f32,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Recognized {
    pub text: String,
    pub score: f32,
    /// Each decoded character and the x of its centre in image coordinates.
    pub chars: Vec<(String, f32)>,
    /// For each pair of neighbouring characters, the highest probability of a space between
    /// them. The model often loses spaces to "blank" in the text, but still rates them here.
    pub space_between: Vec<f32>,
}

pub struct Ocr {
    det: InferenceModel,
    rec: InferenceModel,
    det_plans: HashMap<(usize, usize), Plan>,
    rec_plans: HashMap<usize, Plan>,
    /// CTC classes: blank, dictionary, space.
    chars: Vec<String>,
}

impl Ocr {
    pub fn new() -> TractResult<Self> {
        let det = tract_onnx::onnx().model_for_read(&mut Cursor::new(DET_MODEL))?;
        let rec = tract_onnx::onnx().model_for_read(&mut Cursor::new(REC_MODEL))?;
        let chars = std::iter::once(String::new())
            .chain(KEYS.lines().map(str::to_owned))
            .chain(std::iter::once(" ".to_owned()))
            .collect();
        let mut ocr = Self { det, rec, det_plans: HashMap::new(), rec_plans: HashMap::new(), chars };
        // Short lines and most others.
        ocr.rec_plan(REC_MIN_WIDTH)?;
        ocr.rec_plan(REC_MIN_WIDTH * 2)?;
        Ok(ocr)
    }

    /// Optimizes detection for an input size ahead of time, so the first request does not wait.
    pub fn prepare_detection(&mut self, w: usize, h: usize, scale: f32) -> TractResult<()> {
        let (w, h) = (det_side(w, scale), det_side(h, scale));
        self.det_plan(h, w).map(|_| ())
    }

    /// All text lines of the image (for diagnostics).
    pub fn read(&mut self, img: &BgrImage, scale: f32) -> TractResult<Vec<Line>> {
        let mut lines = Vec::new();
        for rect in self.detect(img, scale)? {
            let rec = self.recognize(img, rect)?;
            if rec.score >= TEXT_SCORE && !rec.text.trim().is_empty() {
                lines.push(Line { rect, text: rec.text, score: rec.score });
            }
        }
        Ok(lines)
    }

    /// Text line rectangles; the image is resized by `scale` for the model.
    pub fn detect(&mut self, img: &BgrImage, scale: f32) -> TractResult<Vec<Rect>> {
        let (w, h) = (det_side(img.width, scale), det_side(img.height, scale));
        let mut input = vec![0f32; 3 * w * h];
        let whole = Rect { x: 0, y: 0, w: img.width as i32, h: img.height as i32 };
        resize_normalized(img, whole, w, h, w, &mut input);

        let plan = self.det_plan(h, w)?;
        let out = plan.run(tvec!(Tensor::from_shape(&[1, 3, h, w], &input)?.into()))?;
        let prob = out[0].to_plain_array_view::<f32>()?;
        let prob = prob.as_slice().ok_or_else(|| TractError::msg("non-contiguous detection output"))?;
        Ok(boxes_from_prob(prob, w, h, img.width, img.height))
    }

    /// Reads the text in `r` (a single line).
    pub fn recognize(&mut self, img: &BgrImage, r: Rect) -> TractResult<Recognized> {
        let ratio = r.w as f64 / r.h as f64;
        let img_w = REC_MIN_WIDTH.max((REC_HEIGHT as f64 * ratio) as usize);
        let resized_w = ((REC_HEIGHT as f64 * ratio).ceil() as usize).clamp(1, img_w);
        // Longer lines get zero padding up to 640, 1280, …: a handful of cached plans covers
        // every width.
        let plan_w = REC_MIN_WIDTH << img_w.div_ceil(REC_MIN_WIDTH).next_power_of_two().trailing_zeros();

        let mut input = vec![0f32; 3 * REC_HEIGHT * plan_w];
        resize_normalized(img, r, resized_w, REC_HEIGHT, plan_w, &mut input);
        let plan = self.rec_plan(plan_w)?;
        let out = plan.run(tvec!(Tensor::from_shape(&[1, 3, REC_HEIGHT, plan_w], &input)?.into()))?;
        let probs = out[0].to_plain_array_view::<f32>()?;
        let (steps, classes) = (probs.shape()[1], probs.shape()[2]);
        let probs = probs.as_slice().ok_or_else(|| TractError::msg("non-contiguous recognition output"))?;
        let decoded = ctc_decode(probs, steps, classes, &self.chars);
        // A step covers plan_w / steps model pixels; the resized text spans resized_w of them.
        let step_px = plan_w as f32 / steps as f32 * r.w as f32 / resized_w as f32;
        let chars = decoded.chars.into_iter().map(|(ch, step)| (ch, r.x as f32 + (step as f32 + 0.5) * step_px)).collect();
        Ok(Recognized { text: decoded.text, score: decoded.score, chars, space_between: decoded.space_between })
    }

    fn det_plan(&mut self, h: usize, w: usize) -> TractResult<&Plan> {
        if !self.det_plans.contains_key(&(h, w)) {
            if self.det_plans.len() >= PLAN_CACHE_LIMIT {
                self.det_plans.clear();
            }
            let plan = optimize(&self.det, [1, 3, h, w])?;
            self.det_plans.insert((h, w), plan);
        }
        Ok(&self.det_plans[&(h, w)])
    }

    fn rec_plan(&mut self, w: usize) -> TractResult<&Plan> {
        if !self.rec_plans.contains_key(&w) {
            if self.rec_plans.len() >= PLAN_CACHE_LIMIT {
                self.rec_plans.clear();
            }
            let plan = optimize(&self.rec, [1, 3, REC_HEIGHT, w])?;
            self.rec_plans.insert(w, plan);
        }
        Ok(&self.rec_plans[&w])
    }
}

/// Model input side for an image side: scaled, then rounded to a multiple of 32 like RapidOCR.
fn det_side(len: usize, scale: f32) -> usize {
    let scaled = (len as f64 * scale as f64) as usize;
    ((scaled as f64 / 32.0).round_ties_even() as usize * 32).max(32)
}

fn optimize(model: &InferenceModel, shape: [usize; 4]) -> TractResult<Plan> {
    model.clone().with_input_fact(0, f32::fact(shape).into())?.into_optimized()?.into_runnable()
}

/// DB post-processing: probability map (`pw`×`ph`) → text rectangles in image coordinates.
fn boxes_from_prob(prob: &[f32], pw: usize, ph: usize, dest_w: usize, dest_h: usize) -> Vec<Rect> {
    let bin: Vec<bool> = prob.iter().map(|&p| p > DET_THRESH).collect();
    // cv2.dilate with a 2×2 kernel: the anchor is the kernel centre, so each pixel takes the
    // max of itself and its left/top neighbours.
    let mut mask = vec![false; pw * ph];
    for y in 0..ph {
        for x in 0..pw {
            let at = |dx: usize, dy: usize| x >= dx && y >= dy && bin[(y - dy) * pw + x - dx];
            mask[y * pw + x] = at(0, 0) || at(1, 0) || at(0, 1) || at(1, 1);
        }
    }

    let (sx, sy) = (dest_w as f32 / pw as f32, dest_h as f32 / ph as f32);
    let mut seen = vec![false; pw * ph];
    let mut stack = Vec::new();
    let mut rects = Vec::new();
    for start in 0..pw * ph {
        if !mask[start] || seen[start] {
            continue;
        }
        // 8-connected component, like the outer contour cv2.findContours would trace.
        seen[start] = true;
        stack.push(start);
        let (mut x0, mut y0, mut x1, mut y1) = (usize::MAX, usize::MAX, 0, 0);
        while let Some(i) = stack.pop() {
            let (x, y) = (i % pw, i / pw);
            (x0, y0, x1, y1) = (x0.min(x), y0.min(y), x1.max(x), y1.max(y));
            for ny in y.saturating_sub(1)..=(y + 1).min(ph - 1) {
                for nx in x.saturating_sub(1)..=(x + 1).min(pw - 1) {
                    let j = ny * pw + nx;
                    if mask[j] && !seen[j] {
                        seen[j] = true;
                        stack.push(j);
                    }
                }
            }
        }

        // Box extent between pixel centres, as cv2.minAreaRect measures contour points.
        let (bw, bh) = ((x1 - x0) as f32, (y1 - y0) as f32);
        if bw.min(bh) < DET_MIN_SIZE {
            continue;
        }
        let mut sum = 0.0;
        for y in y0..=y1 {
            sum += prob[y * pw + x0..=y * pw + x1].iter().sum::<f32>();
        }
        if sum / (((x1 - x0 + 1) * (y1 - y0 + 1)) as f32) < DET_BOX_THRESH {
            continue;
        }
        // Unclip: grow the box by area * ratio / perimeter on every side.
        let d = bw * bh * DET_UNCLIP_RATIO / (2.0 * (bw + bh));
        if (bw + 2.0 * d).min(bh + 2.0 * d) < DET_MIN_SIZE + 2.0 {
            continue;
        }
        let fx = |v: f32| ((v * sx).round().clamp(0.0, dest_w as f32) as i32).min(dest_w as i32 - 1);
        let fy = |v: f32| ((v * sy).round().clamp(0.0, dest_h as f32) as i32).min(dest_h as i32 - 1);
        let (l, t) = (fx(x0 as f32 - d), fy(y0 as f32 - d));
        let (r, b) = (fx(x1 as f32 + d), fy(y1 as f32 + d));
        if r - l > 3 && b - t > 3 {
            rects.push(Rect { x: l, y: t, w: r - l, h: b - t });
        }
    }
    rects
}

struct Decoded {
    text: String,
    /// Mean confidence of the decoded characters.
    score: f32,
    /// Each character with the step it was read at.
    chars: Vec<(String, usize)>,
    space_between: Vec<f32>,
}

/// Greedy CTC decoding: best class per step, collapse repeats, drop blanks.
/// The last class is the space.
fn ctc_decode(probs: &[f32], steps: usize, classes: usize, chars: &[String]) -> Decoded {
    let mut out = Decoded { text: String::new(), score: 0.0, chars: Vec::new(), space_between: Vec::new() };
    let mut total = 0.0;
    let mut prev = usize::MAX;
    let mut space = 0f32; // highest space probability since the last character
    for (step, row) in probs.chunks_exact(classes).take(steps).enumerate() {
        let (idx, p) = row.iter().copied().enumerate().fold((0, f32::MIN), |best, (i, v)| if v > best.1 { (i, v) } else { best });
        if idx != 0 && idx != prev {
            if let Some(ch) = chars.get(idx) {
                if !out.chars.is_empty() {
                    out.space_between.push(space);
                }
                out.text.push_str(ch);
                out.chars.push((ch.clone(), step));
                total += p;
                space = 0.0;
            }
        } else {
            space = space.max(row[classes - 1]);
        }
        prev = idx;
    }
    out.score = if out.chars.is_empty() { 0.0 } else { total / out.chars.len() as f32 };
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ctc_collapses_repeats_and_blanks() {
        // Classes: blank, a, space. Steps: a, a, blank (space 0.3), a, space, a.
        let chars: Vec<String> = ["", "a", " "].iter().map(|s| s.to_string()).collect();
        let probs = [0.1, 0.9, 0.0, 0.1, 0.8, 0.1, 0.6, 0.1, 0.3, 0.2, 0.7, 0.1, 0.1, 0.0, 0.9, 0.0, 0.8, 0.2];
        let d = ctc_decode(&probs, 6, 3, &chars);
        assert_eq!(d.text, "aa a");
        assert!((d.score - (0.9 + 0.7 + 0.9 + 0.8) / 4.0).abs() < 1e-6);
        let steps: Vec<usize> = d.chars.iter().map(|(_, s)| *s).collect();
        assert_eq!(steps, [0, 3, 4, 5]);
        assert_eq!(d.space_between, [0.3, 0.0, 0.0]);
    }

    #[test]
    fn det_side_rounds_to_32() {
        assert_eq!(det_side(640, 2.0), 1280);
        assert_eq!(det_side(1120, 2.0 / 1.75), 1280);
        assert_eq!(det_side(5, 1.0), 32);
    }
}
