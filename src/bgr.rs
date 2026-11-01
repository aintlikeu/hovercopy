//! BGR pixel buffer shared by screen capture, OCR and tests.
//!
//! BGR because PaddleOCR models were trained on OpenCV images, and Windows screen capture
//! delivers BGRA anyway.

use std::path::Path;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Rect {
    pub x: i32,
    pub y: i32,
    pub w: i32,
    pub h: i32,
}

impl Rect {
    pub fn right(&self) -> i32 {
        self.x + self.w
    }

    pub fn bottom(&self) -> i32 {
        self.y + self.h
    }

    pub fn intersect(&self, other: &Rect) -> Option<Rect> {
        let x = self.x.max(other.x);
        let y = self.y.max(other.y);
        let r = self.right().min(other.right());
        let b = self.bottom().min(other.bottom());
        (r > x && b > y).then_some(Rect { x, y, w: r - x, h: b - y })
    }
}

#[derive(Clone)]
pub struct BgrImage {
    pub width: usize,
    pub height: usize,
    pub data: Vec<u8>,
}

impl BgrImage {
    pub fn from_bgra(width: usize, height: usize, bgra: &[u8]) -> Self {
        let data = bgra.chunks_exact(4).flat_map(|p| [p[0], p[1], p[2]]).collect();
        Self { width, height, data }
    }

    pub fn load(path: &Path) -> Result<Self, String> {
        let rgb = image::open(path).map_err(|e| format!("{}: {e}", path.display()))?.to_rgb8();
        let (width, height) = (rgb.width() as usize, rgb.height() as usize);
        let data = rgb.into_raw().chunks_exact(3).flat_map(|p| [p[2], p[1], p[0]]).collect();
        Ok(Self { width, height, data })
    }

    pub fn save_png(&self, path: &Path) -> Result<(), String> {
        let rgb = self.data.chunks_exact(3).flat_map(|p| [p[2], p[1], p[0]]).collect();
        image::RgbImage::from_raw(self.width as u32, self.height as u32, rgb)
            .ok_or("bad image size")?
            .save(path)
            .map_err(|e| format!("{}: {e}", path.display()))
    }

    /// `r` must lie inside the image.
    pub fn crop(&self, r: Rect) -> Self {
        let (x, w) = (r.x as usize, r.w as usize);
        let mut data = Vec::with_capacity(w * r.h as usize * 3);
        for y in r.y as usize..r.bottom() as usize {
            let row = (y * self.width + x) * 3;
            data.extend_from_slice(&self.data[row..row + w * 3]);
        }
        Self { width: w, height: r.h as usize, data }
    }
}

/// Bilinear resize of `region` to `dst_w`×`dst_h` (OpenCV INTER_LINEAR sampling), normalized
/// to [-1, 1] and written as planar BGR into `out` (3 × `dst_h` × `stride`); columns past
/// `dst_w` are left untouched (zero padding).
pub fn resize_normalized(src: &BgrImage, region: Rect, dst_w: usize, dst_h: usize, stride: usize, out: &mut [f32]) {
    let xs = sample_axis(region.x as usize, region.w as usize, dst_w);
    let ys = sample_axis(region.y as usize, region.h as usize, dst_h);
    let plane = dst_h * stride;
    for (dy, &(y0, y1, fy)) in ys.iter().enumerate() {
        let (row0, row1) = (y0 * src.width, y1 * src.width);
        for (dx, &(x0, x1, fx)) in xs.iter().enumerate() {
            for c in 0..3 {
                let p = |row: usize, x: usize| src.data[(row + x) * 3 + c] as f32;
                let top = p(row0, x0) + (p(row0, x1) - p(row0, x0)) * fx;
                let bottom = p(row1, x0) + (p(row1, x1) - p(row1, x0)) * fx;
                let v = top + (bottom - top) * fy;
                out[c * plane + dy * stride + dx] = v / 127.5 - 1.0;
            }
        }
    }
}

/// Source pixel pair and weight for each destination pixel, with pixel-centre alignment.
fn sample_axis(start: usize, src_len: usize, dst_len: usize) -> Vec<(usize, usize, f32)> {
    let scale = src_len as f32 / dst_len as f32;
    (0..dst_len)
        .map(|d| {
            let f = ((d as f32 + 0.5) * scale - 0.5).max(0.0);
            let i0 = (f as usize).min(src_len - 1);
            let i1 = (i0 + 1).min(src_len - 1);
            (start + i0, start + i1, f - i0 as f32)
        })
        .collect()
}
