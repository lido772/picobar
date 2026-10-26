//! The popover: Now / Agents / Trends, drawn to match the design canvas.

use super::paint::{Canvas, Rgb};
use super::text::{Face, Fonts};
use crate::alerts::WARN_PCT;
use crate::model::{AgentRow, AgentState, Snapshot, Trends, human};
use crate::sources::claude_limits::LimitsError;
use crate::update::{self, UpdateState};
use crate::util;

pub const WIDTH: f32 = 380.0;
const P: f32 = 16.0;
const IW: f32 = WIDTH - 2.0 * P;

const BG: Rgb = Rgb::hex(0x16181B);
const CARD: Rgb = Rgb::hex(0x1C1F23);
const BORDER: Rgb = Rgb::hex(0x262A2E);
const TRACK: Rgb = Rgb::hex(0x2A2D31);
const LOGO: Rgb = Rgb::hex(0x1F2226);
const TEXT: Rgb = Rgb::hex(0xECEDEE);
const SUB: Rgb = Rgb::hex(0xB4BAC0);
const MUTED: Rgb = Rgb::hex(0x8E959C);
const MINT: Rgb = Rgb::hex(0x5EE6A8);
const AMBER: Rgb = Rgb::hex(0xF5B547);
const CORAL: Rgb = Rgb::hex(0xFF7A6B);
const BLUE: Rgb = Rgb::hex(0x7AA2F7);
const GREY: Rgb = Rgb::hex(0x6B7177);
const DARK: Rgb = Rgb::hex(0x0B0C0E);
const CORAL_BG: Rgb = Rgb::hex(0x2A1B19);
const CORAL_BORDER: Rgb = Rgb::hex(0x4A2A26);
const CORAL_TEXT: Rgb = Rgb::hex(0xE3B3AC);
const CORAL_LIGHT: Rgb = Rgb::hex(0xF2D6D1);
const AMBER_BG: Rgb = Rgb::hex(0x2E2516);
const HEAT: [Rgb; 5] = [
    Rgb::hex(0x22262A),
    Rgb::hex(0x1F4A3A),
    Rgb::hex(0x2C7A5C),
    Rgb::hex(0x45B486),
    MINT,
];

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Tab {
    #[default]
    Now,
    Agents,
    Trends,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Action {
    Tab(Tab),
    Range(usize),
    Refresh,
    ToggleNotify,
    Quit,
    Update,
    OpenReleasePage,
}

pub struct View<'a> {
    pub snap: &'a Snapshot,
    pub agents: &'a [AgentRow],
    pub trends: &'a Trends,
    pub notify: bool,
    /// Seconds since the limits were last fetched.
    pub updated_ago: Option<i64>,
    pub history_loading: bool,
    pub update: &'a UpdateState,
}

pub struct Popover {
    pub tab: Tab,
    pub range: usize,
    hits: Vec<([f32; 4], Action)>,
}

impl Default for Popover {
    fn default() -> Self {
        Popover {
            tab: Tab::Now,
            range: 7,
            hits: Vec::new(),
        }
    }
}

struct Ui<'a> {
    cv: &'a mut Canvas,
    f: &'a Fonts,
    hits: Vec<([f32; 4], Action)>,
}

impl Ui<'_> {
    fn t(&mut self, face: Face, size: f32, x: f32, y: f32, c: Rgb, s: &str) -> f32 {
        self.f.draw(self.cv, face, size, x, y, c, s)
    }
    fn tr(&mut self, face: Face, size: f32, right: f32, y: f32, c: Rgb, s: &str) {
        self.f.draw_right(self.cv, face, size, right, y, c, s);
    }
    fn tc(&mut self, face: Face, size: f32, cx: f32, y: f32, c: Rgb, s: &str) {
        self.f.draw_center(self.cv, face, size, cx, y, c, s);
    }
    fn fit(&self, face: Face, size: f32, s: &str, max: f32) -> String {
        self.f.fit(face, size, s, max)
    }
    fn hit(&mut self, x: f32, y: f32, w: f32, h: f32, a: Action) {
        self.hits.push(([x, y, w, h], a));
    }
    fn bar(&mut self, x: f32, y: f32, w: f32, h: f32, frac: f32, c: Rgb) {
        self.cv.rrect(x, y, w, h, h / 2.0, TRACK);
        if frac > 0.0 {
            self.cv
                .rrect(x, y, (w * frac.clamp(0.0, 1.0)).max(h), h, h / 2.0, c);
        }
    }
    fn section(&mut self, x: f32, y: f32, c: Rgb, s: &str) {
        let spaced: String = s.chars().flat_map(|ch| [ch, '\u{200A}']).collect();
        self.t(Face::Semibold, 10.5, x, y, c, spaced.trim_end());
    }
    fn pill_badge(&mut self, x: f32, cy: f32, n: usize) -> f32 {
        let s = n.to_string();
        let w = (self.f.width(Face::Semibold, 10.0, &s) + 10.0).max(16.0);
        self.cv.rrect(x, cy - 8.0, w, 16.0, 8.0, CORAL);
        self.tc(
            Face::Semibold,
            10.0,
            x + w / 2.0,
            cy + 3.5,
            Rgb::hex(0x1A0E0C),
            &s,
        );
        w
    }
    fn switch(&mut self, x: f32, y: f32, on: bool) {
        self.cv
            .rrect(x, y, 36.0, 22.0, 11.0, if on { MINT } else { TRACK });
        let kx = if on { x + 16.0 } else { x + 2.0 };
        self.cv
            .circle(kx + 9.0, y + 11.0, 9.0, if on { DARK } else { MUTED });
    }
}

impl Popover {
    /// Action under the logical point (x, y), from the last render.
    pub fn hit_test(&self, x: f32, y: f32) -> Option<Action> {
        self.hits
            .iter()
            .rev()
            .find(|([hx, hy, w, h], _)| x >= *hx && x <= hx + w && y >= *hy && y <= hy + h)
            .map(|(_, a)| *a)
    }

    /// Draw everything; returns the content height in logical pixels.
    pub fn render(&mut self, cv: &mut Canvas, fonts: &Fonts, v: &View) -> f32 {
        cv.clear(BG);
        let mut ui = Ui {
            cv,
            f: fonts,
            hits: Vec::new(),
        };
        let mut y = header(&mut ui, v);
        y = tabs(&mut ui, y, self.tab, v);
        y = match self.tab {
            Tab::Now => now_tab(&mut ui, y, v),
            Tab::Agents => agents_tab(&mut ui, y, v),
            Tab::Trends => trends_tab(&mut ui, y, v, self.range),
        };
        y = footer(&mut ui, y + 4.0, v);
        self.hits = ui.hits;
        y + P
    }
}

fn header(ui: &mut Ui, v: &View) -> f32 {
    let y = P;
    ui.cv.rrect(P, y, 28.0, 28.0, 8.0, LOGO);
    ui.cv.ring(P + 14.0, y + 14.0, 7.5, 2.4, 0.62, MINT, TRACK);
    ui.t(Face::Semibold, 14.0, P + 38.0, y + 12.5, TEXT, "PicoBar");
    let sub = match v.updated_ago {
        Some(s) if s < 60 => format!("Updated {}s ago", s.max(1)),
        Some(s) => format!("Updated {} ago", util::countdown(s)),
        None => "Updating…".into(),
    };
    let sub = format!("{sub} · v{}", update::CURRENT);
    ui.t(Face::Regular, 11.0, P + 38.0, y + 27.0, MUTED, &sub);
    // refresh button
    let bx = WIDTH - P - 32.0;
    refresh_icon(ui, bx + 16.0, y + 14.0);
    ui.hit(bx, y - 2.0, 32.0, 32.0, Action::Refresh);
    y + 40.0
}

/// Circular arrow: an open ring with an arrowhead at the end of the arc.
fn refresh_icon(ui: &mut Ui, cx: f32, cy: f32) {
    let (r, width, frac) = (6.5, 1.8, 0.8);
    ui.cv.ring(cx, cy, r, width, frac, SUB, BG);
    let theta = frac * std::f32::consts::TAU;
    let rm = r - width / 2.0;
    let p = (cx + rm * theta.sin(), cy - rm * theta.cos());
    let t = (theta.cos(), theta.sin()); // clockwise tangent
    let n = (theta.sin(), -theta.cos()); // outward normal
    let tip = (p.0 + t.0 * 3.6, p.1 + t.1 * 3.6);
    let b1 = (p.0 + n.0 * 3.4 - t.0 * 0.6, p.1 + n.1 * 3.4 - t.1 * 0.6);
    let b2 = (p.0 - n.0 * 3.4 - t.0 * 0.6, p.1 - n.1 * 3.4 - t.1 * 0.6);
    ui.cv.tri(tip, b1, b2, SUB);
}

fn tabs(ui: &mut Ui, y: f32, active: Tab, v: &View) -> f32 {
    ui.cv.panel(P, y, IW, 34.0, 9.0, CARD, BORDER);
    let w = (IW - 4.0) / 3.0;
    let waiting = v.snap.waiting;
    for (i, (tab, label)) in [
        (Tab::Now, "Now"),
        (Tab::Agents, "Agents"),
        (Tab::Trends, "Trends"),
    ]
    .into_iter()
    .enumerate()
    {
        let x = P + 2.0 + i as f32 * w;
        if tab == active {
            ui.cv.rrect(x, y + 2.0, w, 30.0, 7.0, TRACK);
        }
        let color = if tab == active { TEXT } else { SUB };
        let lw = ui.f.width(Face::Semibold, 12.0, label);
        let badge = tab == Tab::Agents && waiting > 0;
        let total = lw + if badge { 22.0 } else { 0.0 };
        let lx = x + (w - total) / 2.0;
        ui.t(Face::Semibold, 12.0, lx, y + 21.5, color, label);
        if badge {
            ui.pill_badge(lx + lw + 6.0, y + 17.0, waiting);
        }
        ui.hit(x, y, w, 34.0, Action::Tab(tab));
    }
    y + 46.0
}

fn footer(ui: &mut Ui, y: f32, v: &View) -> f32 {
    ui.switch(P, y + 5.0, v.notify);
    ui.t(
        Face::Regular,
        12.0,
        P + 46.0,
        y + 20.0,
        SUB,
        "Notify on resets and at 80%",
    );
    ui.hit(P, y, 240.0, 32.0, Action::ToggleNotify);
    let qw = 52.0;
    ui.cv
        .panel(WIDTH - P - qw, y + 2.0, qw, 28.0, 8.0, BG, TRACK);
    ui.tc(
        Face::Regular,
        12.0,
        WIDTH - P - qw / 2.0,
        y + 20.0,
        SUB,
        "Quit",
    );
    ui.hit(WIDTH - P - qw, y + 2.0, qw, 28.0, Action::Quit);
    y + 32.0
}

// ------------------------------------------------------------------ Now

fn update_banner(ui: &mut Ui, y: f32, state: &UpdateState) -> f32 {
    let (title, sub, button) = match state {
        UpdateState::Available(r) => (
            format!("PicoBar {} is available", r.version),
            format!("You have {}", update::CURRENT),
            Some(if r.installable() {
                ("Update", Action::Update)
            } else {
                ("Download", Action::OpenReleasePage)
            }),
        ),
        UpdateState::Installing(v) => (
            format!("Updating to {v}…"),
            "PicoBar restarts by itself".into(),
            None,
        ),
        UpdateState::Failed(e) => (
            "Update failed".into(),
            e.clone(),
            Some(("Open page", Action::OpenReleasePage)),
        ),
        _ => return y,
    };
    ui.cv
        .panel(P, y, IW, 50.0, 10.0, Rgb::hex(0x18233A), Rgb::hex(0x2C3E63));
    ui.cv.rrect(P + 12.0, y + 11.0, 28.0, 28.0, 8.0, BLUE);
    let ink = Rgb::hex(0x0E1730);
    ui.cv.rrect(P + 24.5, y + 17.0, 3.0, 11.0, 1.5, ink);
    ui.cv.tri(
        (P + 26.0, y + 33.5),
        (P + 20.0, y + 26.0),
        (P + 32.0, y + 26.0),
        ink,
    );
    ui.t(Face::Semibold, 12.0, P + 50.0, y + 22.0, TEXT, &title);
    let max = IW - 62.0 - if button.is_some() { 80.0 } else { 0.0 };
    let sub = ui.fit(Face::Regular, 11.0, &sub, max);
    ui.t(
        Face::Regular,
        11.0,
        P + 50.0,
        y + 38.0,
        Rgb::hex(0xA9BCE6),
        &sub,
    );
    if let Some((label, action)) = button {
        let bw = ui.f.width(Face::Semibold, 12.0, label) + 24.0;
        let bx = WIDTH - P - 10.0 - bw;
        ui.cv.rrect(bx, y + 11.0, bw, 28.0, 8.0, BLUE);
        ui.tc(Face::Semibold, 12.0, bx + bw / 2.0, y + 29.5, ink, label);
        ui.hit(bx, y + 11.0, bw, 28.0, action);
    }
    y + 62.0
}

fn now_tab(ui: &mut Ui, mut y: f32, v: &View) -> f32 {
    let s = v.snap;
    y = update_banner(ui, y, v.update);
    let waiting: Vec<&AgentRow> = v
        .agents
        .iter()
        .filter(|a| matches!(a.state, AgentState::YourTurn | AgentState::Stopped))
        .collect();
    if let Some(first) = waiting.first() {
        ui.cv.panel(P, y, IW, 50.0, 10.0, CORAL_BG, CORAL_BORDER);
        bubble(ui, P + 12.0, y + 11.0);
        let title = match waiting.len() {
            1 => "1 agent is waiting for you".to_owned(),
            n => format!("{n} agents are waiting for you"),
        };
        ui.t(Face::Semibold, 12.0, P + 50.0, y + 22.0, TEXT, &title);
        let detail = if first.preview.is_empty() {
            format!("{} · {}", first.name, first.tool)
        } else {
            format!("{} · {}", first.name, first.preview)
        };
        let detail = ui.fit(Face::Regular, 11.0, &detail, IW - 80.0);
        ui.t(Face::Regular, 11.0, P + 50.0, y + 38.0, CORAL_TEXT, &detail);
        ui.t(
            Face::Semibold,
            14.0,
            WIDTH - P - 18.0,
            y + 30.0,
            CORAL_TEXT,
            "›",
        );
        ui.hit(P, y, IW, 50.0, Action::Tab(Tab::Agents));
        y += 62.0;
    }

    // Claude card
    let top = y;
    let x = P + 14.0;
    let r = WIDTH - P - 14.0;
    let w = r - x;
    let mut cy = top + 14.0;
    let card_h = claude_card_height(s);
    ui.cv.panel(P, top, IW, card_h, 12.0, CARD, BORDER);

    let adv = ui.t(Face::Semibold, 13.0, x, cy + 11.0, TEXT, "Claude");
    if let Some(plan) = s.limits.as_ref().and_then(|l| l.plan.as_deref()) {
        let plan = capitalize(plan);
        ui.t(Face::Regular, 11.0, x + adv + 8.0, cy + 11.0, MUTED, &plan);
    }
    let status = s.status();
    let scol = if status == "Live" { MINT } else { MUTED };
    let sw = ui.f.width(Face::Semibold, 11.0, status);
    ui.cv.circle(r - sw - 9.0, cy + 7.0, 3.0, scol);
    ui.tr(Face::Semibold, 11.0, r, cy + 11.0, scol, status);
    cy += 26.0;

    // session
    ui.t(
        Face::Regular,
        12.0,
        x,
        cy + 20.0,
        SUB,
        "Session · 5h window",
    );
    let pct = s.session().map(|b| b.pct);
    match pct {
        Some(p) => {
            let pw = ui.f.width(Face::Regular, 16.0, "%");
            ui.tr(Face::Regular, 16.0, r, cy + 22.0, MUTED, "%");
            ui.tr(
                Face::MonoBold,
                28.0,
                r - pw + 1.5,
                cy + 22.0,
                TEXT,
                &format!("{p:.0}"),
            );
        }
        None => ui.tr(Face::MonoBold, 28.0, r, cy + 22.0, MUTED, "–"),
    }
    cy += 32.0;
    let pc = pct.unwrap_or(0.0);
    ui.bar(
        x,
        cy,
        w,
        8.0,
        pc / 100.0,
        if pc >= WARN_PCT { AMBER } else { MINT },
    );
    cy += 14.0;
    match s.session().and_then(|b| b.resets_at) {
        Some(reset) => {
            let a = ui.t(Face::Regular, 11.0, x, cy + 10.0, MUTED, "Resets in ");
            ui.t(
                Face::Mono,
                11.0,
                x + a,
                cy + 10.0,
                TEXT,
                &util::countdown(reset - s.now),
            );
            ui.tr(
                Face::Regular,
                11.0,
                r,
                cy + 10.0,
                MUTED,
                &format!("at {}", util::local_hm(reset)),
            );
        }
        None => {
            let note = match s.limits_error {
                Some(LimitsError::NoLogin) => "Sign in to Claude Code to see your limits",
                Some(LimitsError::Expired) => "Open Claude Code to refresh your login",
                Some(LimitsError::Offline) => "Offline · showing local data",
                Some(LimitsError::Unexpected) => "Limits unavailable right now",
                None => "No active session window",
            };
            ui.t(Face::Regular, 11.0, x, cy + 10.0, MUTED, note);
        }
    }
    cy += 22.0;

    // token counter
    let t = s.tokens;
    let cw = (w - 18.0) / 4.0;
    for (i, (val, label)) in [
        (t.total(), "total"),
        (t.input, "input"),
        (t.output, "output"),
        (t.cache(), "cache"),
    ]
    .into_iter()
    .enumerate()
    {
        let cx = x + i as f32 * (cw + 6.0);
        ui.cv.rrect(cx, cy, cw, 42.0, 8.0, BG);
        ui.t(Face::MonoBold, 13.0, cx + 8.0, cy + 19.0, TEXT, &human(val));
        ui.t(Face::Regular, 10.0, cx + 8.0, cy + 33.0, MUTED, label);
    }
    cy += 50.0;
    let a = ui.t(Face::Regular, 11.0, x, cy + 10.0, MUTED, "Burn ");
    match s.burn() {
        Some(b) => {
            ui.t(
                Face::Mono,
                11.0,
                x + a,
                cy + 10.0,
                TEXT,
                &format!("{} tok/h", human(b.per_hour)),
            );
            match b.limit_at {
                Some(at) => ui.tr(
                    Face::Regular,
                    11.0,
                    r,
                    cy + 10.0,
                    AMBER,
                    &format!("Limit ~{} at this pace", util::local_hm(at)),
                ),
                None => ui.tr(
                    Face::Regular,
                    11.0,
                    r,
                    cy + 10.0,
                    MUTED,
                    "On pace to stay under",
                ),
            }
        }
        None => {
            // No session window from Claude: average over the last 5 hours.
            let per_hour = s.tokens.total() / 5;
            ui.t(
                Face::Mono,
                11.0,
                x + a,
                cy + 10.0,
                TEXT,
                &format!("{} tok/h", human(per_hour)),
            );
            ui.tr(
                Face::Regular,
                11.0,
                r,
                cy + 10.0,
                MUTED,
                "avg over the last 5h",
            );
        }
    }
    cy += 22.0;

    // weekly
    if let Some(wk) = s.limits.as_ref().and_then(|l| l.weekly) {
        ui.cv.hline(x, cy, w, BORDER);
        cy += 12.0;
        cy = weekly_row(ui, x, r, cy, "Weekly · all models", wk.pct, TEXT);
        let reset = wk
            .resets_at
            .map(|t| format!("Resets {}", util::local_day_hm(t)));
        if let Some(reset) = reset {
            ui.t(Face::Regular, 11.0, x, cy + 10.0, MUTED, &reset);
            cy += 18.0;
        }
        for (name, b) in s.model_buckets() {
            cy += 4.0;
            cy = weekly_row(ui, x, r, cy, &format!("Weekly · {name}"), b.pct, TEXT);
        }
    }
    let _ = cy;
    y = top + card_h + 12.0;

    // OpenCode card
    opencode_card(ui, y, v)
}

fn claude_card_height(s: &Snapshot) -> f32 {
    let mut h = 14.0 + 26.0 + 32.0 + 14.0 + 22.0 + 50.0 + 22.0;
    if let Some(wk) = s.limits.as_ref().and_then(|l| l.weekly) {
        h += 12.0 + 30.0 + if wk.resets_at.is_some() { 18.0 } else { 0.0 };
        h += s.model_buckets().len() as f32 * 34.0;
    }
    h + 4.0
}

fn weekly_row(ui: &mut Ui, x: f32, r: f32, y: f32, label: &str, pct: f32, base: Rgb) -> f32 {
    ui.t(Face::Regular, 12.0, x, y + 11.0, SUB, label);
    let warn = pct >= WARN_PCT;
    ui.tr(
        Face::MonoBold,
        12.0,
        r,
        y + 11.0,
        if warn { AMBER } else { TEXT },
        &format!("{pct:.0}%"),
    );
    ui.bar(
        x,
        y + 18.0,
        r - x,
        6.0,
        pct / 100.0,
        if warn { AMBER } else { base },
    );
    y + 30.0
}

fn opencode_card(ui: &mut Ui, y: f32, v: &View) -> f32 {
    let x = P + 14.0;
    let r = WIDTH - P - 14.0;
    let Some(oc) = &v.snap.opencode else {
        ui.cv.panel(P, y, IW, 48.0, 12.0, CARD, BORDER);
        ui.t(Face::Semibold, 13.0, x, y + 29.0, TEXT, "OpenCode");
        ui.tr(
            Face::Regular,
            11.0,
            r,
            y + 29.0,
            MUTED,
            "not found on this computer",
        );
        return y + 60.0;
    };
    let h = if oc.providers.is_empty() {
        108.0
    } else {
        150.0
    };
    ui.cv.panel(P, y, IW, h, 12.0, CARD, BORDER);
    let a = ui.t(Face::Semibold, 13.0, x, y + 25.0, TEXT, "OpenCode");
    ui.t(
        Face::Regular,
        11.0,
        x + a + 8.0,
        y + 25.0,
        MUTED,
        "Last 24h",
    );
    let running = match oc.active {
        0 => "no session running".to_owned(),
        1 => "1 running".to_owned(),
        n => format!("{n} running"),
    };
    ui.tr(Face::Regular, 11.0, r, y + 25.0, MUTED, &running);

    let by = y + 64.0;
    let w1 = ui.t(Face::MonoBold, 22.0, x, by, TEXT, &human(oc.tokens));
    ui.t(Face::Regular, 11.0, x, by + 16.0, MUTED, "tokens");
    let cx = x + w1.max(60.0) + 20.0;
    ui.t(
        Face::MonoBold,
        22.0,
        cx,
        by,
        TEXT,
        &format!("${:.2}", oc.cost),
    );
    ui.t(Face::Regular, 11.0, cx, by + 16.0, MUTED, "est. cost");

    // 7-day sparkline
    let today = util::local_day(v.snap.now);
    let vals: Vec<u64> = (today - 6..=today)
        .map(|d| {
            oc.daily
                .iter()
                .find(|(dd, _)| *dd == d)
                .map_or(0, |(_, t)| *t)
        })
        .collect();
    let max = vals.iter().copied().max().unwrap_or(0).max(1);
    if vals.iter().all(|v| *v == 0) {
        ui.tr(Face::Regular, 11.0, r, by, MUTED, "no activity this week");
    }
    for (i, val) in vals.iter().enumerate().filter(|_| max > 1) {
        let bh = (*val as f32 / max as f32 * 36.0).max(3.0);
        let bx = r - (7 - i) as f32 * 9.0 + 3.0;
        ui.cv.rrect(
            bx,
            by + 14.0 - bh,
            6.0,
            bh,
            2.0,
            if i == 6 { MINT } else { Rgb::hex(0x33373C) },
        );
    }

    if !oc.providers.is_empty() {
        let total: u64 = oc.providers.iter().map(|p| p.1).sum::<u64>().max(1);
        let colors = [BLUE, AMBER, MINT, GREY];
        let mut px = x;
        let sy = by + 32.0;
        let w = r - x;
        for (i, (_, t)) in oc.providers.iter().enumerate() {
            let seg = w * *t as f32 / total as f32;
            if seg >= 1.0 {
                ui.cv
                    .rrect(px, sy, (seg - 2.0).max(1.0), 6.0, 3.0, colors[i % 4]);
            }
            px += seg;
        }
        let mut lx = x;
        for (i, (name, t)) in oc.providers.iter().enumerate().take(3) {
            ui.cv.rrect(lx, sy + 15.0, 8.0, 8.0, 2.0, colors[i % 4]);
            let label = format!("{name} {:.0}%", *t as f32 * 100.0 / total as f32);
            let adv = ui.t(Face::Regular, 11.0, lx + 14.0, sy + 23.0, SUB, &label);
            lx += 14.0 + adv + 14.0;
        }
    }
    y + h + 12.0
}

fn bubble(ui: &mut Ui, x: f32, y: f32) {
    ui.cv.rrect(x, y, 28.0, 28.0, 8.0, CORAL);
    let ink = Rgb::hex(0x1A0E0C);
    ui.cv.rrect(x + 6.0, y + 7.0, 16.0, 12.0, 6.0, ink);
    ui.cv.rrect(x + 7.0, y + 15.0, 5.0, 6.0, 1.5, ink);
    for i in 0..3 {
        ui.cv
            .circle(x + 10.0 + i as f32 * 4.0, y + 13.0, 1.2, CORAL);
    }
}

fn capitalize(s: &str) -> String {
    let mut c = s.chars();
    c.next()
        .map(|f| f.to_uppercase().chain(c).collect())
        .unwrap_or_default()
}

// --------------------------------------------------------------- Agents

fn agents_tab(ui: &mut Ui, mut y: f32, v: &View) -> f32 {
    let count = |f: &dyn Fn(&AgentRow) -> bool| v.agents.iter().filter(|a| f(a)).count();
    let yours = count(&|a| matches!(a.state, AgentState::YourTurn | AgentState::Stopped));
    let running = count(&|a| a.state == AgentState::Running);
    let idle = count(&|a| a.state == AgentState::Idle);
    let subagents: usize = v.agents.iter().map(|a| a.subagents).sum();
    let cw = (IW - 18.0) / 4.0;
    for (i, (n, label, c)) in [
        (yours, "your turn", CORAL),
        (running, "running", MINT),
        (idle, "idle", TEXT),
        (subagents, "subagents", BLUE),
    ]
    .into_iter()
    .enumerate()
    {
        let x = P + i as f32 * (cw + 6.0);
        ui.cv.panel(x, y, cw, 44.0, 8.0, CARD, BORDER);
        ui.t(Face::MonoBold, 16.0, x + 9.0, y + 21.0, c, &n.to_string());
        ui.t(Face::Regular, 10.0, x + 9.0, y + 35.0, MUTED, label);
    }
    y += 56.0;

    if v.agents.is_empty() {
        ui.cv.panel(P, y, IW, 70.0, 12.0, CARD, BORDER);
        ui.tc(
            Face::Semibold,
            13.0,
            WIDTH / 2.0,
            y + 30.0,
            SUB,
            "No agent activity in the last hour",
        );
        ui.tc(
            Face::Regular,
            11.0,
            WIDTH / 2.0,
            y + 48.0,
            MUTED,
            "Claude Code and OpenCode sessions show up here",
        );
        return y + 82.0;
    }

    let waiting: Vec<&AgentRow> = v
        .agents
        .iter()
        .filter(|a| matches!(a.state, AgentState::YourTurn | AgentState::Stopped))
        .take(3)
        .collect();
    if !waiting.is_empty() {
        ui.section(P, y + 10.0, CORAL, "YOUR TURN");
        y += 18.0;
        for a in waiting {
            let h = if a.preview.is_empty() { 50.0 } else { 68.0 };
            ui.cv.panel(P, y, IW, h, 12.0, CORAL_BG, CORAL_BORDER);
            let x = P + 12.0;
            let r = WIDTH - P - 12.0;
            let nw = ui.t(
                Face::Semibold,
                13.0,
                x,
                y + 21.0,
                TEXT,
                &ui.fit(Face::Semibold, 13.0, &a.name, 150.0),
            );
            let meta = format!("{} · {}", a.tool, a.model);
            let meta = ui.fit(Face::Regular, 11.0, &meta, r - x - nw - 60.0);
            ui.t(
                Face::Regular,
                11.0,
                x + nw + 8.0,
                y + 21.0,
                CORAL_TEXT,
                &meta,
            );
            ui.tr(
                Face::Mono,
                11.0,
                r,
                y + 21.0,
                CORAL_TEXT,
                &util::countdown(a.since),
            );
            let status = if a.state == AgentState::Stopped {
                "Stopped: needs a look"
            } else {
                "Finished its turn, waiting for you"
            };
            ui.t(Face::Regular, 12.0, x, y + 39.0, CORAL_LIGHT, status);
            if !a.preview.is_empty() {
                let p = ui.fit(Face::Regular, 11.0, &format!("“{}”", a.preview), r - x);
                ui.t(Face::Regular, 11.0, x, y + 56.0, CORAL_TEXT, &p);
            }
            y += h + 8.0;
        }
        y += 4.0;
    }

    let rows: Vec<&AgentRow> = v
        .agents
        .iter()
        .filter(|a| matches!(a.state, AgentState::Running | AgentState::Idle))
        .take(6)
        .collect();
    if !rows.is_empty() {
        ui.section(P, y + 10.0, MUTED, "RUNNING & IDLE");
        y += 18.0;
        let h = rows.len() as f32 * 48.0;
        ui.cv.panel(P, y, IW, h, 12.0, CARD, BORDER);
        for (i, a) in rows.iter().enumerate() {
            let ry = y + i as f32 * 48.0;
            if i > 0 {
                ui.cv.hline(P + 1.0, ry, IW - 2.0, BORDER);
            }
            let x = P + 12.0;
            let r = WIDTH - P - 12.0;
            let dot = if a.state == AgentState::Running {
                MINT
            } else {
                GREY
            };
            if a.state == AgentState::Running {
                ui.cv.circle(x + 4.0, ry + 24.0, 6.5, Rgb::hex(0x22433A));
            }
            ui.cv.circle(x + 4.0, ry + 24.0, 4.0, dot);
            let nx = x + 18.0;
            let nw = ui.t(
                Face::Semibold,
                12.0,
                nx,
                ry + 20.0,
                TEXT,
                &ui.fit(Face::Semibold, 12.0, &a.name, 130.0),
            );
            let meta = ui.fit(
                Face::Regular,
                11.0,
                &format!("{} · {}", a.tool, a.model),
                r - nx - nw - 70.0,
            );
            ui.t(Face::Regular, 11.0, nx + nw + 6.0, ry + 20.0, MUTED, &meta);
            let detail = match (a.state, a.subagents) {
                (AgentState::Running, 0) => "Working".to_owned(),
                (AgentState::Running, 1) => "Working · 1 subagent".to_owned(),
                (AgentState::Running, n) => format!("Working · {n} subagents"),
                _ => format!("Idle for {}", util::countdown(a.since)),
            };
            ui.t(Face::Regular, 11.0, nx, ry + 36.0, MUTED, &detail);
            ui.tr(Face::MonoBold, 12.0, r, ry + 20.0, TEXT, &human(a.tokens));
            ui.tr(
                Face::Regular,
                11.0,
                r,
                ry + 36.0,
                MUTED,
                if a.tool == "OpenCode" {
                    "session"
                } else {
                    "last 5h"
                },
            );
        }
        y += h + 12.0;
    }
    y
}

// --------------------------------------------------------------- Trends

fn trends_tab(ui: &mut Ui, mut y: f32, v: &View, range: usize) -> f32 {
    let t = v.trends;
    // range picker
    let sw = 92.0;
    let sx = WIDTH - P - sw;
    ui.cv.panel(sx, y, sw, 28.0, 8.0, CARD, BORDER);
    for (i, (r, label)) in [(7usize, "7 days"), (30, "30 days")]
        .into_iter()
        .enumerate()
    {
        let bx = sx + 2.0 + i as f32 * 44.0;
        if r == range {
            ui.cv.rrect(bx, y + 2.0, 44.0, 24.0, 6.0, TRACK);
        }
        ui.tc(
            Face::Regular,
            11.0,
            bx + 22.0,
            y + 18.0,
            if r == range { TEXT } else { SUB },
            label,
        );
        ui.hit(bx, y, 44.0, 28.0, Action::Range(r));
    }
    if v.history_loading {
        ui.t(Face::Regular, 11.0, P, y + 18.0, MUTED, "Reading history…");
    }
    y += 38.0;

    // tokens card
    let x = P + 14.0;
    let r = WIDTH - P - 14.0;
    let ch = 214.0;
    ui.cv.panel(P, y, IW, ch, 12.0, CARD, BORDER);
    ui.t(
        Face::Regular,
        12.0,
        x,
        y + 24.0,
        SUB,
        &format!("Tokens · last {} days", t.range),
    );
    ui.t(Face::MonoBold, 26.0, x, y + 54.0, TEXT, &human(t.total()));
    if let Some(c) = t.change {
        let label = format!("{}{:.0}% vs prev", if c >= 0.0 { "+" } else { "" }, c);
        let lw = ui.f.width(Face::Semibold, 11.0, &label) + 16.0;
        let (bg, fg) = if c >= 0.0 {
            (AMBER_BG, AMBER)
        } else {
            (Rgb::hex(0x15302A), MINT)
        };
        ui.cv.rrect(r - lw, y + 14.0, lw, 22.0, 6.0, bg);
        ui.tc(Face::Semibold, 11.0, r - lw / 2.0, y + 29.0, fg, &label);
    }
    // stacked bars
    let top = y + 70.0;
    let bh = 90.0;
    let n = t.days.len().max(1);
    let gap = if n > 10 { 3.0 } else { 10.0 };
    let bw = ((r - x) - gap * (n as f32 - 1.0)) / n as f32;
    let max = t
        .days
        .iter()
        .map(|d| d.claude + d.opencode)
        .max()
        .unwrap_or(0)
        .max(1);
    for (i, d) in t.days.iter().enumerate() {
        let bx = x + i as f32 * (bw + gap);
        let alpha = if d.today { 1.0 } else { 0.55 };
        let hc = d.claude as f32 / max as f32 * bh;
        let ho = d.opencode as f32 / max as f32 * bh;
        if hc + ho < 2.0 {
            ui.cv.rrect(bx, top + bh - 2.0, bw, 2.0, 1.0, TRACK);
        }
        if hc > 0.5 {
            ui.cv.rrect_a(
                bx,
                top + bh - hc,
                bw,
                hc,
                3.0_f32.min(bw / 2.0),
                MINT,
                alpha,
            );
        }
        if ho > 0.5 {
            ui.cv.rrect_a(
                bx,
                top + bh - hc - ho - 1.0,
                bw,
                ho,
                3.0_f32.min(bw / 2.0),
                BLUE,
                alpha,
            );
        }
        if !d.label.is_empty() {
            ui.tc(
                Face::Regular,
                10.0,
                bx + bw / 2.0,
                top + bh + 14.0,
                if d.today { TEXT } else { MUTED },
                &d.label,
            );
        }
    }
    if t.range > 7 {
        ui.t(
            Face::Regular,
            10.0,
            x,
            top + bh + 14.0,
            MUTED,
            &format!("{} days ago", t.range - 1),
        );
        ui.tr(Face::Regular, 10.0, r, top + bh + 14.0, TEXT, "today");
    }
    let ly = top + bh + 34.0;
    ui.cv.rrect(x, ly - 8.0, 8.0, 8.0, 2.0, MINT);
    let a = ui.t(
        Face::Regular,
        11.0,
        x + 14.0,
        ly,
        SUB,
        &format!("Claude {}", human(t.claude_total)),
    );
    let ox = x + 14.0 + a + 14.0;
    ui.cv.rrect(ox, ly - 8.0, 8.0, 8.0, 2.0, BLUE);
    ui.t(
        Face::Regular,
        11.0,
        ox + 14.0,
        ly,
        SUB,
        &format!("OpenCode {}", human(t.opencode_total)),
    );
    ui.tr(
        Face::Regular,
        11.0,
        r,
        ly,
        MUTED,
        &format!("avg {}/day", human(t.total() / t.range.max(1) as u64)),
    );
    y += ch + 12.0;

    // burn card
    if let Some(b) = v.snap.burn() {
        let h = 112.0;
        ui.cv.panel(P, y, IW, h, 12.0, CARD, BORDER);
        ui.t(
            Face::Regular,
            12.0,
            x,
            y + 24.0,
            SUB,
            "Burn rate · this session",
        );
        let unit = " tok/h";
        let uw = ui.f.width(Face::Regular, 11.0, unit);
        ui.tr(Face::Regular, 11.0, r, y + 24.0, MUTED, unit);
        ui.tr(
            Face::MonoBold,
            14.0,
            r - uw,
            y + 24.0,
            TEXT,
            &human(b.per_hour),
        );
        let by = y + 38.0;
        let w = r - x;
        ui.cv.rrect(x, by, w, 10.0, 5.0, TRACK);
        let proj = (b.pct_at_reset / 100.0).clamp(0.0, 1.0);
        let now = (b.pct_now / 100.0).clamp(0.0, 1.0);
        if proj > now {
            ui.cv.rrect_a(x, by, w * proj, 10.0, 5.0, AMBER, 0.45);
        }
        ui.cv.rrect(x, by, (w * now).max(10.0), 10.0, 5.0, MINT);
        ui.t(
            Face::Regular,
            11.0,
            x,
            by + 26.0,
            MUTED,
            &format!("Now {:.0}%", b.pct_now),
        );
        ui.tr(
            Face::Regular,
            11.0,
            r,
            by + 26.0,
            MUTED,
            &format!("Reset {}", util::local_hm(b.reset_at)),
        );
        let line = match b.limit_at {
            Some(at) => format!(
                "At this pace you hit the limit around {}, {} before reset.",
                util::local_hm(at),
                util::countdown(b.reset_at - at)
            ),
            None => format!(
                "At this pace you reach about {:.0}% by the reset.",
                b.pct_at_reset.min(100.0)
            ),
        };
        let line = ui.fit(Face::Regular, 12.0, &line, w);
        ui.t(
            Face::Regular,
            12.0,
            x,
            by + 50.0,
            if b.limit_at.is_some() {
                AMBER
            } else {
                Rgb::hex(0xC9CED3)
            },
            &line,
        );
        y += h + 12.0;
    }

    // heatmap
    let h = 172.0;
    ui.cv.panel(P, y, IW, h, 12.0, CARD, BORDER);
    ui.t(
        Face::Regular,
        12.0,
        x,
        y + 24.0,
        SUB,
        "When you burn · last 4 weeks",
    );
    if let Some(p) = &t.peak {
        ui.tr(
            Face::Regular,
            11.0,
            r,
            y + 24.0,
            MUTED,
            &format!("peak {p}"),
        );
    }
    let max = t.heat.iter().flatten().copied().max().unwrap_or(0).max(1);
    let gx = x + 16.0;
    let cw = (r - gx - 11.0 * 3.0) / 12.0;
    for (row, vals) in t.heat.iter().enumerate() {
        let ry = y + 38.0 + row as f32 * 15.0;
        ui.t(
            Face::Regular,
            10.0,
            x,
            ry + 10.0,
            MUTED,
            &util::WEEKDAYS[row][..1],
        );
        for (col, val) in vals.iter().enumerate() {
            let level = if *val == 0 {
                0
            } else {
                1 + ((*val as f32 / max as f32) * 3.99) as usize
            };
            ui.cv.rrect(
                gx + col as f32 * (cw + 3.0),
                ry,
                cw,
                12.0,
                3.0,
                HEAT[level.min(4)],
            );
        }
    }
    let ay = y + 38.0 + 7.0 * 15.0 + 12.0;
    ui.t(Face::Regular, 10.0, gx, ay, MUTED, "00");
    ui.tc(Face::Regular, 10.0, gx + 6.0 * (cw + 3.0), ay, MUTED, "12");
    ui.tr(Face::Regular, 10.0, r, ay, MUTED, "22");
    y += h + 12.0;

    for line in t.insights.iter().take(3) {
        ui.cv.circle(P + 3.0, y + 6.0, 2.5, MINT);
        let line = ui.fit(Face::Regular, 12.0, line, IW - 14.0);
        ui.t(
            Face::Regular,
            12.0,
            P + 14.0,
            y + 10.0,
            Rgb::hex(0xC9CED3),
            &line,
        );
        y += 20.0;
    }
    y + 6.0
}
