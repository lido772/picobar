//! Text with the operating system's own fonts (nothing embedded in the
//! binary): San Francisco on macOS, Segoe UI / Consolas on Windows.

// Drawing calls take position, size and style together.
#![allow(clippy::too_many_arguments)]

use ab_glyph::{Font, FontRef, PxScale, ScaleFont, VariableFont, point};

use super::paint::{Canvas, Rgb};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Face {
    Regular,
    Semibold,
    Mono,
    MonoBold,
}

/// Map a font file read-only for the life of the process. Mapped pages are
/// backed by the file, so a large system font costs almost no memory, and
/// each file is mapped once even when used for several weights.
#[cfg(unix)]
fn map_file(path: &str) -> Option<&'static [u8]> {
    use std::collections::HashMap;
    use std::os::fd::AsRawFd;
    use std::sync::Mutex;
    static MAPS: Mutex<Option<HashMap<String, (usize, usize)>>> = Mutex::new(None);
    let mut maps = MAPS.lock().ok()?;
    let maps = maps.get_or_insert_with(HashMap::new);
    if let Some(&(ptr, len)) = maps.get(path) {
        // SAFETY: mapping created below and never unmapped.
        return Some(unsafe { std::slice::from_raw_parts(ptr as *const u8, len) });
    }
    let file = std::fs::File::open(path).ok()?;
    let len = file.metadata().ok()?.len() as usize;
    if len == 0 {
        return None;
    }
    // SAFETY: read-only private mapping of a whole file we just opened.
    let ptr = unsafe {
        libc::mmap(
            std::ptr::null_mut(),
            len,
            libc::PROT_READ,
            libc::MAP_PRIVATE,
            file.as_raw_fd(),
            0,
        )
    };
    if ptr == libc::MAP_FAILED {
        return None;
    }
    maps.insert(path.to_owned(), (ptr as usize, len));
    // SAFETY: valid for `len` bytes for the rest of the process.
    Some(unsafe { std::slice::from_raw_parts(ptr as *const u8, len) })
}

#[cfg(not(unix))]
fn map_file(path: &str) -> Option<&'static [u8]> {
    Some(Box::leak(std::fs::read(path).ok()?.into_boxed_slice()))
}

/// Resolve a font file through fontconfig (Linux).
#[cfg(not(any(target_os = "macos", windows)))]
fn fc_match(pattern: &str) -> Option<String> {
    let out = crate::util::hidden("fc-match")
        .args(["-f", "%{file}", pattern])
        .output()
        .ok()?;
    let path = String::from_utf8(out.stdout).ok()?;
    (path.ends_with(".ttf") || path.ends_with(".otf")).then_some(path)
}

pub struct Fonts {
    faces: [Option<FontRef<'static>>; 4],
}

impl Fonts {
    pub fn load() -> Fonts {
        #[cfg(target_os = "macos")]
        let spec: [(&[&str], f32); 4] = [
            (
                &[
                    "/System/Library/Fonts/SFNS.ttf",
                    "/System/Library/Fonts/Helvetica.ttc",
                ],
                400.0,
            ),
            (
                &[
                    "/System/Library/Fonts/SFNS.ttf",
                    "/System/Library/Fonts/Helvetica.ttc",
                ],
                600.0,
            ),
            (
                &[
                    "/System/Library/Fonts/SFNSMono.ttf",
                    "/System/Library/Fonts/Menlo.ttc",
                ],
                500.0,
            ),
            (
                &[
                    "/System/Library/Fonts/SFNSMono.ttf",
                    "/System/Library/Fonts/Menlo.ttc",
                ],
                650.0,
            ),
        ];
        #[cfg(windows)]
        let spec: [(&[&str], f32); 4] = [
            (&["C:\\Windows\\Fonts\\segoeui.ttf"], 400.0),
            (
                &[
                    "C:\\Windows\\Fonts\\seguisb.ttf",
                    "C:\\Windows\\Fonts\\segoeuib.ttf",
                ],
                600.0,
            ),
            (&["C:\\Windows\\Fonts\\consola.ttf"], 400.0),
            (
                &[
                    "C:\\Windows\\Fonts\\consolab.ttf",
                    "C:\\Windows\\Fonts\\consola.ttf",
                ],
                700.0,
            ),
        ];
        // Linux: ask fontconfig, fall back to DejaVu.
        #[cfg(not(any(target_os = "macos", windows)))]
        let lists: Vec<[String; 2]> = [
            ("sans:weight=80", "DejaVuSans.ttf"),
            ("sans:weight=180", "DejaVuSans-Bold.ttf"),
            ("monospace:weight=80", "DejaVuSansMono.ttf"),
            ("monospace:weight=200", "DejaVuSansMono-Bold.ttf"),
        ]
        .iter()
        .map(|(q, f)| {
            [
                fc_match(q).unwrap_or_default(),
                format!("/usr/share/fonts/truetype/dejavu/{f}"),
            ]
        })
        .collect();
        #[cfg(not(any(target_os = "macos", windows)))]
        let refs: Vec<[&str; 2]> = lists
            .iter()
            .map(|l| [l[0].as_str(), l[1].as_str()])
            .collect();
        #[cfg(not(any(target_os = "macos", windows)))]
        let spec: [(&[&str], f32); 4] = [
            (&refs[0][..], 400.0),
            (&refs[1][..], 600.0),
            (&refs[2][..], 400.0),
            (&refs[3][..], 700.0),
        ];
        let faces = spec.map(|(paths, weight)| {
            paths.iter().filter(|p| !p.is_empty()).find_map(|p| {
                let mut f = FontRef::try_from_slice(map_file(p)?).ok()?;
                f.set_variation(b"wght", weight);
                Some(f)
            })
        });
        Fonts { faces }
    }

    fn face(&self, face: Face) -> Option<&FontRef<'static>> {
        let i = face as usize;
        self.faces[i]
            .as_ref()
            .or(self.faces[i & 2].as_ref())
            .or(self.faces[0].as_ref())
    }

    /// Width of `text` in logical pixels.
    pub fn width(&self, face: Face, size: f32, text: &str) -> f32 {
        let Some(font) = self.face(face) else {
            return text.len() as f32 * size * 0.55;
        };
        let scaled = font.as_scaled(PxScale::from(size));
        let mut w = 0.0;
        let mut prev = None;
        for c in text.chars() {
            let id = font.glyph_id(c);
            if let Some(p) = prev {
                w += scaled.kern(p, id);
            }
            w += scaled.h_advance(id);
            prev = Some(id);
        }
        w
    }

    /// Draw `text` with its left edge at `x` and baseline at `y` (logical).
    /// Returns the advance width in logical pixels.
    pub fn draw(
        &self,
        cv: &mut Canvas,
        face: Face,
        size: f32,
        x: f32,
        y: f32,
        color: Rgb,
        text: &str,
    ) -> f32 {
        let Some(font) = self.face(face) else {
            return 0.0;
        };
        let s = cv.scale;
        let px = size * s;
        let scaled = font.as_scaled(PxScale::from(px));
        let mut caret = x * s;
        let base = y * s;
        let mut prev = None;
        for c in text.chars() {
            let id = font.glyph_id(c);
            if let Some(p) = prev {
                caret += scaled.kern(p, id);
            }
            let glyph = id.with_scale_and_position(px, point(caret, base));
            caret += scaled.h_advance(id);
            prev = Some(id);
            if let Some(og) = font.outline_glyph(glyph) {
                let b = og.px_bounds();
                og.draw(|gx, gy, cov| {
                    cv.blend(
                        b.min.x as i32 + gx as i32,
                        b.min.y as i32 + gy as i32,
                        color,
                        cov,
                    );
                });
            }
        }
        caret / s - x
    }

    /// Draw right-aligned so the text ends at `right`.
    pub fn draw_right(
        &self,
        cv: &mut Canvas,
        face: Face,
        size: f32,
        right: f32,
        y: f32,
        color: Rgb,
        text: &str,
    ) {
        let w = self.width(face, size, text);
        self.draw(cv, face, size, right - w, y, color, text);
    }

    /// Draw centered on `cx`.
    pub fn draw_center(
        &self,
        cv: &mut Canvas,
        face: Face,
        size: f32,
        cx: f32,
        y: f32,
        color: Rgb,
        text: &str,
    ) {
        let w = self.width(face, size, text);
        self.draw(cv, face, size, cx - w / 2.0, y, color, text);
    }

    /// Truncate with an ellipsis so the text fits in `max` logical pixels.
    pub fn fit(&self, face: Face, size: f32, text: &str, max: f32) -> String {
        if self.width(face, size, text) <= max {
            return text.to_owned();
        }
        let mut out: String = text.to_owned();
        while !out.is_empty() && self.width(face, size, &format!("{out}…")) > max {
            out.pop();
        }
        format!("{}…", out.trim_end())
    }
}
