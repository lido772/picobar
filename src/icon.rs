//! Procedurally drawn ring gauge — no image files, no image crate.

use tray_icon::Icon;

use crate::alerts::WARN_PCT;

const SIZE: u32 = 32;
const AMBER: [u8; 3] = [245, 181, 71];
const MINT: [u8; 3] = [94, 230, 168];

/// Whether the icon should be drawn as a macOS template image (monochrome,
/// adapts to light and dark menu bars). Past the threshold it turns amber.
pub fn is_template(pct: Option<u8>) -> bool {
    cfg!(target_os = "macos") && pct.is_none_or(|p| (p as f32) < WARN_PCT)
}

/// Ring that fills clockwise with `pct`; an empty, faint ring when unknown.
pub fn gauge(pct: Option<u8>) -> Icon {
    let fill = match pct {
        Some(p) if p as f32 >= WARN_PCT => AMBER,
        _ if is_template(pct) => [0, 0, 0],
        _ => MINT,
    };
    let frac = pct.map_or(0.0, |p| p.min(100) as f32 / 100.0);
    let (c, r_out, r_in) = (SIZE as f32 / 2.0, 14.0_f32, 9.5_f32);
    let mut rgba = vec![0u8; (SIZE * SIZE * 4) as usize];

    for y in 0..SIZE {
        for x in 0..SIZE {
            let (dx, dy) = (x as f32 + 0.5 - c, y as f32 + 0.5 - c);
            let d = (dx * dx + dy * dy).sqrt();
            if d < r_in - 0.5 || d > r_out + 0.5 {
                continue;
            }
            // soft edge for a little anti-aliasing
            let edge = (r_out + 0.5 - d).min(d - (r_in - 0.5)).clamp(0.0, 1.0);
            // angle from 12 o'clock, clockwise, 0..1
            let a = (dx.atan2(-dy) / std::f32::consts::TAU).rem_euclid(1.0);
            let (rgb, alpha) = if a <= frac {
                (fill, 255.0)
            } else if is_template(pct) {
                ([0, 0, 0], 80.0)
            } else {
                ([128, 128, 128], 110.0)
            };
            let i = ((y * SIZE + x) * 4) as usize;
            rgba[i..i + 3].copy_from_slice(&rgb);
            rgba[i + 3] = (alpha * edge) as u8;
        }
    }
    Icon::from_rgba(rgba, SIZE, SIZE).expect("icon")
}
