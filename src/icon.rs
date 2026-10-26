//! Procedurally drawn ring gauge — no image files, no image crate.

use tray_icon::Icon;

const SIZE: u32 = 32;
const AMBER: [u8; 3] = [245, 181, 71];
const MINT: [u8; 3] = [94, 230, 168];
const GREY: [u8; 3] = [154, 160, 166];
const CORAL: [u8; 3] = [255, 122, 107];

/// Whether the icon is drawn as a macOS template image (monochrome, adapts to
/// light and dark menu bars). Colored when past the threshold or when an
/// agent waits for you.
pub fn is_template(pct: Option<u8>, attention: bool, warn: f32) -> bool {
    cfg!(target_os = "macos") && !attention && pct.is_none_or(|p| (p as f32) < warn)
}

/// Ring that fills clockwise with `pct` (faint when unknown), with a coral
/// dot when an agent is waiting for you.
pub fn gauge(pct: Option<u8>, attention: bool, warn: f32) -> Icon {
    let template = is_template(pct, attention, warn);
    let fill = match pct {
        Some(p) if p as f32 >= warn => AMBER,
        _ if template => [0, 0, 0],
        _ if cfg!(target_os = "macos") => GREY,
        _ => MINT,
    };
    let frac = pct.map_or(0.0, |p| p.min(100) as f32 / 100.0);
    let (c, r_out, r_in) = (SIZE as f32 / 2.0, 14.0_f32, 9.5_f32);
    let mut rgba = vec![0u8; (SIZE * SIZE * 4) as usize];

    for y in 0..SIZE {
        for x in 0..SIZE {
            let (dx, dy) = (x as f32 + 0.5 - c, y as f32 + 0.5 - c);
            let i = ((y * SIZE + x) * 4) as usize;
            if attention {
                let dd = ((dx - 9.5).powi(2) + (dy + 9.5).powi(2)).sqrt();
                if dd < 7.5 {
                    // dot plus a transparent gap around it
                    let cov = (6.0 - dd).clamp(0.0, 1.0);
                    rgba[i..i + 3].copy_from_slice(&CORAL);
                    rgba[i + 3] = (255.0 * cov) as u8;
                    continue;
                }
            }
            let d = (dx * dx + dy * dy).sqrt();
            if d < r_in - 0.5 || d > r_out + 0.5 {
                continue;
            }
            let edge = (r_out + 0.5 - d).min(d - (r_in - 0.5)).clamp(0.0, 1.0);
            let a = (dx.atan2(-dy) / std::f32::consts::TAU).rem_euclid(1.0);
            let (rgb, alpha) = if a <= frac {
                (fill, 255.0)
            } else if template {
                ([0, 0, 0], 80.0)
            } else {
                ([128, 128, 128], 110.0)
            };
            rgba[i..i + 3].copy_from_slice(&rgb);
            rgba[i + 3] = (alpha * edge) as u8;
        }
    }
    Icon::from_rgba(rgba, SIZE, SIZE).expect("icon")
}
