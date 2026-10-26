//! PicoBar — ultra-light usage monitor for Claude and OpenCode.

#![cfg_attr(windows, windows_subsystem = "windows")]

mod alerts;
mod autostart;
mod demo;
mod icon;
mod model;
mod notify;
mod settings;
mod sources;
mod ui;
mod update;
mod util;

use std::time::{Duration, Instant};

use tao::event::{ElementState, Event, MouseButton, StartCause, WindowEvent};
use tao::event_loop::{ControlFlow, EventLoopBuilder, EventLoopProxy};
use tao::window::Theme;
use tray_icon::menu::{
    CheckMenuItem, Menu, MenuEvent, MenuId, MenuItem, PredefinedMenuItem, Submenu,
};
use tray_icon::{MouseButtonState, TrayIcon, TrayIconBuilder, TrayIconEvent};

use model::{AgentRow, SESSION_SECS, Snapshot, Trends};
use settings::{Appearance, MenuBar, Settings};
use sources::claude_hook;
use sources::claude_limits::{self, Limits, LimitsError};
use sources::claude_logs::ClaudeLogs;
use sources::opencode::OpenCodeStats;
use ui::PopoverWindow;
use ui::popover::{Action, Tab, View};
use update::{Release, UpdateState};

/// Local sources are re-read every tick; the limits endpoint less often
/// (`Settings::refresh_min`).
const TICK: Duration = Duration::from_secs(60);
const USAGE_PAGE: &str = "https://claude.ai/settings/usage";
const UPDATE_CHECK_EVERY_SECS: i64 = 86_400;

enum UserEvent {
    Menu(MenuEvent),
    Tray(TrayIconEvent),
    Limits(Result<Limits, LimitsError>),
    Logs(Box<ClaudeLogs>),
    UpdateChecked(Result<Option<Release>, String>, bool),
    UpdateFailed(String),
    UpdateInstalled,
}

/// A radio-like group of check items in a submenu, kept in sync by hand.
struct Choice<T> {
    menu: Submenu,
    items: Vec<(CheckMenuItem, T)>,
}

impl<T: Copy + PartialEq> Choice<T> {
    fn new(title: &str, options: Vec<(String, T)>, current: T) -> Self {
        let menu = Submenu::new(title, true);
        let items: Vec<_> = options
            .into_iter()
            .map(|(label, v)| (CheckMenuItem::new(label, true, v == current, None), v))
            .collect();
        for (item, _) in &items {
            menu.append(item).expect("menu");
        }
        Choice { menu, items }
    }

    /// If `id` is one of ours, check only it and return its value.
    fn pick(&self, id: &MenuId) -> Option<T> {
        let v = self.items.iter().find(|(i, _)| i.id() == id)?.1;
        for (item, x) in &self.items {
            item.set_checked(*x == v);
        }
        Some(v)
    }
}

fn notify_label(warn: u8) -> String {
    format!("Notify on resets and at {warn}%")
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
    prompts: CheckMenuItem,
    settings: Submenu,
    warn: Choice<u8>,
    refresh_every: Choice<u8>,
    menubar: Choice<MenuBar>,
    theme: Choice<Appearance>,
    login: CheckMenuItem,
    usage: MenuItem,
    update: MenuItem,
    #[cfg(not(any(target_os = "macos", windows)))]
    open: MenuItem,
    quit: MenuItem,
}

impl Items {
    fn new(s: &Settings) -> Self {
        let info = |text: &str| MenuItem::new(text, false, None);
        let warn = Choice::new(
            "Warn at",
            settings::WARN_CHOICES
                .map(|w| (format!("{w}%"), w))
                .to_vec(),
            s.warn,
        );
        let refresh_every = Choice::new(
            "Refresh limits every",
            settings::REFRESH_CHOICES
                .map(|m| (format!("{m} min"), m))
                .to_vec(),
            s.refresh_min,
        );
        let menubar = Choice::new(
            "Menu bar shows",
            vec![
                ("Percent".into(), MenuBar::Percent),
                ("Time to reset".into(), MenuBar::Reset),
                ("Icon only".into(), MenuBar::Icon),
            ],
            s.menubar,
        );
        let theme = Choice::new(
            "Appearance",
            vec![
                ("System".into(), Appearance::System),
                ("Light".into(), Appearance::Light),
                ("Dark".into(), Appearance::Dark),
            ],
            s.theme,
        );
        let login = CheckMenuItem::new("Launch at login", true, autostart::enabled(), None);
        let settings = Submenu::new("Settings", true);
        settings.append(&warn.menu).expect("menu");
        settings.append(&refresh_every.menu).expect("menu");
        settings.append(&theme.menu).expect("menu");
        #[cfg(not(windows))] // Windows trays have no title
        settings.append(&menubar.menu).expect("menu");
        settings
            .append_items(&[&PredefinedMenuItem::separator(), &login])
            .expect("menu");
        Items {
            session: info("Session  –"),
            weekly: info("Weekly  –"),
            models: info("Weekly by model  –"),
            tokens: info("Tokens  –"),
            opencode: info("OpenCode  –"),
            active: info("Active  –"),
            refresh: MenuItem::new("Refresh now", true, None),
            notify: CheckMenuItem::new(notify_label(s.warn), true, s.notify, None),
            prompts: CheckMenuItem::new(
                "Show Claude permission prompts",
                true,
                claude_hook::installed(),
                None,
            ),
            settings,
            warn,
            refresh_every,
            menubar,
            theme,
            login,
            usage: MenuItem::new("Open Claude usage page", true, None),
            update: MenuItem::new("Check for updates", true, None),
            #[cfg(not(any(target_os = "macos", windows)))]
            open: MenuItem::new("Open PicoBar", true, None),
            quit: MenuItem::new("Quit PicoBar", true, None),
        }
    }

    fn menu(&self) -> Menu {
        let menu = Menu::new();
        #[cfg(not(any(target_os = "macos", windows)))]
        menu.append_items(&[&self.open, &PredefinedMenuItem::separator()])
            .expect("menu");
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
            &self.prompts,
            &self.settings,
            &self.usage,
            &self.update,
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
    settings: Settings,
    update: UpdateState,
    last_update_check: i64,
    announced_version: Option<String>,
}

impl App {
    fn limits_every(&self) -> i64 {
        i64::from(self.settings.refresh_min) * 60
    }

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

    fn check_update(&mut self, proxy: &EventLoopProxy<UserEvent>, manual: bool) {
        if matches!(
            self.update,
            UpdateState::Checking | UpdateState::Installing(_)
        ) {
            return;
        }
        self.last_update_check = util::now();
        if manual {
            self.update = UpdateState::Checking;
        }
        let proxy = proxy.clone();
        std::thread::spawn(move || {
            let _ = proxy.send_event(UserEvent::UpdateChecked(update::check(), manual));
        });
    }

    fn on_update_checked(&mut self, result: Result<Option<Release>, String>, manual: bool) {
        self.update = match result {
            Ok(Some(r)) => {
                if !manual
                    && self.settings.notify
                    && self.announced_version.as_deref() != Some(r.version.as_str())
                {
                    notify::send(
                        &format!("PicoBar {} is available", r.version),
                        "Open PicoBar to update.",
                    );
                }
                self.announced_version = Some(r.version.clone());
                UpdateState::Available(r)
            }
            Ok(None) => UpdateState::UpToDate,
            Err(e) if manual => UpdateState::Failed(e),
            Err(_) => UpdateState::Unknown,
        };
    }

    fn install_update(&mut self, proxy: &EventLoopProxy<UserEvent>) {
        let UpdateState::Available(r) = &self.update else {
            return;
        };
        if !r.installable() {
            util::open_url(&r.page);
            return;
        }
        let r = r.clone();
        self.update = UpdateState::Installing(r.version.clone());
        let proxy = proxy.clone();
        std::thread::spawn(move || {
            let _ = proxy.send_event(match update::install(&r) {
                Ok(()) => UserEvent::UpdateInstalled,
                Err(e) => UserEvent::UpdateFailed(e),
            });
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
            .filter(|_| now - self.limits_at <= 5 * self.limits_every());
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
        let (label, enabled) = match &self.update {
            UpdateState::Available(r) => (format!("Update to PicoBar {}", r.version), true),
            UpdateState::Checking => ("Checking for updates…".into(), false),
            UpdateState::Installing(v) => (format!("Updating to {v}…"), false),
            UpdateState::UpToDate => (format!("PicoBar {} is up to date", update::CURRENT), true),
            _ => ("Check for updates".into(), true),
        };
        items.update.set_text(label);
        items.update.set_enabled(enabled);

        if let Some(t) = tray {
            let pct = snap.session_pct();
            let attention = snap.waiting > 0;
            let warn = f32::from(self.settings.warn);
            let gauge = icon::gauge(pct, attention, warn);
            #[cfg(target_os = "macos")]
            let _ = if icon::is_template(pct, attention, warn) {
                t.set_icon_templated(Some(gauge))
            } else {
                t.set_icon(Some(gauge))
            };
            #[cfg(not(target_os = "macos"))]
            let _ = t.set_icon(Some(gauge));
            t.set_title(snap.title(self.settings.menubar));
            let _ = t.set_tooltip(Some(snap.tooltip()));
        }

        let mut msgs = self.alerts.check(
            snap.limits.as_ref(),
            snap.now,
            f32::from(self.settings.warn),
        );
        msgs.extend(self.alerts.check_agents(&agents));
        for (title, body) in msgs {
            if self.settings.notify {
                notify::send(&title, &body);
            }
        }

        if let Some(pop) = pop.filter(|p| p.visible) {
            // Linux: tao's theme() asks D-Bus and can block, so System means dark there.
            let palette = match self.settings.theme {
                Appearance::Light => &ui::popover::LIGHT,
                Appearance::System
                    if !cfg!(target_os = "linux") && pop.window.theme() == Theme::Light =>
                {
                    &ui::popover::LIGHT
                }
                _ => &ui::popover::DARK,
            };
            let trends = if pop.popover.tab == Tab::Trends {
                model::trends(&self.logs, self.opencode.as_ref(), now, pop.popover.range)
            } else {
                Trends::default()
            };
            let view = View {
                snap: &snap,
                agents: &agents,
                trends: &trends,
                notify: self.settings.notify,
                warn_pct: f32::from(self.settings.warn),
                updated_ago: (self.limits_at > 0).then(|| now - self.limits_at),
                history_loading: !self.logs_ready,
                update: &self.update,
                palette,
            };
            pop.draw(&view);
        }
    }

    fn set_notify(&mut self, on: bool, items: &Items) {
        self.settings.notify = on;
        items.notify.set_checked(on);
        settings::save(self.settings);
    }

    /// Apply a click on one of the Settings submenu groups; false if `id` is not one.
    fn pick_setting(&mut self, id: &MenuId, items: &Items) -> bool {
        let mut s = self.settings;
        if let Some(w) = items.warn.pick(id) {
            s.warn = w;
            items.notify.set_text(notify_label(w));
        } else if let Some(m) = items.refresh_every.pick(id) {
            s.refresh_min = m;
        } else if let Some(m) = items.menubar.pick(id) {
            s.menubar = m;
        } else if let Some(t) = items.theme.pick(id) {
            s.theme = t;
        } else {
            return false;
        }
        self.settings = s;
        settings::save(s);
        true
    }
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

/// `picobar --render <dir> [--demo]`: draw each popover tab and the menu bar
/// item to BMP files (for screenshots). `--demo` uses fictional data.
fn render_to_files(dir: &str, demo: bool, light: bool) {
    let (palette, suffix) = if light {
        (&ui::popover::LIGHT, "-light")
    } else {
        (&ui::popover::DARK, "")
    };
    let fonts = ui::text::Fonts::load();
    let real = || {
        let mut app = App {
            logs_ready: true,
            ..Default::default()
        };
        app.read_local();
        app.on_limits(claude_limits::fetch());
        app
    };
    let app = (!demo).then(real);
    for (tab, name) in [
        (Tab::Now, "now"),
        (Tab::Agents, "agents"),
        (Tab::Trends, "trends"),
    ] {
        let (snap, agents, trends) = match &app {
            Some(app) => {
                let now = util::now();
                let agents = model::agents(&app.logs, app.opencode.as_ref(), now);
                let trends = model::trends(&app.logs, app.opencode.as_ref(), now, 7);
                (app.snapshot(&agents), agents, trends)
            }
            None => demo::data(7),
        };
        let view = View {
            snap: &snap,
            agents: &agents,
            trends: &trends,
            notify: true,
            warn_pct: 80.0,
            updated_ago: Some(12),
            history_loading: false,
            update: &UpdateState::Unknown,
            palette,
        };
        let mut pop = ui::popover::Popover::default();
        pop.tab = tab;
        let mut cv = ui::paint::Canvas::new(760, 2400, 2.0);
        let h = pop.render(&mut cv, &fonts, &view);
        cv.h = (h * 2.0) as usize;
        cv.px.truncate(cv.w * cv.h);
        let _ = std::fs::write(format!("{dir}/{name}{suffix}.bmp"), cv.to_bmp());
    }
    // The menu bar preview already shows both themes.
    let _ = std::fs::write(
        format!("{dir}/menubar.bmp"),
        ui::menubar_preview(&fonts).to_bmp(),
    );
}

fn main() {
    if std::env::args().any(|a| a == "--hook") {
        sources::claude_hook::run_hook();
        return;
    }
    if std::env::args().any(|a| a == "--once") {
        print_once();
        return;
    }
    let args: Vec<String> = std::env::args().collect();
    if args.iter().any(|a| a == "--update") {
        // `picobar --update`: check, install and restart from a terminal.
        match update::check() {
            Ok(None) => println!("PicoBar {} is up to date.", update::CURRENT),
            Ok(Some(r)) => {
                println!("Updating PicoBar {} -> {}…", update::CURRENT, r.version);
                match update::install(&r) {
                    Ok(()) => println!("Done. PicoBar {} is starting.", r.version),
                    Err(e) => {
                        eprintln!("Update failed: {e}");
                        std::process::exit(1);
                    }
                }
            }
            Err(e) => {
                eprintln!("Could not check for updates: {e}");
                std::process::exit(1);
            }
        }
        return;
    }
    if let Some(i) = args.iter().position(|a| a == "--render") {
        let demo = args.iter().any(|a| a == "--demo");
        let light = args.iter().any(|a| a == "--light");
        render_to_files(args.get(i + 1).map_or(".", String::as_str), demo, light);
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

    let settings = settings::load();
    let items = Items::new(&settings);
    let menu = items.menu();
    let mut app = App {
        settings,
        ..Default::default()
    };
    let mut tray: Option<TrayIcon> = None;
    let mut pop: Option<PopoverWindow> = None;
    // `--window`, or no tray available (e.g. GNOME without AppIndicator):
    // keep the popover open as a small window.
    let mut standalone = std::env::args().any(|a| a == "--window");

    event_loop.run(move |event, target, control_flow| match event {
        Event::NewEvents(StartCause::Init) => {
            let builder = TrayIconBuilder::new()
                .with_menu(Box::new(menu.clone()))
                .with_menu_on_left_click(false)
                .with_tooltip("PicoBar");
            #[cfg(target_os = "macos")]
            let builder =
                builder.with_icon_templated(icon::gauge(None, false, f32::from(app.settings.warn)));
            #[cfg(not(target_os = "macos"))]
            let builder = builder.with_icon(icon::gauge(None, false, f32::from(app.settings.warn)));
            tray = builder.build().ok();
            standalone |= tray.is_none();
            pop = PopoverWindow::new(target);
            update::cleanup();
            app.check_update(&proxy, false);
            App::load_history(&proxy);
            app.fetch_limits(&proxy);
            app.read_local();
            app.render(tray.as_ref(), &items, pop.as_mut());
            if standalone && let Some(p) = pop.as_mut() {
                p.visible = true;
                app.render(tray.as_ref(), &items, Some(&mut *p));
                p.show_in_corner();
                app.render(tray.as_ref(), &items, Some(p));
            }
            *control_flow = ControlFlow::WaitUntil(Instant::now() + TICK);
        }
        Event::NewEvents(StartCause::ResumeTimeReached { .. }) => {
            if util::now() - app.last_fetch >= app.limits_every() {
                app.fetch_limits(&proxy);
            }
            if util::now() - app.last_update_check >= UPDATE_CHECK_EVERY_SECS {
                app.check_update(&proxy, false);
            }
            app.read_local();
            app.render(tray.as_ref(), &items, pop.as_mut());
            *control_flow = ControlFlow::WaitUntil(Instant::now() + TICK);
        }
        Event::UserEvent(UserEvent::Limits(result)) => {
            app.on_limits(result);
            app.render(tray.as_ref(), &items, pop.as_mut());
        }
        Event::UserEvent(UserEvent::UpdateChecked(result, manual)) => {
            app.on_update_checked(result, manual);
            app.render(tray.as_ref(), &items, pop.as_mut());
        }
        Event::UserEvent(UserEvent::UpdateFailed(e)) => {
            app.update = UpdateState::Failed(e);
            app.render(tray.as_ref(), &items, pop.as_mut());
        }
        Event::UserEvent(UserEvent::UpdateInstalled) => {
            tray.take();
            *control_flow = ControlFlow::Exit;
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
                WindowEvent::Focused(false) if !standalone => p.hide(),
                WindowEvent::CloseRequested if standalone => *control_flow = ControlFlow::Exit,
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
                        Some(Action::ToggleNotify) => app.set_notify(!app.settings.notify, &items),
                        Some(Action::Update) => app.install_update(&proxy),
                        Some(Action::OpenReleasePage) => {
                            let page = match &app.update {
                                UpdateState::Available(r) => r.page.clone(),
                                _ => update::RELEASES_PAGE.to_owned(),
                            };
                            util::open_url(&page);
                        }
                        Some(Action::Quit) => {
                            tray.take();
                            *control_flow = ControlFlow::Exit;
                            return;
                        }
                        None => return,
                    }
                    app.render(tray.as_ref(), &items, Some(p));
                }
                WindowEvent::ScaleFactorChanged { .. } | WindowEvent::ThemeChanged(_) => {
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
            } else if e.id == items.prompts.id() {
                let on = items.prompts.is_checked();
                let r = if on {
                    claude_hook::install()
                } else {
                    claude_hook::uninstall()
                };
                if let Err(e) = r {
                    notify::send("Could not change Claude settings", &e);
                }
                items.prompts.set_checked(claude_hook::installed());
            } else if e.id == items.login.id() {
                if let Err(e) = autostart::set(items.login.is_checked()) {
                    notify::send("Could not change launch at login", &e);
                }
                items.login.set_checked(autostart::enabled());
            } else if app.pick_setting(&e.id, &items) {
                app.render(tray.as_ref(), &items, pop.as_mut());
            } else if e.id == items.usage.id() {
                util::open_url(USAGE_PAGE);
            } else if e.id == items.update.id() {
                if matches!(app.update, UpdateState::Available(_)) {
                    app.install_update(&proxy);
                } else {
                    app.check_update(&proxy, true);
                }
                app.render(tray.as_ref(), &items, pop.as_mut());
            } else {
                #[cfg(not(any(target_os = "macos", windows)))]
                if e.id == items.open.id()
                    && let Some(p) = pop.as_mut()
                {
                    p.popover.tab = Tab::Now;
                    p.visible = true;
                    app.render(tray.as_ref(), &items, Some(&mut *p));
                    p.show_in_corner();
                    app.render(tray.as_ref(), &items, Some(p));
                }
            }
        }
        _ => {}
    });
}
