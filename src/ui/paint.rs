//! A tiny anti-aliased software painter: rounded rectangles, circles and
//! coverage blending into a 0RGB pixel buffer. Coordinates are logical pixels;
//! `scale` maps them to physical pixels.

// Drawing calls take position, size and style together.
#![allow(clippy::too_many_arguments)]

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Rgb(pub u8, pub u8, pub u8);

impl Rgb {
    pub const fn hex(v: u32) -> Rgb {
        Rgb((v >> 16) as u8, (v >> 8) as u8, v as u8)
    }
}

pub struct Canvas {
    pub w: usize,
    pub h: usize,
    pub scale: f32,
    pub px: Vec<u32>,
}

impl Canvas {
    pub fn new(w: usize, h: usize, scale: f32) -> Canvas {
        Canvas {
            w,
            h,
            scale,
            px: vec![0; w * h],
        }
    }

    pub fn clear(&mut self, c: Rgb) {
        let v = (c.0 as u32) << 16 | (c.1 as u32) << 8 | c.2 as u32;
        self.px.fill(v);
    }

    /// Blend `c` at physical pixel (x, y) with coverage `a` in 0..=1.
    #[inline]
    pub fn blend(&mut self, x: i32, y: i32, c: Rgb, a: f32) {
        if x < 0 || y < 0 || x as usize >= self.w || y as usize >= self.h || a <= 0.0 {
            return;
        }
        let i = y as usize * self.w + x as usize;
        let p = self.px[i];
        let a = a.min(1.0);
        let mix = |dst: u32, src: u8| (dst as f32 + (src as f32 - dst as f32) * a) as u32;
        let r = mix(p >> 16 & 255, c.0);
        let g = mix(p >> 8 & 255, c.1);
        let b = mix(p & 255, c.2);
        self.px[i] = r << 16 | g << 8 | b;
    }

    /// Filled rounded rectangle (logical coordinates), with opacity.
    pub fn rrect_a(&mut self, x: f32, y: f32, w: f32, h: f32, r: f32, c: Rgb, alpha: f32) {
        let s = self.scale;
        let (x, y, w, h, r) = (
            x * s,
            y * s,
            w * s,
            h * s,
            (r * s).min(w * s / 2.0).min(h * s / 2.0),
        );
        if w <= 0.0 || h <= 0.0 {
            return;
        }
        let (cx, cy, hw, hh) = (x + w / 2.0, y + h / 2.0, w / 2.0, h / 2.0);
        let x0 = x.floor().max(0.0) as i32;
        let y0 = y.floor().max(0.0) as i32;
        let x1 = (x + w).ceil().min(self.w as f32) as i32;
        let y1 = (y + h).ceil().min(self.h as f32) as i32;
        for py in y0..y1 {
            let qy = (py as f32 + 0.5 - cy).abs() - (hh - r);
            for px in x0..x1 {
                let qx = (px as f32 + 0.5 - cx).abs() - (hw - r);
                let outside = (qx.max(0.0).powi(2) + qy.max(0.0).powi(2)).sqrt();
                let d = outside + qx.max(qy).min(0.0) - r;
                let cov = (0.5 - d).clamp(0.0, 1.0);
                if cov > 0.0 {
                    self.blend(px, py, c, cov * alpha);
                }
            }
        }
    }

    pub fn rrect(&mut self, x: f32, y: f32, w: f32, h: f32, r: f32, c: Rgb) {
        self.rrect_a(x, y, w, h, r, c, 1.0);
    }

    /// Rounded rectangle with a 1px border.
    pub fn panel(&mut self, x: f32, y: f32, w: f32, h: f32, r: f32, fill: Rgb, border: Rgb) {
        self.rrect(x, y, w, h, r, border);
        self.rrect(x + 1.0, y + 1.0, w - 2.0, h - 2.0, (r - 1.0).max(0.0), fill);
    }

    pub fn circle(&mut self, cx: f32, cy: f32, r: f32, c: Rgb) {
        self.rrect(cx - r, cy - r, r * 2.0, r * 2.0, r, c);
    }

    /// Ring arc from 12 o'clock, clockwise, filled to `frac`.
    pub fn ring(&mut self, cx: f32, cy: f32, r: f32, width: f32, frac: f32, fg: Rgb, bg: Rgb) {
        let s = self.scale;
        let (cx, cy, r_out) = (cx * s, cy * s, r * s);
        let r_in = r_out - width * s;
        let x0 = (cx - r_out - 1.0).max(0.0) as i32;
        let y0 = (cy - r_out - 1.0).max(0.0) as i32;
        let x1 = (cx + r_out + 1.0).min(self.w as f32) as i32;
        let y1 = (cy + r_out + 1.0).min(self.h as f32) as i32;
        for py in y0..y1 {
            for px in x0..x1 {
                let (dx, dy) = (px as f32 + 0.5 - cx, py as f32 + 0.5 - cy);
                let d = (dx * dx + dy * dy).sqrt();
                let cov = (r_out + 0.5 - d).min(d - r_in + 0.5).clamp(0.0, 1.0);
                if cov <= 0.0 {
                    continue;
                }
                let a = (dx.atan2(-dy) / std::f32::consts::TAU).rem_euclid(1.0);
                self.blend(px, py, if a <= frac { fg } else { bg }, cov);
            }
        }
    }

    /// Filled triangle (logical coordinates), anti-aliased by 4x4 supersampling.
    pub fn tri(&mut self, a: (f32, f32), b: (f32, f32), c: (f32, f32), color: Rgb) {
        let s = self.scale;
        let (a, b, c) = ((a.0 * s, a.1 * s), (b.0 * s, b.1 * s), (c.0 * s, c.1 * s));
        let edge = |p: (f32, f32), q: (f32, f32), x: f32, y: f32| {
            (q.0 - p.0) * (y - p.1) - (q.1 - p.1) * (x - p.0)
        };
        let area = edge(a, b, c.0, c.1);
        if area.abs() < 1e-3 {
            return;
        }
        let x0 = a.0.min(b.0).min(c.0).floor().max(0.0) as i32;
        let x1 = a.0.max(b.0).max(c.0).ceil().min(self.w as f32) as i32;
        let y0 = a.1.min(b.1).min(c.1).floor().max(0.0) as i32;
        let y1 = a.1.max(b.1).max(c.1).ceil().min(self.h as f32) as i32;
        for py in y0..y1 {
            for px in x0..x1 {
                let mut hits = 0;
                for sy in 0..4 {
                    for sx in 0..4 {
                        let (x, y) = (
                            px as f32 + (sx as f32 + 0.5) / 4.0,
                            py as f32 + (sy as f32 + 0.5) / 4.0,
                        );
                        let (w0, w1, w2) = (edge(b, c, x, y), edge(c, a, x, y), edge(a, b, x, y));
                        if (w0 >= 0.0 && w1 >= 0.0 && w2 >= 0.0)
                            || (w0 <= 0.0 && w1 <= 0.0 && w2 <= 0.0)
                        {
                            hits += 1;
                        }
                    }
                }
                if hits > 0 {
                    self.blend(px, py, color, hits as f32 / 16.0);
                }
            }
        }
    }

    /// Encode as an uncompressed 24-bit BMP (for screenshots).
    pub fn to_bmp(&self) -> Vec<u8> {
        let row = (self.w * 3).div_ceil(4) * 4;
        let size = 54 + row * self.h;
        let mut out = Vec::with_capacity(size);
        out.extend_from_slice(b"BM");
        for v in [size as u32, 0, 54, 40, self.w as u32, self.h as u32] {
            out.extend_from_slice(&v.to_le_bytes());
        }
        out.extend_from_slice(&1u16.to_le_bytes());
        out.extend_from_slice(&24u16.to_le_bytes());
        for v in [0u32, (row * self.h) as u32, 2835, 2835, 0, 0] {
            out.extend_from_slice(&v.to_le_bytes());
        }
        for y in (0..self.h).rev() {
            let start = out.len();
            for x in 0..self.w {
                let p = self.px[y * self.w + x];
                out.extend_from_slice(&[
                    (p & 255) as u8,
                    (p >> 8 & 255) as u8,
                    (p >> 16 & 255) as u8,
                ]);
            }
            out.resize(start + row, 0);
        }
        out
    }

    /// Horizontal line, 1 logical pixel tall.
    pub fn hline(&mut self, x: f32, y: f32, w: f32, c: Rgb) {
        self.rrect(x, y, w, 1.0, 0.0, c);
    }
}
