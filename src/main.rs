//! PicoBar — ultra-light usage monitor for Claude and OpenCode.

#![cfg_attr(windows, windows_subsystem = "windows")]

mod alerts;
mod icon;
mod model;
mod notify;
mod sources;
mod ui;
mod util;

use std::time::{Duration, Instant};

use tao::event::{ElementState, Event, MouseButton, StartCause, WindowEvent};
use tao::event_loop::{ControlFlow, EventLoopBuilder, EventLoopProxy};
use tray_icon::menu::{CheckMenuItem, Menu, MenuEvent, MenuItem, PredefinedMenuItem};
use tray_icon::{MouseButtonState, TrayIcon, TrayIconBuilder, TrayIconEvent};

use model::{AgentRow, SESSION_SECS, Snapshot, Trends};
use sources::claude_limits::{self, Limits, LimitsError};
use sources::claude_logs::ClaudeLogs;
use sources::opencode::OpenCodeStats;
use ui::PopoverWindow;
use ui::popover::{Action, Tab, View};

/// Local sources are re-read every tick; the limits endpoint less often.
const TICK: Duration = Duration::from_secs(60);
const LIMITS_EVERY_SECS: i64 = 180;
/// Limits older than this are not shown.
const LIMITS_STALE_SECS: i64 = 900;
const USAGE_PAGE: &str = "https://claude.ai/settings/usage";
const NOTIFY_OFF_FILE: &str = "notifications-off";

enum UserEvent {
    Menu(MenuEvent),
    Tray(TrayIconEvent),
    Limits(Result<Limits, LimitsError>),
    Logs(Box<ClaudeLogs>),
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
    logs_ready: bool,
    opencode: Option<OpenCodeStats>,
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

    fn load_history(proxy: &EventLoopProxy<UserEvent>) {
        let proxy = proxy.clone();
        std::thread::spawn(move || {
            let mut logs = ClaudeLogs::default();
            logs.refresh();
            let _ = proxy.send_event(UserEvent::Logs(Box::new(logs)));
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

    /// Re-read local sources (cheap: only new bytes / a few SQL queries).
    fn read_local(&mut self) {
        if self.logs_ready {
            self.logs.refresh();
        }
        self.opencode = sources::opencode::read();
    }

    fn snapshot(&self, agents: &[AgentRow]) -> Snapshot {
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
            window_start,
            claude_active: self.logs.active(now),
            opencode: self.opencode.clone(),
            limits_error: if limits.is_some() {
                None
            } else {
                self.limits_error
            },
            limits,
            waiting: model::waiting(agents),
        }
    }

    fn render(&mut self, tray: Option<&TrayIcon>, items: &Items, pop: Option<&mut PopoverWindow>) {
        let now = util::now();
        let agents = model::agents(&self.logs, self.opencode.as_ref(), now);
        let snap = self.snapshot(&agents);
        items.session.set_text(snap.session_line());
        items.weekly.set_text(snap.weekly_line());
        items.models.set_text(snap.models_line());
        items.tokens.set_text(snap.tokens_line());
        items.opencode.set_text(snap.opencode_line());
        items.active.set_text(snap.active_line());

        if let Some(t) = tray {
            let pct = snap.session_pct();
            let attention = snap.waiting > 0;
            let gauge = icon::gauge(pct, attention);
            #[cfg(target_os = "macos")]
            let _ = if icon::is_template(pct, attention) {
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

        if let Some(pop) = pop.filter(|p| p.visible) {
            let trends = if pop.popover.tab == Tab::Trends {
                model::trends(&self.logs, self.opencode.as_ref(), now, pop.popover.range)
            } else {
                Trends::default()
            };
            let view = View {
                snap: &snap,
                agents: &agents,
                trends: &trends,
                notify: self.notify,
                updated_ago: (self.limits_at > 0).then(|| now - self.limits_at),
                history_loading: !self.logs_ready,
            };
            pop.draw(&view);
        }
    }

    fn set_notify(&mut self, on: bool, items: &Items) {
        self.notify = on;
        items.notify.set_checked(on);
        if let Some(d) = util::config_dir() {
            let path = d.join(NOTIFY_OFF_FILE);
            let _ = if on {
                std::fs::remove_file(path)
            } else {
                std::fs::write(path, b"")
            };
        }
    }
}

fn notify_enabled() -> bool {
    util::config_dir().is_none_or(|d| !d.join(NOTIFY_OFF_FILE).exists())
}

/// `picobar --once`: print what the menu would show and exit.
fn print_once() {
    let mut app = App {
        logs_ready: true,
        ..Default::default()
    };
    app.read_local();
    app.on_limits(claude_limits::fetch());
    let now = util::now();
    let agents = model::agents(&app.logs, app.opencode.as_ref(), now);
    let s = app.snapshot(&agents);
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
    for a in &agents {
        println!(
            "Agent  {:?}  {} · {} · {} · {}",
            a.state,
            a.name,
            a.tool,
            a.model,
            model::human(a.tokens)
        );
    }
    let t = model::trends(&app.logs, app.opencode.as_ref(), now, 7);
    println!(
        "Trends  7d {} (Claude {}, OpenCode {}) · change {:?} · peak {:?}",
        model::human(t.total()),
        model::human(t.claude_total),
        model::human(t.opencode_total),
        t.change.map(|c| c.round()),
        t.peak
    );
    for i in &t.insights {
        println!("Insight  {i}");
    }
}

/// `picobar --render <dir>`: draw each popover tab to a BMP file (screenshots).
fn render_to_files(dir: &str) {
    let mut app = App {
        logs_ready: true,
        ..Default::default()
    };
    app.read_local();
    app.on_limits(claude_limits::fetch());
    let now = util::now();
    let agents = model::agents(&app.logs, app.opencode.as_ref(), now);
    let snap = app.snapshot(&agents);
    let fonts = ui::text::Fonts::load();
    for (tab, name) in [
        (Tab::Now, "now"),
        (Tab::Agents, "agents"),
        (Tab::Trends, "trends"),
    ] {
        let trends = model::trends(&app.logs, app.opencode.as_ref(), now, 7);
        let view = View {
            snap: &snap,
            agents: &agents,
            trends: &trends,
            notify: app.notify,
            updated_ago: Some(12),
            history_loading: false,
        };
        let mut pop = ui::popover::Popover::default();
        pop.tab = tab;
        let mut cv = ui::paint::Canvas::new(760, 2400, 2.0);
        let h = pop.render(&mut cv, &fonts, &view);
        cv.h = (h * 2.0) as usize;
        cv.px.truncate(cv.w * cv.h);
        let _ = std::fs::write(format!("{dir}/{name}.bmp"), cv.to_bmp());
    }
}

fn main() {
    if std::env::args().any(|a| a == "--once") {
        print_once();
        return;
    }
    let args: Vec<String> = std::env::args().collect();
    if let Some(i) = args.iter().position(|a| a == "--render") {
        render_to_files(args.get(i + 1).map_or(".", String::as_str));
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
        let p = proxy.clone();
        MenuEvent::set_event_handler(Some(move |e| {
            let _ = p.send_event(UserEvent::Menu(e));
        }));
        let p = proxy.clone();
        TrayIconEvent::set_event_handler(Some(move |e| {
            let _ = p.send_event(UserEvent::Tray(e));
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
    let mut pop: Option<PopoverWindow> = None;

    event_loop.run(move |event, target, control_flow| match event {
        Event::NewEvents(StartCause::Init) => {
            let builder = TrayIconBuilder::new()
                .with_menu(Box::new(menu.clone()))
                .with_menu_on_left_click(false)
                .with_tooltip("PicoBar");
            #[cfg(target_os = "macos")]
            let builder = builder.with_icon_templated(icon::gauge(None, false));
            #[cfg(not(target_os = "macos"))]
            let builder = builder.with_icon(icon::gauge(None, false));
            tray = Some(builder.build().expect("tray icon"));
            pop = PopoverWindow::new(target);
            App::load_history(&proxy);
            app.fetch_limits(&proxy);
            app.read_local();
            app.render(tray.as_ref(), &items, pop.as_mut());
            *control_flow = ControlFlow::WaitUntil(Instant::now() + TICK);
        }
        Event::NewEvents(StartCause::ResumeTimeReached { .. }) => {
            if util::now() - app.last_fetch >= LIMITS_EVERY_SECS {
                app.fetch_limits(&proxy);
            }
            app.read_local();
            app.render(tray.as_ref(), &items, pop.as_mut());
            *control_flow = ControlFlow::WaitUntil(Instant::now() + TICK);
        }
        Event::UserEvent(UserEvent::Limits(result)) => {
            app.on_limits(result);
            app.render(tray.as_ref(), &items, pop.as_mut());
        }
        Event::UserEvent(UserEvent::Logs(logs)) => {
            app.logs = *logs;
            app.logs_ready = true;
            app.render(tray.as_ref(), &items, pop.as_mut());
        }
        Event::UserEvent(UserEvent::Tray(TrayIconEvent::Click {
            button: tray_icon::MouseButton::Left,
            button_state: MouseButtonState::Up,
            rect,
            ..
        })) => {
            if let Some(p) = pop.as_mut() {
                let just_hidden = p
                    .hidden_at
                    .is_some_and(|t| t.elapsed() < Duration::from_millis(400));
                if p.visible {
                    p.hide();
                } else if !just_hidden {
                    p.popover.tab = Tab::Now;
                    app.read_local();
                    p.visible = true; // draw before showing to size the window
                    app.render(tray.as_ref(), &items, Some(&mut *p));
                    p.show_at(rect);
                    app.render(tray.as_ref(), &items, Some(p));
                }
            }
        }
        Event::WindowEvent { event, .. } => {
            let Some(p) = pop.as_mut() else { return };
            match event {
                WindowEvent::Focused(false) => p.hide(),
                WindowEvent::CloseRequested => p.hide(),
                WindowEvent::CursorMoved { position, .. } => p.cursor_moved(position.x, position.y),
                WindowEvent::MouseInput {
                    state: ElementState::Pressed,
                    button: MouseButton::Left,
                    ..
                } => {
                    match p.click() {
                        Some(Action::Tab(t)) => p.popover.tab = t,
                        Some(Action::Range(r)) => p.popover.range = r,
                        Some(Action::Refresh) => {
                            app.fetch_limits(&proxy);
                            app.read_local();
                        }
                        Some(Action::ToggleNotify) => app.set_notify(!app.notify, &items),
                        Some(Action::Quit) => {
                            tray.take();
                            *control_flow = ControlFlow::Exit;
                            return;
                        }
                        None => return,
                    }
                    app.render(tray.as_ref(), &items, Some(p));
                }
                WindowEvent::ScaleFactorChanged { .. } => {
                    app.render(tray.as_ref(), &items, Some(p))
                }
                _ => {}
            }
        }
        Event::RedrawRequested(_) => app.render(tray.as_ref(), &items, pop.as_mut()),
        Event::UserEvent(UserEvent::Menu(e)) => {
            if e.id == items.quit.id() {
                tray.take();
                *control_flow = ControlFlow::Exit;
            } else if e.id == items.refresh.id() {
                app.fetch_limits(&proxy);
                app.read_local();
                app.render(tray.as_ref(), &items, pop.as_mut());
            } else if e.id == items.notify.id() {
                app.set_notify(items.notify.is_checked(), &items);
            } else if e.id == items.usage.id() {
                util::open_url(USAGE_PAGE);
            }
        }
        _ => {}
    });
}
