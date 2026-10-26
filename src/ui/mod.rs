//! The popover window: a native borderless window filled by our own
//! software painter through `softbuffer` (no GPU stack, no webview).

pub mod paint;
pub mod popover;
pub mod text;

use std::num::NonZeroU32;
use std::rc::Rc;
use std::time::Instant;

use tao::dpi::{LogicalSize, PhysicalPosition};
use tao::event_loop::EventLoopWindowTarget;
use tao::window::{Window, WindowBuilder};

use paint::Canvas;
use popover::{Action, Popover, View, WIDTH};

pub struct PopoverWindow {
    pub window: Rc<Window>,
    surface: softbuffer::Surface<Rc<Window>, Rc<Window>>,
    _context: softbuffer::Context<Rc<Window>>,
    fonts: text::Fonts,
    pub popover: Popover,
    height: f32,
    pub visible: bool,
    pub hidden_at: Option<Instant>,
    cursor: (f64, f64),
}

impl PopoverWindow {
    pub fn new<T: 'static>(target: &EventLoopWindowTarget<T>) -> Option<PopoverWindow> {
        #[allow(unused_mut)]
        let mut builder = WindowBuilder::new()
            .with_title("PicoBar")
            .with_inner_size(LogicalSize::new(WIDTH as f64, 600.0))
            .with_resizable(false)
            .with_visible(false)
            .with_always_on_top(true);
        #[cfg(target_os = "macos")]
        {
            use tao::platform::macos::WindowBuilderExtMacOS;
            builder = builder
                .with_titlebar_transparent(true)
                .with_title_hidden(true)
                .with_titlebar_buttons_hidden(true)
                .with_fullsize_content_view(true)
                .with_movable_by_window_background(false);
        }
        #[cfg(windows)]
        {
            use tao::platform::windows::WindowBuilderExtWindows;
            builder = builder
                .with_decorations(false)
                .with_skip_taskbar(true)
                .with_undecorated_shadow(true);
        }
        let window = Rc::new(builder.build(target).ok()?);
        let context = softbuffer::Context::new(window.clone()).ok()?;
        let surface = softbuffer::Surface::new(&context, window.clone()).ok()?;
        Some(PopoverWindow {
            window,
            surface,
            _context: context,
            fonts: text::Fonts::load(),
            popover: Popover::default(),
            height: 600.0,
            visible: false,
            hidden_at: None,
            cursor: (0.0, 0.0),
        })
    }

    pub fn draw(&mut self, view: &View) {
        let scale = self.window.scale_factor() as f32;
        let mut cv = self.canvas(scale);
        let needed = self.popover.render(&mut cv, &self.fonts, view).ceil();
        if (needed - self.height).abs() >= 1.0 {
            self.height = needed;
            self.window
                .set_inner_size(LogicalSize::new(WIDTH as f64, needed as f64));
            cv = self.canvas(scale);
            self.popover.render(&mut cv, &self.fonts, view);
        }
        let (Some(w), Some(h)) = (NonZeroU32::new(cv.w as u32), NonZeroU32::new(cv.h as u32))
        else {
            return;
        };
        if self.surface.resize(w, h).is_err() {
            return;
        }
        if let Ok(mut buf) = self.surface.buffer_mut() {
            let n = buf.len().min(cv.px.len());
            buf[..n].copy_from_slice(&cv.px[..n]);
            let _ = buf.present();
        }
    }

    fn canvas(&self, scale: f32) -> Canvas {
        Canvas::new(
            (WIDTH * scale).round() as usize,
            (self.height * scale).round() as usize,
            scale,
        )
    }

    /// Show below (macOS menu bar) or above (Windows taskbar) the tray icon.
    pub fn show_at(&mut self, rect: tray_icon::Rect) {
        let (ix, iy) = (rect.position.x, rect.position.y);
        let (iw, ih) = (rect.size.width as f64, rect.size.height as f64);
        let monitor = self.window.available_monitors().find(|m| {
            let (p, s) = (m.position(), m.size());
            ix >= p.x as f64
                && ix < p.x as f64 + s.width as f64
                && iy >= p.y as f64
                && iy < p.y as f64 + s.height as f64
        });
        let scale = monitor
            .as_ref()
            .map_or(self.window.scale_factor(), |m| m.scale_factor());
        let (w, h) = (WIDTH as f64 * scale, self.height as f64 * scale);
        let mut x = ix + iw / 2.0 - w / 2.0;
        let mut below = true;
        if let Some(m) = &monitor {
            let (p, s) = (m.position(), m.size());
            let (left, right) = (p.x as f64 + 8.0, p.x as f64 + s.width as f64 - w - 8.0);
            x = x.max(left).min(right.max(left));
            below = iy < p.y as f64 + s.height as f64 / 2.0;
        }
        let gap = 6.0 * scale;
        let y = if below { iy + ih + gap } else { iy - h - gap };
        self.window.set_outer_position(PhysicalPosition::new(x, y));
        self.window.set_visible(true);
        self.window.set_focus();
        self.visible = true;
    }

    pub fn hide(&mut self) {
        if self.visible {
            self.window.set_visible(false);
            self.visible = false;
            self.hidden_at = Some(Instant::now());
        }
    }

    pub fn cursor_moved(&mut self, x: f64, y: f64) {
        self.cursor = (x, y);
    }

    /// The action under the cursor, for a left click.
    pub fn click(&self) -> Option<Action> {
        let s = self.window.scale_factor();
        self.popover
            .hit_test((self.cursor.0 / s) as f32, (self.cursor.1 / s) as f32)
    }
}

/// The menu bar item as it looks in a dark and a light menu bar, at 2x.
pub fn menubar_preview(fonts: &text::Fonts) -> Canvas {
    use paint::Rgb;
    use text::Face;
    let mut cv = Canvas::new(2 * 440, 2 * 92, 2.0);
    cv.clear(Rgb::hex(0x0F1113));
    let rows = [
        (
            Rgb::hex(0x2B2F36),
            Rgb::hex(0xF2F3F5),
            Rgb::hex(0x5A5F66),
            0.62,
            false,
            "62%",
        ),
        (
            Rgb::hex(0xE9ECEF),
            Rgb::hex(0x8A5200),
            Rgb::hex(0xC4C8CC),
            0.84,
            true,
            "84%",
        ),
    ];
    for (i, (bar, ink, track, frac, attention, label)) in rows.into_iter().enumerate() {
        let y = 14.0 + i as f32 * 36.0;
        cv.rrect(14.0, y, 412.0, 28.0, 7.0, bar);
        let ring_c = if attention { Rgb::hex(0xF5B547) } else { ink };
        cv.ring(196.0, y + 14.0, 7.5, 2.4, frac, ring_c, track);
        if attention {
            cv.circle(202.5, y + 7.5, 4.5, bar);
            cv.circle(202.5, y + 7.5, 3.2, Rgb::hex(0xFF7A6B));
        }
        fonts.draw(&mut cv, Face::Semibold, 13.0, 209.0, y + 19.0, ink, label);
        let dim = if i == 0 {
            Rgb::hex(0xB4BAC0)
        } else {
            Rgb::hex(0x5B6168)
        };
        fonts.draw(
            &mut cv,
            Face::Semibold,
            13.0,
            26.0,
            y + 19.0,
            if i == 0 {
                Rgb::hex(0xF2F3F5)
            } else {
                Rgb::hex(0x111214)
            },
            if i == 0 { "Terminal" } else { "Code" },
        );
        fonts.draw_right(
            &mut cv,
            Face::Regular,
            13.0,
            414.0,
            y + 19.0,
            dim,
            "Mon 26 Oct  12:42",
        );
    }
    cv
}
