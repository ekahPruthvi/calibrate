use gtk4::prelude::*;
use gtk4::{
    Align, Box as GtkBox, Button, DrawingArea, EventControllerKey, Frame, GestureDrag, Image,
    Label, Orientation, Overlay, Revealer, RevealerTransitionType, ScrolledWindow, gdk, gio,
    glib,
};
use std::cell::{Cell, RefCell};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::rc::Rc;
use std::time::Duration;

use crate::home::page_scroller;

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
        label.set_wrap(true);
        label.set_max_width_chars(60);

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
        glib::timeout_add_local_once(Duration::from_millis(3500), move || {
            if *this.generation.borrow() == id {
                this.revealer.set_reveal_child(false);
            }
        });
    }
}

// ───────────────────────────── helpers ─────────────────────────────

fn caption(text: &str) -> Label {
    let l = Label::new(Some(text));
    l.add_css_class("row-caption");
    l.set_halign(Align::Start);
    l.set_xalign(0.0);
    l.set_wrap(true);
    l
}

// ───────────────────────────── battery percentage & health ─────────────────────────────

/// the gauge is a 270° horseshoe opening at the bottom, split into 5 segments
const GAUGE_START_DEG: f64 = 135.0;
const GAUGE_SWEEP_DEG: f64 = 270.0;
const GAUGE_SEGMENTS: usize = 5;

/// seconds between refreshes while the page is open
const REFRESH_SECS: u32 = 5;

struct BatteryInfo {
    /// current charge, 0..=100
    charge: f64,
    charging: bool,
    /// full-charge capacity as a share of design capacity, 0..=100
    health: Option<f64>,
    full_wh: Option<f64>,
    design_wh: Option<f64>,
    cycles: Option<f64>,
}

fn read_num(dir: &Path, file: &str) -> Option<f64> {
    fs::read_to_string(dir.join(file)).ok()?.trim().parse::<f64>().ok()
}

fn find_battery_dir() -> Option<PathBuf> {
    let mut dirs: Vec<PathBuf> = fs::read_dir("/sys/class/power_supply")
        .ok()?
        .flatten()
        .map(|e| e.path())
        .filter(|p| {
            let is_battery = fs::read_to_string(p.join("type"))
                .map(|t| t.trim() == "Battery")
                .unwrap_or(false);
            // skip batteries of peripherals (mice, keyboards, ...)
            let is_device = fs::read_to_string(p.join("scope"))
                .map(|s| s.trim() == "Device")
                .unwrap_or(false);
            is_battery && !is_device
        })
        .collect();
    dirs.sort();
    dirs.into_iter().next()
}

/// Energy in Wh. Uses energy_* when the driver has it, otherwise charge_* (µAh)
/// times the nominal voltage.
fn energy_wh(dir: &Path, kind: &str) -> Option<f64> {
    if let Some(e) = read_num(dir, &format!("energy_{kind}")) {
        return Some(e / 1e6);
    }
    let charge = read_num(dir, &format!("charge_{kind}"))? / 1e6;
    let volts = read_num(dir, "voltage_min_design").or_else(|| read_num(dir, "voltage_now"))? / 1e6;
    Some(charge * volts)
}

fn load_battery() -> Result<BatteryInfo, &'static str> {
    let dir = find_battery_dir().ok_or("No battery detected")?;

    let full = energy_wh(&dir, "full");
    let design = energy_wh(&dir, "full_design");

    let charge = read_num(&dir, "capacity")
        .or_else(|| match (energy_wh(&dir, "now"), full) {
            (Some(n), Some(f)) if f > 0.0 => Some(n / f * 100.0),
            _ => None,
        })
        .ok_or("This battery doesn't report its charge")?;

    let health = match (full, design) {
        (Some(f), Some(d)) if d > 0.0 => Some((f / d * 100.0).clamp(0.0, 100.0)),
        _ => None,
    };

    let charging = fs::read_to_string(dir.join("status"))
        .map(|s| s.trim().eq_ignore_ascii_case("charging"))
        .unwrap_or(false);

    Ok(BatteryInfo {
        charge: charge.clamp(0.0, 100.0),
        charging,
        health,
        full_wh: full,
        design_wh: design,
        cycles: read_num(&dir, "cycle_count"),
    })
}

/// wording and colour for a health percentage
fn health_word(p: f64) -> (&'static str, (f64, f64, f64)) {
    if p >= 90.0 {
        ("Excellent", (0.30, 0.78, 0.47))
    } else if p >= 80.0 {
        ("Good", (0.30, 0.78, 0.47))
    } else if p >= 60.0 {
        ("Fair", (0.95, 0.70, 0.25))
    } else if p >= 40.0 {
        ("Poor", (0.90, 0.30, 0.30))
    } else {
        ("Very poor", (0.90, 0.30, 0.30))
    }
}

/// colour of the gauge, driven by the current charge
fn charge_color(pct: f64, charging: bool) -> (f64, f64, f64) {
    if charging {
        (0.30, 0.78, 0.47)
    } else if pct <= 15.0 {
        (0.90, 0.30, 0.30)
    } else if pct <= 30.0 {
        (0.95, 0.70, 0.25)
    } else {
        (0.30, 0.78, 0.47)
    }
}

fn hex((r, g, b): (f64, f64, f64)) -> String {
    format!("#{:02x}{:02x}{:02x}", (r * 255.0) as u8, (g * 255.0) as u8, (b * 255.0) as u8)
}

fn build_charge_gauge(value: Rc<Cell<Option<f64>>>, charging: Rc<Cell<bool>>) -> DrawingArea {
    let area = DrawingArea::new();
    area.set_content_width(300);
    area.set_content_height(240);
    area.set_halign(Align::Center);

    area.set_draw_func(move |area, cr, w, h| {
        #[allow(deprecated)]
        let c = area.style_context().color();
        let (tr, tg, tb) = (c.red() as f64, c.green() as f64, c.blue() as f64);

        let (w, h) = (w as f64, h as f64);
        let line = 22.0;
        let (cx, cy) = (w / 2.0, h * 0.54);
        let r = (w / 2.0).min(h * 0.46) - line / 2.0 - 2.0;

        let deg = std::f64::consts::PI / 180.0;
        let start = GAUGE_START_DEG * deg;
        let sweep = GAUGE_SWEEP_DEG * deg;
        let seg = sweep / GAUGE_SEGMENTS as f64;
        let cap = (line / 2.0) / r; // angle taken up by a round cap
        let gap = 4.0 * deg; // visible gap between two segments

        let pct = value.get();
        let (fr, fg, fb) = pct
            .map(|p| charge_color(p, charging.get()))
            .unwrap_or((0.5, 0.5, 0.5));

        // soft glow behind the dial
        if pct.is_some() {
            let glow = gtk4::cairo::RadialGradient::new(cx, cy, 0.0, cx, cy, r * 1.15);
            glow.add_color_stop_rgba(0.0, fr, fg, fb, 0.20);
            glow.add_color_stop_rgba(1.0, fr, fg, fb, 0.0);
            cr.arc(cx, cy, r * 1.15, 0.0, std::f64::consts::TAU);
            let _ = cr.set_source(&glow);
            let _ = cr.fill();
        }

        cr.set_line_width(line);
        cr.set_line_cap(gtk4::cairo::LineCap::Round);

        let value_angle = pct.map(|p| start + sweep * (p / 100.0));

        for i in 0..GAUGE_SEGMENTS {
            let a0 = start + seg * i as f64 + gap / 2.0 + cap;
            let a1 = start + seg * (i + 1) as f64 - gap / 2.0 - cap;

            // empty track
            cr.set_source_rgba(tr, tg, tb, 0.14);
            cr.new_sub_path();
            cr.arc(cx, cy, r, a0, a1);
            let _ = cr.stroke();

            // filled part of this segment
            if let Some(v) = value_angle {
                let end = a1.min(v);
                if end > a0 {
                    cr.set_source_rgb(fr, fg, fb);
                    cr.new_sub_path();
                    cr.arc(cx, cy, r, a0, end);
                    let _ = cr.stroke();
                }
            }
        }

        // knob at the current value
        if let Some(v) = value_angle {
            let v = v.clamp(start + gap / 2.0 + cap, start + sweep - gap / 2.0 - cap);
            let (kx, ky) = (cx + r * v.cos(), cy + r * v.sin());
            cr.new_sub_path();
            cr.arc(kx, ky, line * 0.62, 0.0, std::f64::consts::TAU);
            cr.set_source_rgb(fr, fg, fb);
            let _ = cr.fill_preserve();
            cr.set_source_rgba(1.0, 1.0, 1.0, 0.85);
            cr.set_line_width(2.5);
            let _ = cr.stroke();
        }
    });

    area
}

fn build_health_card() -> Frame {
    let frame = Frame::new(None);
    let outer = GtkBox::new(Orientation::Vertical, 8);
    outer.set_margin_top(20);
    outer.set_margin_bottom(20);
    outer.set_margin_start(20);
    outer.set_margin_end(20);

    let value: Rc<Cell<Option<f64>>> = Rc::new(Cell::new(None));
    let charging: Rc<Cell<bool>> = Rc::new(Cell::new(false));
    let gauge = build_charge_gauge(value.clone(), charging.clone());

    // charge percentage + "Fair Health (70%)" sit inside the dial
    let pct_lbl = Label::new(None);
    pct_lbl.set_halign(Align::Center);

    let state_lbl = Label::new(None);
    state_lbl.add_css_class("row-label");
    state_lbl.set_halign(Align::Center);

    let text = GtkBox::new(Orientation::Vertical, 2);
    text.set_halign(Align::Center);
    text.set_valign(Align::Center);
    text.set_can_target(false);
    text.append(&pct_lbl);
    text.append(&state_lbl);

    let overlay = Overlay::new();
    overlay.set_halign(Align::Center);
    overlay.set_child(Some(&gauge));
    overlay.add_overlay(&text);

    let detail = caption("");
    detail.set_halign(Align::Center);
    detail.set_xalign(0.5);
    detail.set_justify(gtk4::Justification::Center);

    outer.append(&overlay);
    outer.append(&detail);
    frame.set_child(Some(&outer));

    let refresh: Rc<dyn Fn()> = {
        let gauge = gauge.clone();
        Rc::new(move || {
            // 1/1024 pt units: 46080 = 45 pt, 22528 = 22 pt
            match load_battery() {
                Ok(b) => {
                    value.set(Some(b.charge));
                    charging.set(b.charging);

                    pct_lbl.set_markup(&format!(
                        "<span size=\"46080\" weight=\"bold\">{:.0}</span><span size=\"22528\" weight=\"bold\"> %</span>",
                        b.charge
                    ));

                    match b.health {
                        Some(hp) => {
                            let (word, color) = health_word(hp);
                            state_lbl.set_markup(&format!(
                                "<span foreground=\"{}\" weight=\"bold\">{} Health ({:.0}%)</span>",
                                hex(color),
                                word,
                                hp
                            ));
                        }
                        None => state_lbl.set_text("Health unavailable"),
                    }

                    let mut parts: Vec<String> = Vec::new();
                    if b.charging {
                        parts.push("Charging".to_string());
                    }
                    if let (Some(f), Some(d)) = (b.full_wh, b.design_wh) {
                        parts.push(format!("Holds {f:.1} Wh of {d:.1} Wh design capacity"));
                    }
                    if let Some(c) = b.cycles {
                        parts.push(format!("{c:.0} cycles"));
                    }
                    detail.set_text(&parts.join(" · "));
                }
                Err(msg) => {
                    value.set(None);
                    charging.set(false);
                    pct_lbl.set_markup("<span size=\"46080\" weight=\"bold\">—</span>");
                    state_lbl.set_text("Unavailable");
                    detail.set_text(msg);
                }
            }
            gauge.queue_draw();
        })
    };

    {
        let refresh = refresh.clone();
        frame.connect_map(move |_| refresh());
    }

    // keep the percentage live; stops by itself once the page is gone
    {
        let refresh = refresh.clone();
        let weak = gauge.downgrade();
        glib::timeout_add_seconds_local(REFRESH_SECS, move || match weak.upgrade() {
            Some(_) => {
                refresh();
                glib::ControlFlow::Continue
            }
            None => glib::ControlFlow::Break,
        });
    }

    frame
}

// ───────────────────────────── charge control ─────────────────────────────

/// the only charge limits that can be picked
const THRESHOLDS: [f64; 3] = [60.0, 80.0, 100.0];
/// labelled ticks on the slider
const SLIDER_MARKS: [f64; 6] = [0.0, 20.0, 40.0, 60.0, 80.0, 100.0];
/// horizontal padding of the slider track inside its drawing area
const SLIDER_PAD: f64 = 26.0;

fn tlp_installed() -> bool {
    let mut dirs: Vec<PathBuf> = std::env::var_os("PATH")
        .map(|p| std::env::split_paths(&p).collect())
        .unwrap_or_default();
    dirs.extend(["/usr/sbin", "/sbin", "/usr/bin", "/bin"].map(PathBuf::from));
    dirs.iter().any(|d| d.join("tlp").is_file())
}

/// `tlp setcharge 0 <stop> <bat>` as root. A GUI has no terminal for sudo to ask a
/// password on, so pkexec (graphical prompt) goes first and `sudo -n` is the fallback.
fn set_charge_limit(bat: &str, stop: u32) -> Result<(), String> {
    let stop = stop.to_string();
    let tlp_args = ["tlp", "setcharge", "0", stop.as_str(), bat];

    let run = |prog: &str, pre: &[&str]| {
        Command::new(prog)
            .args(pre)
            .args(tlp_args)
            .stdin(Stdio::null())
            .output()
    };

    let out = match run("pkexec", &[]) {
        Ok(o) => o,
        Err(_) => run("sudo", &["-n"]).map_err(|e| format!("couldn't run pkexec or sudo: {e}"))?,
    };

    if out.status.success() {
        return Ok(());
    }
    match out.status.code() {
        Some(126) | Some(127) => Err("authentication cancelled".to_string()),
        _ => {
            let err = String::from_utf8_lossy(&out.stderr);
            let err = err.trim();
            Err(if err.is_empty() {
                format!("tlp exited with {}", out.status)
            } else {
                err.to_string()
            })
        }
    }
}

fn nearest_stop(v: f64) -> f64 {
    THRESHOLDS
        .iter()
        .copied()
        .min_by(|a, b| (a - v).abs().total_cmp(&(b - v).abs()))
        .unwrap_or(100.0)
}

fn stop_hint(v: f64) -> &'static str {
    if v <= 60.0 {
        "Longest battery lifespan. Charging stops at 60 %."
    } else if v <= 80.0 {
        "Balanced. Charging stops at 80 %."
    } else {
        "No limit. Charges to full, for maximum runtime."
    }
}

fn current_limit(dir: &Path) -> Option<f64> {
    read_num(dir, "charge_control_end_threshold").map(|v| v.clamp(0.0, 100.0))
}

fn x_to_value(area: &DrawingArea, x: f64) -> f64 {
    let gw = (area.width() as f64 - 2.0 * SLIDER_PAD).max(1.0);
    ((x - SLIDER_PAD) / gw * 100.0).clamp(0.0, 100.0)
}

/// 0-100 track in steps of 20. The coloured line is the battery's health, the thumb is
/// the charge limit. Only 60 / 80 / 100 can be selected.
fn build_limit_slider(
    selected: Rc<Cell<f64>>,
    health: Rc<Cell<Option<f64>>>,
    enabled: bool,
) -> DrawingArea {
    let area = DrawingArea::new();
    area.set_content_height(84);
    area.set_hexpand(true);
    area.set_focusable(true);
    if enabled {
        area.set_cursor_from_name(Some("pointer"));
    }

    area.set_draw_func(move |area, cr, w, h| {
        #[allow(deprecated)]
        let c = area.style_context().color();
        let (tr, tg, tb) = (c.red() as f64, c.green() as f64, c.blue() as f64);

        let (w, _h) = (w as f64, h as f64);
        let gw = (w - 2.0 * SLIDER_PAD).max(1.0);
        let (ty, th) = (30.0, 12.0);
        let x_of = |v: f64| SLIDER_PAD + gw * (v / 100.0).clamp(0.0, 1.0);

        // track
        crate::home::rounded_rect(cr, SLIDER_PAD, ty - th / 2.0, gw, th, th / 2.0);
        cr.set_source_rgba(tr, tg, tb, 0.14);
        let _ = cr.fill();

        // battery health, as a coloured line from 0 up to the health value
        if let Some(hp) = health.get() {
            let (r, g, b) = health_word(hp).1;
            let fw = (gw * hp / 100.0).max(th);
            crate::home::rounded_rect(cr, SLIDER_PAD, ty - th / 2.0, fw, th, th / 2.0);
            cr.set_source_rgb(r, g, b);
            let _ = cr.fill();
        }

        // ticks + numbers
        cr.set_font_size(11.0);
        cr.set_line_width(1.0);
        for m in SLIDER_MARKS {
            let x = x_of(m);
            let pickable = THRESHOLDS.contains(&m);
            let alpha = if pickable { 0.75 } else { 0.32 };

            cr.set_source_rgba(tr, tg, tb, alpha);
            cr.move_to(x + 0.5, ty + th / 2.0 + 5.0);
            cr.line_to(x + 0.5, ty + th / 2.0 + 11.0);
            let _ = cr.stroke();

            let label = format!("{m:.0}");
            cr.move_to(x - label.len() as f64 * 3.3, ty + th / 2.0 + 27.0);
            let _ = cr.show_text(&label);
        }

        // thumb on the charge limit
        let tx = x_of(selected.get());
        let a = if enabled { 1.0 } else { 0.5 };
        cr.new_sub_path();
        cr.arc(tx, ty, 13.0, 0.0, std::f64::consts::TAU);
        cr.set_source_rgba(1.0, 1.0, 1.0, 0.96 * a);
        let _ = cr.fill_preserve();
        cr.set_source_rgba(0.38, 0.62, 1.0, a);
        cr.set_line_width(3.0);
        let _ = cr.stroke();
    });

    area
}

fn build_limit_card(toast: &Toast) -> Frame {
    let frame = Frame::new(None);
    let outer = GtkBox::new(Orientation::Vertical, 8);
    outer.set_margin_top(20);
    outer.set_margin_bottom(20);
    outer.set_margin_start(20);
    outer.set_margin_end(20);

    let bat_dir = find_battery_dir();
    let bat_name = bat_dir
        .as_ref()
        .and_then(|d| d.file_name())
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_else(|| "BAT0".to_string());

    let installed = tlp_installed();
    let enabled = installed && bat_dir.is_some();

    // thumb starts on whatever limit TLP has set right now
    let initial = bat_dir.as_deref().and_then(current_limit).unwrap_or(100.0);
    let selected = Rc::new(Cell::new(initial));
    let applied = Rc::new(Cell::new(initial));
    let health: Rc<Cell<Option<f64>>> = Rc::new(Cell::new(None));

    let value_lbl = Label::new(Some(&format!("{initial:.0} %")));
    value_lbl.add_css_class("shortcut-title");
    value_lbl.set_halign(Align::Start);

    let hint_lbl = caption(stop_hint(initial));

    let area = build_limit_slider(selected.clone(), health.clone(), enabled);

    let legend = caption("");

    let apply = Button::with_label("Apply");
    apply.add_css_class("sub-btn");
    apply.set_halign(Align::End);
    apply.set_sensitive(false);
    apply.set_cursor_from_name(Some("pointer"));

    // one place that moves the thumb and keeps the labels + button in step
    let set_value: Rc<dyn Fn(f64)> = {
        let selected = selected.clone();
        let applied = applied.clone();
        let value_lbl = value_lbl.clone();
        let hint_lbl = hint_lbl.clone();
        let apply = apply.clone();
        let area = area.clone();
        Rc::new(move |v: f64| {
            selected.set(v);
            value_lbl.set_text(&format!("{v:.0} %"));
            hint_lbl.set_text(stop_hint(v));
            apply.set_sensitive(enabled && (v - applied.get()).abs() > 0.5);
            area.queue_draw();
        })
    };

    if enabled {
        // click / drag: snaps to 60, 80 or 100 (anything lower lands on 60)
        let drag = GestureDrag::new();
        {
            let area = area.clone();
            let set_value = set_value.clone();
            drag.connect_drag_begin(move |_, x, _| {
                area.grab_focus();
                set_value(nearest_stop(x_to_value(&area, x)));
            });
        }
        {
            let area = area.clone();
            let set_value = set_value.clone();
            drag.connect_drag_update(move |g, dx, _| {
                if let Some((sx, _)) = g.start_point() {
                    set_value(nearest_stop(x_to_value(&area, sx + dx)));
                }
            });
        }
        area.add_controller(drag);

        // arrow keys step between the allowed limits
        let keys = EventControllerKey::new();
        {
            let selected = selected.clone();
            let set_value = set_value.clone();
            keys.connect_key_pressed(move |_, key, _, _| {
                let idx = THRESHOLDS
                    .iter()
                    .position(|t| *t == nearest_stop(selected.get()))
                    .unwrap_or(THRESHOLDS.len() - 1);
                let next = match key {
                    gdk::Key::Left | gdk::Key::Down => idx.saturating_sub(1),
                    gdk::Key::Right | gdk::Key::Up => (idx + 1).min(THRESHOLDS.len() - 1),
                    _ => return glib::Propagation::Proceed,
                };
                set_value(THRESHOLDS[next]);
                glib::Propagation::Stop
            });
        }
        area.add_controller(keys);
    }

    {
        let selected = selected.clone();
        let applied = applied.clone();
        let toast = toast.clone();
        apply.connect_clicked(move |btn| {
            let target = nearest_stop(selected.get());
            btn.set_sensitive(false);
            toast.show("Waiting for authentication…");

            let btn = btn.clone();
            let applied = applied.clone();
            let toast = toast.clone();
            let name = bat_name.clone();

            glib::spawn_future_local(async move {
                let res = gio::spawn_blocking(move || set_charge_limit(&name, target as u32)).await;
                match res {
                    Ok(Ok(())) => {
                        applied.set(target);
                        toast.show(&format!("Charge limit set to {target:.0} %"));
                    }
                    Ok(Err(e)) => {
                        eprintln!("[calibrate] tlp setcharge failed: {e}");
                        toast.show(&format!("Couldn't set charge limit: {e}"));
                        btn.set_sensitive(true);
                    }
                    Err(_) => {
                        toast.show("Couldn't set charge limit");
                        btn.set_sensitive(true);
                    }
                }
            });
        });
    }

    outer.append(&value_lbl);
    outer.append(&hint_lbl);
    outer.append(&area);
    outer.append(&legend);

    if !installed {
        let note = caption("TLP isn't installed, so the charge limit can't be changed. Install the tlp package to use this.");
        note.set_margin_top(6);
        outer.append(&note);
    } else if bat_dir.is_none() {
        let note = caption("No battery detected.");
        note.set_margin_top(6);
        outer.append(&note);
    }

    let row = GtkBox::new(Orientation::Horizontal, 0);
    row.set_margin_top(10);
    row.set_halign(Align::End);
    row.append(&apply);
    outer.append(&row);

    frame.set_child(Some(&outer));

    let refresh: Rc<dyn Fn()> = {
        let area = area.clone();
        let health = health.clone();
        let selected = selected.clone();
        let applied = applied.clone();
        let set_value = set_value.clone();
        Rc::new(move || {
            // health line
            let hp = load_battery().ok().and_then(|b| b.health);
            health.set(hp);
            legend.set_text(&match hp {
                Some(p) => format!("Coloured line: battery health ({p:.0}%)"),
                None => "Battery health isn't reported by this device".to_string(),
            });

            // pick up a limit changed elsewhere, unless there's a pending choice
            if (selected.get() - applied.get()).abs() < 0.5 {
                if let Some(cur) = bat_dir.as_deref().and_then(current_limit) {
                    applied.set(cur);
                    set_value(cur);
                }
            }
            area.queue_draw();
        })
    };

    {
        let refresh = refresh.clone();
        frame.connect_map(move |_| refresh());
    }

    frame
}

// ───────────────────────────── page ─────────────────────────────

fn section_head(title: &str, subtitle: &str) -> GtkBox {
    let head = GtkBox::new(Orientation::Vertical, 2);
    head.set_hexpand(true);
    head.set_margin_top(20);
    head.set_margin_start(20);

    let t = Label::new(Some(title));
    t.add_css_class("frame-title");
    t.set_halign(Align::Start);

    let s = Label::new(Some(subtitle));
    s.add_css_class("frame-subtitle");
    s.set_halign(Align::Start);

    head.append(&t);
    head.append(&s);
    head
}

/// shown instead of the battery cards on machines without a battery (desktop PCs)
fn build_no_battery_card() -> Frame {
    let frame = Frame::new(None);

    let inner = GtkBox::new(Orientation::Vertical, 8);
    inner.set_margin_top(40);
    inner.set_margin_bottom(40);
    inner.set_margin_start(20);
    inner.set_margin_end(20);
    inner.set_halign(Align::Center);
    inner.set_valign(Align::Center);

    let title = Label::new(Some("No battery found"));
    title.add_css_class("shortcut-title");
    title.set_halign(Align::Center);

    let msg = Label::new(Some("This device doesn't seem to have a battery, so there's nothing to show here."));
    msg.add_css_class("shortcut-desc");
    msg.set_halign(Align::Center);
    msg.set_justify(gtk4::Justification::Center);
    msg.set_wrap(true);

    inner.append(&title);
    inner.append(&msg);
    frame.set_child(Some(&inner));
    frame
}

pub fn build_battery_page() -> Overlay {
    let content = GtkBox::new(Orientation::Vertical, 16);

    // header card
    let card = GtkBox::new(Orientation::Vertical, 10);
    card.add_css_class("shortcut-card");
    card.add_css_class("detail-card");
    card.set_margin_top(10);
    card.set_hexpand(true);

    let infoicon = Image::from_file("/var/lib/cynager/icons/battfull.svg");
    infoicon.set_pixel_size(54);
    infoicon.set_halign(Align::Center);
    infoicon.set_css_classes(&["card-icons"]);

    let title = Label::new(Some("Battery"));
    title.add_css_class("shortcut-title");
    title.set_halign(Align::Center);

    let subtitle = Label::new(Some("Manage Your Power Source in Laptops"));
    subtitle.add_css_class("shortcut-desc");
    subtitle.set_halign(Align::Center);

    card.append(&infoicon);
    card.append(&title);
    card.append(&subtitle);
    content.append(&card);

    let toast = Toast::new();

    if find_battery_dir().is_none() {
        // desktop PC: nothing else to show
        content.append(&build_no_battery_card());
    } else {
        content.append(&section_head(
            "Battery Percentage and Health",
            "Current charge, and how much capacity your battery has left compared to when it was new.",
        ));
        content.append(&build_health_card());

        content.append(&section_head(
            "Charge Control",
            "Stop charging before 100 % to slow down battery ageing. Applied with TLP.",
        ));
        content.append(&build_limit_card(&toast));
    }

    let overlay = Overlay::new();
    let scroller: ScrolledWindow = page_scroller(&content);
    overlay.set_child(Some(&scroller));
    overlay.add_overlay(&toast.revealer);
    overlay
}