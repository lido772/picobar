//! PicoBar — ultra-light usage monitor for Claude and OpenCode.

#![cfg_attr(windows, windows_subsystem = "windows")]

mod alerts;
mod icon;
mod model;
mod notify;
mod sources;
mod util;

use std::time::{Duration, Instant};

use tao::event::{Event, StartCause};
use tao::event_loop::{ControlFlow, EventLoopBuilder, EventLoopProxy};
use tray_icon::menu::{CheckMenuItem, Menu, MenuEvent, MenuItem, PredefinedMenuItem};
use tray_icon::{TrayIcon, TrayIconBuilder};

use model::{SESSION_SECS, Snapshot};
use sources::claude_limits::{self, Limits, LimitsError};
use sources::claude_logs::ClaudeLogs;

/// Local sources are re-read every tick; the limits endpoint less often.
const TICK: Duration = Duration::from_secs(60);
const LIMITS_EVERY_SECS: i64 = 180;
/// Limits older than this are not shown.
const LIMITS_STALE_SECS: i64 = 900;
const USAGE_PAGE: &str = "https://claude.ai/settings/usage";
const NOTIFY_OFF_FILE: &str = "notifications-off";

enum UserEvent {
    Menu(MenuEvent),
    Limits(Result<Limits, LimitsError>),
}

struct Items {
    session: MenuItem,
    weekly: MenuItem,
    models: MenuItem,
    tokens: MenuItem,
    opencode: MenuItem,
    active: MenuItem,
    refresh: MenuItem,
    notify: CheckMenuItem,
    usage: MenuItem,
    quit: MenuItem,
}

impl Items {
    fn new(notify: bool) -> Self {
        let info = |text: &str| MenuItem::new(text, false, None);
        Items {
            session: info("Session  –"),
            weekly: info("Weekly  –"),
            models: info("Weekly by model  –"),
            tokens: info("Tokens  –"),
            opencode: info("OpenCode  –"),
            active: info("Active  –"),
            refresh: MenuItem::new("Refresh now", true, None),
            notify: CheckMenuItem::new("Notify on resets and at 80%", true, notify, None),
            usage: MenuItem::new("Open Claude usage page", true, None),
            quit: MenuItem::new("Quit PicoBar", true, None),
        }
    }

    fn menu(&self) -> Menu {
        let menu = Menu::new();
        menu.append_items(&[
            &self.session,
            &self.weekly,
            &self.models,
            &PredefinedMenuItem::separator(),
            &self.tokens,
            &self.opencode,
            &self.active,
            &PredefinedMenuItem::separator(),
            &self.refresh,
            &self.notify,
            &self.usage,
            &PredefinedMenuItem::separator(),
            &self.quit,
        ])
        .expect("menu");
        menu
    }
}

#[derive(Default)]
struct App {
    logs: ClaudeLogs,
    limits: Option<Limits>,
    limits_at: i64,
    limits_error: Option<LimitsError>,
    last_fetch: i64,
    fetching: bool,
    alerts: alerts::Alerts,
    notify: bool,
}

impl App {
    fn fetch_limits(&mut self, proxy: &EventLoopProxy<UserEvent>) {
        if self.fetching {
            return;
        }
        self.fetching = true;
        self.last_fetch = util::now();
        let proxy = proxy.clone();
        std::thread::spawn(move || {
            let _ = proxy.send_event(UserEvent::Limits(claude_limits::fetch()));
        });
    }

    fn on_limits(&mut self, result: Result<Limits, LimitsError>) {
        self.fetching = false;
        match result {
            Ok(l) => {
                self.limits = Some(l);
                self.limits_at = util::now();
                self.limits_error = None;
            }
            Err(e) => self.limits_error = Some(e),
        }
    }

    fn snapshot(&self) -> Snapshot {
        let now = util::now();
        let limits = self
            .limits
            .clone()
            .filter(|_| now - self.limits_at <= LIMITS_STALE_SECS);
        let window_start = limits
            .as_ref()
            .and_then(|l| l.session?.resets_at)
            .map(|r| r - SESSION_SECS)
            .filter(|start| *start <= now);
        Snapshot {
            now,
            tokens: self.logs.since(window_start.unwrap_or(now - SESSION_SECS)),
            window_known: window_start.is_some(),
            claude_active: self.logs.active(),
            opencode: sources::opencode::read(),
            limits_error: if limits.is_some() {
                None
            } else {
                self.limits_error
            },
            limits,
        }
    }

    fn render(&mut self, tray: Option<&TrayIcon>, items: &Items) {
        let snap = self.snapshot();
        items.session.set_text(snap.session_line());
        items.weekly.set_text(snap.weekly_line());
        items.models.set_text(snap.models_line());
        items.tokens.set_text(snap.tokens_line());
        items.opencode.set_text(snap.opencode_line());
        items.active.set_text(snap.active_line());

        if let Some(t) = tray {
            let pct = snap.session_pct();
            let gauge = icon::gauge(pct);
            #[cfg(target_os = "macos")]
            let _ = if icon::is_template(pct) {
                t.set_icon_templated(Some(gauge))
            } else {
                t.set_icon(Some(gauge))
            };
            #[cfg(not(target_os = "macos"))]
            let _ = t.set_icon(Some(gauge));
            t.set_title(Some(snap.title()));
            let _ = t.set_tooltip(Some(snap.tooltip()));
        }

        for (title, body) in self.alerts.check(snap.limits.as_ref(), snap.now) {
            if self.notify {
                notify::send(&title, &body);
            }
        }
    }
}

fn notify_enabled() -> bool {
    util::config_dir().is_none_or(|d| !d.join(NOTIFY_OFF_FILE).exists())
}

fn set_notify_enabled(on: bool) {
    if let Some(d) = util::config_dir() {
        let path = d.join(NOTIFY_OFF_FILE);
        let _ = if on {
            std::fs::remove_file(path)
        } else {
            std::fs::write(path, b"")
        };
    }
}

/// `picobar --once`: print what the menu would show and exit.
fn print_once() {
    let mut app = App::default();
    app.logs.refresh();
    app.on_limits(claude_limits::fetch());
    let s = app.snapshot();
    for line in [
        s.session_line(),
        s.weekly_line(),
        s.models_line(),
        s.tokens_line(),
        s.opencode_line(),
        s.active_line(),
    ] {
        println!("{line}");
    }
}

fn main() {
    if std::env::args().any(|a| a == "--once") {
        print_once();
        return;
    }
    #[allow(unused_mut)]
    let mut event_loop = EventLoopBuilder::<UserEvent>::with_user_event().build();
    #[cfg(target_os = "macos")]
    {
        use tao::platform::macos::{ActivationPolicy, EventLoopExtMacOS};
        event_loop.set_activation_policy(ActivationPolicy::Accessory); // no Dock icon
    }

    let proxy = event_loop.create_proxy();
    {
        let proxy = proxy.clone();
        MenuEvent::set_event_handler(Some(move |e| {
            let _ = proxy.send_event(UserEvent::Menu(e));
        }));
    }

    let notify = notify_enabled();
    let items = Items::new(notify);
    let menu = items.menu();
    let mut app = App {
        notify,
        ..Default::default()
    };
    let mut tray: Option<TrayIcon> = None;

    event_loop.run(move |event, _, control_flow| match event {
        Event::NewEvents(StartCause::Init) => {
            let builder = TrayIconBuilder::new()
                .with_menu(Box::new(menu.clone()))
                .with_tooltip("PicoBar");
            #[cfg(target_os = "macos")]
            let builder = builder.with_icon_templated(icon::gauge(None));
            #[cfg(not(target_os = "macos"))]
            let builder = builder.with_icon(icon::gauge(None));
            tray = Some(builder.build().expect("tray icon"));
            app.fetch_limits(&proxy);
            app.logs.refresh();
            app.render(tray.as_ref(), &items);
            *control_flow = ControlFlow::WaitUntil(Instant::now() + TICK);
        }
        Event::NewEvents(StartCause::ResumeTimeReached { .. }) => {
            if util::now() - app.last_fetch >= LIMITS_EVERY_SECS {
                app.fetch_limits(&proxy);
            }
            app.logs.refresh();
            app.render(tray.as_ref(), &items);
            *control_flow = ControlFlow::WaitUntil(Instant::now() + TICK);
        }
        Event::UserEvent(UserEvent::Limits(result)) => {
            app.on_limits(result);
            app.render(tray.as_ref(), &items);
        }
        Event::UserEvent(UserEvent::Menu(e)) => {
            if e.id == items.quit.id() {
                tray.take();
                *control_flow = ControlFlow::Exit;
            } else if e.id == items.refresh.id() {
                app.fetch_limits(&proxy);
                app.logs.refresh();
                app.render(tray.as_ref(), &items);
            } else if e.id == items.notify.id() {
                app.notify = items.notify.is_checked();
                set_notify_enabled(app.notify);
            } else if e.id == items.usage.id() {
                util::open_url(USAGE_PAGE);
            }
        }
        _ => {}
    });
}
