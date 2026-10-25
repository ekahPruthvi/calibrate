use gtk4::prelude::*;
use gtk4::{
    Align, Box as GtkBox, Button, CssProvider, DrawingArea, DropDown, Fixed, Frame, GestureDrag, Grid,
    Label, Orientation, Overlay, Revealer, RevealerTransitionType, ScrolledWindow, StringList,
    Switch, gdk, glib, pango,
};
use niri_ipc::{socket::Socket, Request, Response, Transform};
use std::cell::{Cell, RefCell};
use std::fs;
use std::path::Path;
use std::rc::Rc;
use std::time::Duration;

use crate::home::page_scroller;

// Same file the cursor settings write to. If your output blocks live in a separate
// included file (e.g. display.kdl), point this at that file instead.
const CFGPATH: &str = "/var/lib/cynager/niri/display.kdl";

const CANVAS_W: i32 = 1400;
const CANVAS_H: i32 = 480;
const PX: f64 = 0.12; // canvas pixels per logical pixel
const SNAP: f64 = 80.0; // snap distance in logical pixels

const SCALES: [f64; 9] = [0.5, 0.75, 1.0, 1.25, 1.5, 1.75, 2.0, 2.5, 3.0];
const ROTATIONS: [&str; 4] = ["Normal", "90°", "180°", "270°"];
const TRANSFORM_KDL: [&str; 4] = ["normal", "90", "180", "270"];

// ───────────────────────────── toast ─────────────────────────────

#[derive(Clone)]
struct Toast {
    revealer: Revealer,
    label: Label,
    generation: Rc<RefCell<u32>>,
}

impl Toast {
    fn new() -> Self {
        let label = Label::new(None);
        label.add_css_class("shell-toast-label");

        let bubble = GtkBox::new(Orientation::Horizontal, 0);
        bubble.add_css_class("shell-toast");
        bubble.append(&label);

        let revealer = Revealer::new();
        revealer.set_transition_type(RevealerTransitionType::SlideUp);
        revealer.set_transition_duration(200);
        revealer.set_halign(Align::Center);
        revealer.set_valign(Align::End);
        revealer.set_margin_bottom(24);
        revealer.set_can_target(false);
        revealer.set_child(Some(&bubble));

        Self { revealer, label, generation: Rc::new(RefCell::new(0)) }
    }

    fn show(&self, text: &str) {
        self.label.set_text(text);
        self.revealer.set_reveal_child(true);

        let id = {
            let mut g = self.generation.borrow_mut();
            *g += 1;
            *g
        };
        let this = self.clone();
        glib::timeout_add_local_once(Duration::from_millis(2500), move || {
            if *this.generation.borrow() == id {
                this.revealer.set_reveal_child(false);
            }
        });
    }
}

// ───────────────────────────── data model ─────────────────────────────

#[derive(Clone, Copy, PartialEq)]
struct ModeOpt {
    w: u32,
    h: u32,
    mhz: u32, // refresh rate in millihertz, as reported by niri
}

#[derive(Clone)]
struct MonState {
    name: String,
    label: String,
    modes: Vec<ModeOpt>,
    mode: usize, // index into modes
    enabled: bool,
    scale: f64,
    rot: usize, // index into ROTATIONS
    x: i32,
    y: i32,
}

impl MonState {
    fn cur(&self) -> ModeOpt {
        self.modes[self.mode]
    }

    /// Size in logical pixels (after rotation and scale).
    fn logical(&self) -> (f64, f64) {
        let m = self.cur();
        let (mut w, mut h) = (m.w as f64, m.h as f64);
        if self.rot % 2 == 1 {
            std::mem::swap(&mut w, &mut h);
        }
        (w / self.scale, h / self.scale)
    }
}

fn fmt_trim(v: f64, decimals: usize) -> String {
    let s = format!("{:.*}", decimals, v);
    s.trim_end_matches('0').trim_end_matches('.').to_string()
}

fn fmt_hz(mhz: u32) -> String {
    fmt_trim(mhz as f64 / 1000.0, 3)
}

fn resolutions(modes: &[ModeOpt]) -> Vec<(u32, u32)> {
    let mut r: Vec<(u32, u32)> = Vec::new();
    for m in modes {
        if !r.contains(&(m.w, m.h)) {
            r.push((m.w, m.h));
        }
    }
    r.sort_by(|a, b| (b.0 * b.1).cmp(&(a.0 * a.1)).then(b.0.cmp(&a.0)));
    r
}

fn refresh_idxs(modes: &[ModeOpt], res: (u32, u32)) -> Vec<usize> {
    let mut v: Vec<usize> = modes
        .iter()
        .enumerate()
        .filter(|(_, m)| m.w == res.0 && m.h == res.1)
        .map(|(i, _)| i)
        .collect();
    v.sort_by(|&a, &b| modes[b].mhz.cmp(&modes[a].mhz));
    v
}

fn pick_mode(modes: &[ModeOpt], w: u32, h: u32, hz: Option<f64>) -> Option<usize> {
    let want = hz.map(|h| (h * 1000.0) as i64);
    refresh_idxs(modes, (w, h)).into_iter().min_by_key(|&i| match want {
        Some(t) => (modes[i].mhz as i64 - t).abs(),
        None => -(modes[i].mhz as i64), // no preference: highest refresh
    })
}

// ───────────────────────────── niri config (read) ─────────────────────────────

/// Finds the `output "name" { ... }` block, returns (first line, last line).
fn find_block(lines: &[&str], name: &str) -> Option<(usize, usize)> {
    let needle = format!("\"{name}\"");
    let start = lines.iter().position(|l| {
        let t = l.trim_start();
        t.starts_with("output ") && t.contains(&needle) && t.contains('{')
    })?;

    let mut depth = 0i32;
    for (i, l) in lines.iter().enumerate().skip(start) {
        let code = l.split("//").next().unwrap_or("");
        for c in code.chars() {
            match c {
                '{' => depth += 1,
                '}' => depth -= 1,
                _ => {}
            }
        }
        if depth <= 0 {
            return Some((start, i));
        }
    }
    None
}

#[derive(Default)]
struct CfgOut {
    mode: Option<(u32, u32, Option<f64>)>,
    scale: Option<f64>,
    rot: Option<usize>,
    pos: Option<(i32, i32)>,
}

/// Reads saved values for an output. Only used for outputs that are currently off,
/// because niri doesn't report their mode/scale/position over IPC.
fn read_cfg(name: &str) -> CfgOut {
    let mut out = CfgOut::default();
    let Ok(src) = fs::read_to_string(CFGPATH) else { return out };
    let lines: Vec<&str> = src.lines().collect();
    let Some((s, e)) = find_block(&lines, name) else { return out };

    for l in &lines[s..=e] {
        let t = l.trim();
        let quoted = t.split('"').nth(1).unwrap_or("");
        if t.starts_with("mode ") {
            let (dims, hz) = match quoted.split_once('@') {
                Some((d, r)) => (d, r.parse::<f64>().ok()),
                None => (quoted, None),
            };
            if let Some((w, h)) = dims.split_once('x') {
                if let (Ok(w), Ok(h)) = (w.parse(), h.parse()) {
                    out.mode = Some((w, h, hz));
                }
            }
        } else if t.starts_with("scale ") {
            out.scale = t["scale ".len()..].trim().parse().ok();
        } else if t.starts_with("transform ") {
            out.rot = TRANSFORM_KDL.iter().position(|k| *k == quoted);
        } else if t.starts_with("position ") {
            let (mut x, mut y) = (None, None);
            for tok in t.split_whitespace() {
                if let Some(v) = tok.strip_prefix("x=") {
                    x = v.parse().ok();
                }
                if let Some(v) = tok.strip_prefix("y=") {
                    y = v.parse().ok();
                }
            }
            if let (Some(x), Some(y)) = (x, y) {
                out.pos = Some((x, y));
            }
        }
    }
    out
}

// ───────────────────────────── niri config (write) ─────────────────────────────

fn render_block(st: &MonState) -> String {
    let m = st.cur();
    let mut s = format!("output \"{}\" {{\n", st.name);
    if !st.enabled {
        s.push_str("    off\n");
    }
    s.push_str(&format!("    mode \"{}x{}@{:.3}\"\n", m.w, m.h, m.mhz as f64 / 1000.0));
    s.push_str(&format!("    scale {}\n", fmt_trim(st.scale, 2)));
    s.push_str(&format!("    transform \"{}\"\n", TRANSFORM_KDL[st.rot]));
    s.push_str(&format!("    position x={} y={}\n}}", st.x, st.y));
    s
}

fn upsert_block(src: &str, name: &str, block: &str) -> String {
    let lines: Vec<&str> = src.lines().collect();
    match find_block(&lines, name) {
        Some((s, e)) => {
            let mut out: Vec<&str> = lines[..s].to_vec();
            out.extend(block.lines());
            out.extend_from_slice(&lines[e + 1..]);
            out.join("\n") + "\n"
        }
        None => {
            let mut out = src.trim_end().to_string();
            if !out.is_empty() {
                out.push_str("\n\n");
            }
            out.push_str(block);
            out.push('\n');
            out
        }
    }
}

fn write_outputs(states: &[MonState]) -> std::io::Result<()> {
    let mut src = fs::read_to_string(CFGPATH).unwrap_or_default();
    for st in states {
        src = upsert_block(&src, &st.name, &render_block(st));
    }
    if let Some(parent) = Path::new(CFGPATH).parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(CFGPATH, src)
}

// ───────────────────────────── niri ipc (read) ─────────────────────────────

fn rot_from(t: Transform) -> usize {
    match t {
        Transform::_90 => 1,
        Transform::_180 => 2,
        Transform::_270 => 3,
        _ => 0, // flipped variants aren't supported by this page
    }
}

fn load_monitors() -> Vec<MonState> {
    let outputs = match Socket::connect().and_then(|mut s| s.send(Request::Outputs)) {
        Ok(Ok(Response::Outputs(o))) => o,
        other => {
            eprintln!("[calibrate] cannot read outputs from niri: {other:?}");
            return vec![];
        }
    };

    let mut found: Vec<(MonState, bool)> = Vec::new(); // (state, has a known position)

    for (name, o) in outputs {
        if o.modes.is_empty() {
            continue;
        }
        let modes: Vec<ModeOpt> = o
            .modes
            .iter()
            .map(|m| ModeOpt { w: m.width as u32, h: m.height as u32, mhz: m.refresh_rate })
            .collect();

        let label = [o.make.trim(), o.model.trim()]
            .iter()
            .filter(|s| !s.is_empty())
            .cloned()
            .collect::<Vec<_>>()
            .join(" ");

        let (mode, enabled, scale, rot, pos) = match (o.current_mode, &o.logical) {
            (Some(cm), Some(lg)) => (
                cm.min(modes.len() - 1),
                true,
                lg.scale,
                rot_from(lg.transform),
                Some((lg.x, lg.y)),
            ),
            _ => {
                let cfg = read_cfg(&name);
                let preferred = o.modes.iter().position(|m| m.is_preferred).unwrap_or(0);
                let mode = cfg
                    .mode
                    .and_then(|(w, h, hz)| pick_mode(&modes, w, h, hz))
                    .unwrap_or(preferred);
                (mode, false, cfg.scale.unwrap_or(1.0), cfg.rot.unwrap_or(0), cfg.pos)
            }
        };

        let (x, y) = pos.unwrap_or((0, 0));
        found.push((
            MonState { name, label, modes, mode, enabled, scale, rot, x, y },
            pos.is_some(),
        ));
    }

    found.sort_by(|a, b| a.0.name.cmp(&b.0.name));

    // Off outputs with no saved position go to the right of everything else.
    let mut right = found
        .iter()
        .filter(|(_, placed)| *placed)
        .map(|(m, _)| m.x + m.logical().0 as i32)
        .max()
        .unwrap_or(0);
    for (m, placed) in found.iter_mut() {
        if !*placed {
            m.x = right;
            m.y = 0;
            right += m.logical().0 as i32;
        }
    }

    found.into_iter().map(|(m, _)| m).collect()
}

// ───────────────────────────── canvas geometry ─────────────────────────────

struct Mon {
    st: MonState,
    widget: GtkBox,
    info: Label,
}

struct Ui {
    mons: Vec<Mon>,
    sel: usize,
    ox: f64,
    oy: f64,
    scale_opts: Vec<f64>,
}

fn canvas_rect(ui: &Ui, st: &MonState) -> (f64, f64, f64, f64) {
    let (lw, lh) = st.logical();
    (
        ui.ox + st.x as f64 * PX,
        ui.oy + st.y as f64 * PX,
        (lw * PX).max(60.0),
        (lh * PX).max(40.0),
    )
}

fn others_of(ui: &Ui, idx: usize) -> Vec<(f64, f64, f64, f64)> {
    ui.mons
        .iter()
        .enumerate()
        .filter(|(i, m)| *i != idx && m.st.enabled)
        .map(|(_, m)| {
            let (w, h) = m.st.logical();
            (m.st.x as f64, m.st.y as f64, w, h)
        })
        .collect()
}

/// Snaps a rectangle to the edges of the others, then pushes it out of any overlap.
fn arrange(
    others: &[(f64, f64, f64, f64)],
    w: f64,
    h: f64,
    mut x: f64,
    mut y: f64,
    snap: bool,
) -> (f64, f64) {
    if snap {
        let (mut bx, mut by): (Option<f64>, Option<f64>) = (None, None);
        for &(ox, oy, ow, oh) in others {
            for cx in [ox + ow, ox - w, ox, ox + ow - w] {
                let d = (cx - x).abs();
                if d <= SNAP && bx.map_or(true, |b| d < (b - x).abs()) {
                    bx = Some(cx);
                }
            }
            for cy in [oy + oh, oy - h, oy, oy + oh - h] {
                let d = (cy - y).abs();
                if d <= SNAP && by.map_or(true, |b| d < (b - y).abs()) {
                    by = Some(cy);
                }
            }
        }
        if let Some(v) = bx {
            x = v;
        }
        if let Some(v) = by {
            y = v;
        }
    }

    for &(ox, oy, ow, oh) in others {
        if x < ox + ow && x + w > ox && y < oy + oh && y + h > oy {
            let l = x + w - ox;
            let r = ox + ow - x;
            let u = y + h - oy;
            let d = oy + oh - y;
            let m = l.min(r).min(u).min(d);
            if m == l {
                x -= l;
            } else if m == r {
                x += r;
            } else if m == u {
                y -= u;
            } else {
                y += d;
            }
        }
    }
    (x.round(), y.round())
}

fn build_mon_widget() -> (GtkBox, Label) {
    let b = GtkBox::new(Orientation::Vertical, 2);
    b.add_css_class("canvas-mon");
    b.set_overflow(gtk4::Overflow::Hidden);
    b.set_cursor_from_name(Some("grab"));

    let name = Label::new(None);
    name.add_css_class("canvas-mon-name");
    name.set_ellipsize(pango::EllipsizeMode::End);
    name.set_vexpand(true);
    name.set_valign(Align::End);

    let info = Label::new(None);
    info.add_css_class("canvas-mon-info");
    info.set_ellipsize(pango::EllipsizeMode::End);
    info.set_vexpand(true);
    info.set_valign(Align::Start);

    b.append(&name);
    b.append(&info);
    (b, info)
}

fn load_css() {
    let css = CssProvider::new();
    css.load_from_data(
        r#"
        .monitor-canvas {
            background-color: #0000005b;
            border: 1px solid #ffffff39;
            border-radius: 20px;
        }
        .canvas-mon {
            background: linear-gradient(#fff2, #0001), #2b2f3a;
            border: 2px solid #ffffff33;
            border-radius: 10px;
            color: #eceff4;
            box-shadow: 0 6px 12px #0000004d;
            transition: border-color 150ms ease-in-out, opacity 150ms ease-in-out;
        }
        .canvas-mon-selected {
            border-color: #d8ff7c;
            box-shadow: 0 0 0 3px #d8ff7c40, 0 6px 12px #0000004d;
        }
        .canvas-mon-off { opacity: 0.45; }
        .canvas-mon-name { font-weight: 700; }
        .setting-list {
            background-color: #ffffff00;
            border: 1px solid #ffffff39;
            border-radius: 20px;
        }
        .setting-row { padding: 16px 20px; }
        .setting-border-left { border-left: 1px solid #ffffff22; }
        .setting-border-top { border-top: 1px solid #ffffff22; }
        .setting-title { font-weight: 600; font-size: 14px; color: #f2f2f5; }
        .canvas-mon-info { font-size: 11px; color: #ffffff99; }
        "#,
    );
    gtk4::style_context_add_provider_for_display(
        &gdk::Display::default().expect("Could not connect to display"),
        &css,
        gtk4::STYLE_PROVIDER_PRIORITY_APPLICATION,
    );
}

fn build_setting_row(title_text: &str, subtitle_text: &str) -> (GtkBox, GtkBox) {
    let row = GtkBox::new(Orientation::Horizontal, 20);
    row.set_hexpand(true);
    row.add_css_class("setting-row");

    let text = GtkBox::new(Orientation::Vertical, 2);
    text.set_hexpand(true);
    text.set_halign(Align::Start);
    text.set_valign(Align::Center);

    let title = Label::new(Some(title_text));
    title.add_css_class("setting-title");
    title.set_halign(Align::Start);

    let subtitle = Label::new(Some(subtitle_text));
    subtitle.add_css_class("frame-subtitle");
    subtitle.set_halign(Align::Start);
    subtitle.set_wrap(true);
    subtitle.set_xalign(0.0);
    subtitle.set_max_width_chars(34);

    text.append(&title);
    text.append(&subtitle);

    let control_box = GtkBox::new(Orientation::Horizontal, 0);
    control_box.set_halign(Align::End);
    control_box.set_valign(Align::Center);

    row.append(&text);
    row.append(&control_box);
    (row, control_box)
}

fn set_strings(dd: &DropDown, items: &[String], sel: usize) {
    let refs: Vec<&str> = items.iter().map(String::as_str).collect();
    dd.set_model(Some(&StringList::new(&refs)));
    dd.set_selected(sel as u32);
}

// ───────────────────────────── page controller ─────────────────────────────

#[derive(Clone)]
struct Controls {
    title: Label,
    enabled: Switch,
    res: DropDown,
    refresh: DropDown,
    scale: DropDown,
    rot: DropDown,
    rows: GtkBox, // everything that only makes sense while the display is on
}

#[derive(Clone)]
struct Page {
    ui: Rc<RefCell<Ui>>,
    fixed: Fixed,
    ctl: Controls,
    busy: Rc<Cell<bool>>, // true while we change widgets ourselves, so handlers ignore it
    toast: Toast,
}

impl Page {
    fn place(&self, idx: usize) {
        let ui = self.ui.borrow();
        let m = &ui.mons[idx];
        let (px, py, w, h) = canvas_rect(&ui, &m.st);
        m.widget.set_size_request(w as i32, h as i32);
        self.fixed.move_(&m.widget, px, py);

        let c = m.st.cur();
        if m.st.enabled {
            m.widget.remove_css_class("canvas-mon-off");
            m.info.set_text(&format!("{}×{}", c.w, c.h));
        } else {
            m.widget.add_css_class("canvas-mon-off");
            m.info.set_text("Off");
        }
        if idx == ui.sel {
            m.widget.add_css_class("canvas-mon-selected");
        } else {
            m.widget.remove_css_class("canvas-mon-selected");
        }
    }

    fn place_all(&self) {
        let n = self.ui.borrow().mons.len();
        for i in 0..n {
            self.place(i);
        }
    }

    /// Push a monitor out of any overlap (after its size changed or it was turned on).
    fn reflow(&self, idx: usize) {
        {
            let mut ui = self.ui.borrow_mut();
            if ui.mons[idx].st.enabled {
                let (w, h) = ui.mons[idx].st.logical();
                let others = others_of(&ui, idx);
                let (x, y) = arrange(
                    &others,
                    w,
                    h,
                    ui.mons[idx].st.x as f64,
                    ui.mons[idx].st.y as f64,
                    false,
                );
                ui.mons[idx].st.x = x as i32;
                ui.mons[idx].st.y = y as i32;
            }
        }
        self.place(idx);
    }

    fn select(&self, idx: usize) {
        self.ui.borrow_mut().sel = idx;
        self.place_all();
        self.sync_controls();
    }

    fn fill_refresh(&self, st: &MonState) {
        let cur = st.cur();
        let idxs = refresh_idxs(&st.modes, (cur.w, cur.h));
        let strs: Vec<String> =
            idxs.iter().map(|&i| format!("{} Hz", fmt_hz(st.modes[i].mhz))).collect();
        let sel = idxs.iter().position(|&i| i == st.mode).unwrap_or(0);
        set_strings(&self.ctl.refresh, &strs, sel);
    }

    /// Loads the selected monitor's values into the settings controls.
    fn sync_controls(&self) {
        self.busy.set(true);
        let st = {
            let ui = self.ui.borrow();
            ui.mons[ui.sel].st.clone()
        };
        let c = &self.ctl;
        let cur = st.cur();

        let title = if st.label.is_empty() {
            st.name.clone()
        } else {
            format!("{} — {}", st.name, st.label)
        };
        c.title.set_text(&title);
        c.enabled.set_active(st.enabled);
        c.rows.set_sensitive(st.enabled);

        let res = resolutions(&st.modes);
        let strs: Vec<String> = res.iter().map(|(w, h)| format!("{w} × {h}")).collect();
        let sel = res.iter().position(|r| *r == (cur.w, cur.h)).unwrap_or(0);
        set_strings(&c.res, &strs, sel);

        self.fill_refresh(&st);

        let mut opts: Vec<f64> = SCALES.to_vec();
        if !opts.iter().any(|s| (s - st.scale).abs() < 0.005) {
            opts.push(st.scale);
            opts.sort_by(|a, b| a.partial_cmp(b).unwrap());
        }
        let sel = opts.iter().position(|s| (s - st.scale).abs() < 0.005).unwrap_or(0);
        let strs: Vec<String> =
            opts.iter().map(|s| format!("{}%", (s * 100.0).round() as i32)).collect();
        set_strings(&c.scale, &strs, sel);
        self.ui.borrow_mut().scale_opts = opts;

        c.rot.set_selected(st.rot as u32);
        self.busy.set(false);
    }

    fn wire(&self, canvas: &Overlay) {
        // ── on / off ──
        let p = self.clone();
        self.ctl.enabled.connect_active_notify(move |sw| {
            if p.busy.get() {
                return;
            }
            let on = sw.is_active();
            let idx = {
                let mut ui = p.ui.borrow_mut();
                let enabled_count = ui.mons.iter().filter(|m| m.st.enabled).count();
                if !on && enabled_count <= 1 {
                    drop(ui);
                    p.busy.set(true);
                    sw.set_active(true);
                    p.busy.set(false);
                    p.toast.show("At least one display has to stay on");
                    return;
                }
                let i = ui.sel;
                ui.mons[i].st.enabled = on;
                i
            };
            p.ctl.rows.set_sensitive(on);
            p.reflow(idx);
        });

        // ── resolution ──
        let p = self.clone();
        self.ctl.res.connect_selected_notify(move |dd| {
            if p.busy.get() {
                return;
            }
            let sel = dd.selected() as usize;
            let (idx, st) = {
                let mut ui = p.ui.borrow_mut();
                let i = ui.sel;
                let m = &mut ui.mons[i].st;
                let res = resolutions(&m.modes);
                let Some(&(w, h)) = res.get(sel) else { return };
                let cur_hz = m.cur().mhz as f64 / 1000.0;
                if let Some(best) = pick_mode(&m.modes, w, h, Some(cur_hz)) {
                    m.mode = best;
                }
                (i, m.clone())
            };
            p.busy.set(true);
            p.fill_refresh(&st);
            p.busy.set(false);
            p.reflow(idx);
        });

        // ── refresh rate ──
        let p = self.clone();
        self.ctl.refresh.connect_selected_notify(move |dd| {
            if p.busy.get() {
                return;
            }
            let sel = dd.selected() as usize;
            let idx = {
                let mut ui = p.ui.borrow_mut();
                let i = ui.sel;
                let m = &mut ui.mons[i].st;
                let cur = m.cur();
                if let Some(&k) = refresh_idxs(&m.modes, (cur.w, cur.h)).get(sel) {
                    m.mode = k;
                }
                i
            };
            p.place(idx);
        });

        // ── scale ──
        let p = self.clone();
        self.ctl.scale.connect_selected_notify(move |dd| {
            if p.busy.get() {
                return;
            }
            let sel = dd.selected() as usize;
            let idx = {
                let mut ui = p.ui.borrow_mut();
                let i = ui.sel;
                let Some(v) = ui.scale_opts.get(sel).copied() else { return };
                ui.mons[i].st.scale = v;
                i
            };
            p.reflow(idx);
        });

        // ── rotation ──
        let p = self.clone();
        self.ctl.rot.connect_selected_notify(move |dd| {
            if p.busy.get() {
                return;
            }
            let sel = (dd.selected() as usize).min(ROTATIONS.len() - 1);
            let idx = {
                let mut ui = p.ui.borrow_mut();
                let i = ui.sel;
                ui.mons[i].st.rot = sel;
                i
            };
            p.reflow(idx);
        });

        // ── drag & select on the canvas ──
        // The gesture lives on the (stationary) canvas and hit-tests monitors itself,
        // so the pointer offset doesn't jitter while a monitor moves under it.
        let drag = GestureDrag::new();
        drag.set_button(gdk::BUTTON_PRIMARY);
        let grab: Rc<Cell<Option<(usize, i32, i32)>>> = Rc::new(Cell::new(None));

        {
            let p = self.clone();
            let grab = grab.clone();
            drag.connect_drag_begin(move |_, sx, sy| {
                let hit = {
                    let ui = p.ui.borrow();
                    ui.mons
                        .iter()
                        .enumerate()
                        .rev()
                        .find(|(_, m)| {
                            let (px, py, w, h) = canvas_rect(&ui, &m.st);
                            sx >= px && sx <= px + w && sy >= py && sy <= py + h
                        })
                        .map(|(i, m)| (i, m.st.x, m.st.y))
                };
                grab.set(hit);
                if let Some((i, _, _)) = hit {
                    p.select(i);
                }
            });
        }
        {
            let p = self.clone();
            let grab = grab.clone();
            drag.connect_drag_update(move |_, dx, dy| {
                let Some((i, x0, y0)) = grab.get() else { return };
                {
                    let mut ui = p.ui.borrow_mut();
                    let (w, h) = ui.mons[i].st.logical();
                    let others =
                        if ui.mons[i].st.enabled { others_of(&ui, i) } else { Vec::new() };

                    let min_x = -ui.ox / PX;
                    let max_x = ((CANVAS_W as f64 - ui.ox) / PX - w).max(min_x);
                    let min_y = -ui.oy / PX;
                    let max_y = ((CANVAS_H as f64 - ui.oy) / PX - h).max(min_y);

                    let cx = (x0 as f64 + dx / PX).clamp(min_x, max_x);
                    let cy = (y0 as f64 + dy / PX).clamp(min_y, max_y);
                    let (x, y) = arrange(&others, w, h, cx, cy, true);
                    ui.mons[i].st.x = x as i32;
                    ui.mons[i].st.y = y as i32;
                }
                p.place(i);
            });
        }
        canvas.add_controller(drag);
    }
}

// ───────────────────────────── page ─────────────────────────────

pub fn build_display_page(
) -> Overlay {
    load_css();

    let content = GtkBox::new(Orientation::Vertical, 16);

    let card = GtkBox::new(Orientation::Vertical, 10);
    card.add_css_class("shortcut-card");
    card.add_css_class("detail-card");
    card.set_margin_top(10);
    card.set_hexpand(true);

    let infoicon = gtk4::Image::from_file("/var/lib/cynager/icons/mon.svg");
    infoicon.set_pixel_size(54);
    infoicon.set_halign(Align::Center);
    infoicon.set_css_classes(&["card-icons"]);

    let title = Label::new(Some("Display and Monitor Settings"));
    title.add_css_class("shortcut-title");
    title.set_halign(Align::Center);

    let subtitle = Label::new(Some("Change the settings for your monitors."));
    subtitle.add_css_class("shortcut-desc");
    subtitle.set_halign(Align::Center);

    card.append(&infoicon);
    card.append(&title);
    card.append(&subtitle);
    content.append(&card);

    let toast = Toast::new();

    // ── monitors frame (canvas) ──
    let monframe = Frame::new(None);
    let monbox = GtkBox::new(Orientation::Vertical, 10);

    let montitle = Label::new(Some("Monitors"));
    montitle.add_css_class("frame-title");
    montitle.set_halign(Align::Start);
    montitle.set_margin_start(20);
    montitle.set_margin_top(20);
    montitle.set_margin_end(20);

    let monsub = Label::new(Some("Drag to arrange your displays, click one to change its settings."));
    monsub.add_css_class("frame-subtitle");
    monsub.set_halign(Align::Start);
    monsub.set_margin_start(20);
    monsub.set_margin_end(20);

    monbox.append(&montitle);
    monbox.append(&monsub);
    monframe.set_child(Some(&monbox));
    content.append(&monframe);

    let states = load_monitors();

    if states.is_empty() {
        let empty = Label::new(Some("No displays found. Is niri running?"));
        empty.add_css_class("frame-subtitle");
        empty.set_halign(Align::Start);
        empty.set_margin_start(20);
        empty.set_margin_bottom(20);
        monbox.append(&empty);
    } else {
        // centre the whole arrangement in the canvas
        let (mut minx, mut miny, mut maxx, mut maxy) = (f64::MAX, f64::MAX, f64::MIN, f64::MIN);
        for s in &states {
            let (w, h) = s.logical();
            minx = minx.min(s.x as f64);
            miny = miny.min(s.y as f64);
            maxx = maxx.max(s.x as f64 + w);
            maxy = maxy.max(s.y as f64 + h);
        }
        let ox = CANVAS_W as f64 / 2.0 - (minx + maxx) / 2.0 * PX;
        let oy = CANVAS_H as f64 / 2.0 - (miny + maxy) / 2.0 * PX;

        let fixed = Fixed::new();
        let mons: Vec<Mon> = states
            .into_iter()
            .map(|st| {
                let (widget, info) = build_mon_widget();
                // the name label is the first child
                if let Some(first) = widget.first_child().and_downcast::<Label>() {
                    first.set_text(&st.name);
                }
                widget.set_tooltip_text(Some(&st.label));
                fixed.put(&widget, 0.0, 0.0);
                Mon { st, widget, info }
            })
            .collect();

        // dotted grid behind the monitors
        let grid = DrawingArea::new();
        grid.set_content_width(CANVAS_W);
        grid.set_content_height(CANVAS_H);
        grid.set_draw_func(|_, cr, w, h| {
            cr.set_source_rgba(1.0, 1.0, 1.0, 0.10);
            let mut x = 12.0;
            while x < w as f64 {
                let mut y = 12.0;
                while y < h as f64 {
                    cr.rectangle(x, y, 1.5, 1.5);
                    y += 24.0;
                }
                x += 24.0;
            }
            let _ = cr.fill();
        });

        let canvas = Overlay::new();
        canvas.set_child(Some(&grid));
        canvas.add_overlay(&fixed);

        let canvas_scroller = ScrolledWindow::builder()
            .hscrollbar_policy(gtk4::PolicyType::Automatic)
            .vscrollbar_policy(gtk4::PolicyType::Never)
            .hexpand(true)
            .height_request(CANVAS_H)
            .margin_start(20)
            .margin_end(20)
            .margin_bottom(20)
            .child(&canvas)
            .build();
        canvas_scroller.add_css_class("monitor-canvas");
        canvas_scroller.set_overflow(gtk4::Overflow::Hidden);
        monbox.append(&canvas_scroller);

        // ── settings frame ──

        let settitle = Label::new(None);
        settitle.add_css_class("frame-title");
        settitle.set_halign(Align::Start);
        settitle.set_valign(Align::Center);
        settitle.set_hexpand(true);
        settitle.set_ellipsize(pango::EllipsizeMode::End);

        let enabled = Switch::new();
        enabled.set_valign(Align::Center);
        enabled.set_tooltip_text(Some("Turn this display on or off"));

        let headrow = GtkBox::new(Orientation::Horizontal, 12);
        headrow.set_margin_start(20);
        headrow.set_margin_end(20);
        headrow.append(&settitle);
        headrow.append(&enabled);

        let mk_dd = || {
            let dd = DropDown::from_strings(&["-"]);
            dd.set_valign(Align::Center);
            dd.remove_css_class("background");
            dd
        };
        let res = mk_dd();
        let refresh = mk_dd();
        let scale = mk_dd();
        let rot = mk_dd();
        let rot_strs: Vec<String> = ROTATIONS.iter().map(|s| s.to_string()).collect();
        set_strings(&rot, &rot_strs, 0);

        // 2x2 grid inside one rounded outline, cells split by thin lines
        let rows = GtkBox::new(Orientation::Vertical, 0);
        rows.add_css_class("setting-list");
        rows.set_overflow(gtk4::Overflow::Hidden);
        rows.set_margin_start(20);
        rows.set_margin_end(20);

        let grid = Grid::new();
        grid.set_column_homogeneous(true);
        grid.set_hexpand(true);

        for (i, (name, desc, dd)) in [
            ("Resolution", "How many pixels the screen shows. Higher is sharper, but things look smaller.", &res),
            ("Refresh rate", "How many times per second the screen updates. Higher feels smoother.", &refresh),
            ("Scale", "Makes text and windows bigger or smaller without changing the resolution.", &scale),
            ("Rotation", "Turns the picture for a monitor that is mounted sideways or upside down.", &rot),
        ]
        .into_iter()
        .enumerate()
        {
            let (col, row_i) = ((i % 2) as i32, (i / 2) as i32);
            let (cell, ctl) = build_setting_row(name, desc);
            ctl.append(dd);
            if col == 1 {
                cell.add_css_class("setting-border-left");
            }
            if row_i == 1 {
                cell.add_css_class("setting-border-top");
            }
            grid.attach(&cell, col, row_i, 1, 1);
        }
        rows.append(&grid);

        let apply = Button::new();
        apply.set_child(Some(&Label::new(Some("Apply"))));
        apply.add_css_class("sub-btn");
        apply.set_halign(Align::End);
        apply.set_margin_end(20);
        apply.set_margin_bottom(20);
        apply.set_cursor_from_name(Some("pointer"));

        monbox.append(&headrow);
        monbox.append(&rows);
        monbox.append(&apply);

        let page = Page {
            ui: Rc::new(RefCell::new(Ui { mons, sel: 0, ox, oy, scale_opts: Vec::new() })),
            fixed,
            ctl: Controls { title: settitle, enabled, res, refresh, scale, rot, rows },
            busy: Rc::new(Cell::new(false)),
            toast: toast.clone(),
        };
        page.wire(&canvas);
        page.select(0);

        {
            let page = page.clone();
            apply.connect_clicked(move |_| {
                let states: Vec<MonState> =
                    page.ui.borrow().mons.iter().map(|m| m.st.clone()).collect();
                match write_outputs(&states) {
                    Ok(()) => page.toast.show("Display settings saved"),
                    Err(e) => {
                        eprintln!("[calibrate] failed to write niri config: {e}");
                        page.toast.show("Couldn't save display settings");
                    }
                }
            });
        }
    }

    let overlay = Overlay::new();
    let scroller: ScrolledWindow = page_scroller(&content);
    overlay.set_child(Some(&scroller));
    overlay.add_overlay(&toast.revealer);
    overlay
}