//! Procedurally drawn ring gauge — no image files, no image crate.

use tray_icon::Icon;

const SIZE: u32 = 32;

/// Ring that fills clockwise with `pct`, plus a coral dot when an agent waits.
pub fn gauge(pct: u8, attention: bool) -> Icon {
    let pct = pct.min(100) as f32 / 100.0;
    let (c, r_out, r_in) = (SIZE as f32 / 2.0, 14.0_f32, 9.5_f32);
    let fill = if pct >= 0.8 {
        [245, 181, 71]
    } else {
        [236, 237, 238]
    }; // amber past 80%
    let mut rgba = vec![0u8; (SIZE * SIZE * 4) as usize];

    for y in 0..SIZE {
        for x in 0..SIZE {
            let (dx, dy) = (x as f32 + 0.5 - c, y as f32 + 0.5 - c);
            let d = (dx * dx + dy * dy).sqrt();
            let i = ((y * SIZE + x) * 4) as usize;

            if attention && ((dx - 9.0).powi(2) + (dy + 9.0).powi(2)).sqrt() < 5.5 {
                rgba[i..i + 4].copy_from_slice(&[255, 122, 107, 255]);
                continue;
            }
            if d < r_in || d > r_out {
                continue;
            }
            // angle from 12 o'clock, clockwise, 0..1
            let a = (dx.atan2(-dy) / std::f32::consts::TAU).rem_euclid(1.0);
            let px = if a <= pct {
                [fill[0], fill[1], fill[2], 255]
            } else {
                [128, 128, 128, 90]
            };
            rgba[i..i + 4].copy_from_slice(&px);
        }
    }
    Icon::from_rgba(rgba, SIZE, SIZE).expect("icon")
}
