//! PicoBar — ultra-light usage monitor for Claude and OpenCode.
//!
//! Skeleton: a native tray / menu bar item with a gauge icon, a title and a
//! native menu. Data sources are stubbed in `sources`; see docs/ARCHITECTURE.md.

#![cfg_attr(windows, windows_subsystem = "windows")]

mod icon;
mod model;
mod sources;

use std::time::{Duration, Instant};

use tao::event::{Event, StartCause};
use tao::event_loop::{ControlFlow, EventLoopBuilder};
use tray_icon::menu::{Menu, MenuEvent, MenuItem, PredefinedMenuItem};
use tray_icon::{TrayIcon, TrayIconBuilder};

const REFRESH: Duration = Duration::from_secs(60);

fn main() {
    let event_loop = EventLoopBuilder::new().build();

    let session = MenuItem::new("Claude session: –", false, None);
    let weekly = MenuItem::new("Claude weekly: –", false, None);
    let opencode = MenuItem::new("OpenCode today: –", false, None);
    let agents = MenuItem::new("Agents: –", false, None);
    let quit = MenuItem::new("Quit PicoBar", true, None);

    let menu = Menu::new();
    menu.append_items(&[
        &session,
        &weekly,
        &opencode,
        &agents,
        &PredefinedMenuItem::separator(),
        &quit,
    ])
    .expect("menu");

    let mut tray: Option<TrayIcon> = None;
    let menu_rx = MenuEvent::receiver();

    event_loop.run(move |event, _, control_flow| {
        *control_flow = ControlFlow::WaitUntil(Instant::now() + REFRESH);

        match event {
            Event::NewEvents(StartCause::Init) => {
                let snap = sources::snapshot();
                tray = Some(
                    TrayIconBuilder::new()
                        .with_menu(Box::new(menu.clone()))
                        .with_icon(icon::gauge(snap.claude.session_pct, snap.needs_attention()))
                        .with_title(snap.title())
                        .with_tooltip("PicoBar")
                        .build()
                        .expect("tray icon"),
                );
                apply(&snap, tray.as_ref(), &session, &weekly, &opencode, &agents);
            }
            Event::NewEvents(StartCause::ResumeTimeReached { .. }) => {
                let snap = sources::snapshot();
                apply(&snap, tray.as_ref(), &session, &weekly, &opencode, &agents);
            }
            _ => {}
        }

        if let Ok(ev) = menu_rx.try_recv()
            && ev.id == quit.id()
        {
            tray.take();
            *control_flow = ControlFlow::Exit;
        }
    });
}

fn apply(
    snap: &model::Snapshot,
    tray: Option<&TrayIcon>,
    session: &MenuItem,
    weekly: &MenuItem,
    opencode: &MenuItem,
    agents: &MenuItem,
) {
    session.set_text(snap.claude.session_line());
    weekly.set_text(snap.claude.weekly_line());
    opencode.set_text(snap.opencode.line());
    agents.set_text(snap.agents_line());
    if let Some(t) = tray {
        let _ = t.set_icon(Some(icon::gauge(
            snap.claude.session_pct,
            snap.needs_attention(),
        )));
        t.set_title(Some(snap.title()));
    }
}
