use gtk4::prelude::*;
use gtk4::{
    Align, Box as GtkBox, Button, DrawingArea, Grid, Image, Label, Orientation, Overlay,
    PositionType, Revealer, RevealerTransitionType, Scale, gio, glib,
};
use std::cell::{Cell, RefCell};
use std::collections::VecDeque;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::rc::Rc;
use std::time::Duration;

use crate::home::page_scroller;

const POWER_SUPPLY_DIR: &str = "/sys/class/power_supply";
const BAT_ICON: &str = "/var/lib/cynager/icons/bat.svg";

/// seconds between live refreshes
const SAMPLE_SECS: u32 = 2;
/// samples kept for the graph (150 * 2 s = 5 minutes)
const MAX_SAMPLES: usize = 150;

/// the only charge limits offered by the slider
const THRESHOLDS: [f64; 3] = [60.0, 80.0, 100.0];

// ---------------------------------------------------------------------------
// sysfs helpers
// ---------------------------------------------------------------------------

struct Battery {
    /// e.g. "BAT0", this is what `tlp setcharge` wants
    name: String,
    path: PathBuf,
}

fn read_trimmed(path: &Path) -> Option<String> {
    fs::read_to_string(path)
        .ok()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
}

fn read_text(dir: &Path, file: &str) -> Option<String> {
    read_trimmed(&dir.join(file))
}

fn read_num(dir: &Path, file: &str) -> Option<f64> {
    read_text(dir, file)?.parse::<f64>().ok()
}

fn find_battery() -> Option<Battery> {
    let mut found: Vec<(String, PathBuf)> = fs::read_dir(POWER_SUPPLY_DIR)
        .ok()?
        .filter_map(|e| e.ok())
        .filter_map(|e| {
            let path = e.path();
            if read_text(&path, "type")? != "Battery" {
                return None;
            }
            // skip batteries of peripherals (mice, keyboards, ...)
            if read_text(&path, "scope").as_deref() == Some("Device") {
                return None;
            }
            Some((e.file_name().to_string_lossy().to_string(), path))
        })
        .collect();

    found.sort_by(|a, b| a.0.cmp(&b.0));
    found.into_iter().next().map(|(name, path)| Battery { name, path })
}

struct Snapshot {
    percent: f64,
    status: String,
    power_w: f64,
    voltage_v: f64,
    now_wh: Option<f64>,
    full_wh: Option<f64>,
}

/// Energy in Wh. Uses energy_* when the driver has it, otherwise charge_* (uAh)
/// multiplied by the nominal voltage.
fn energy_wh(dir: &Path, kind: &str) -> Option<f64> {
    if let Some(e) = read_num(dir, &format!("energy_{kind}")) {
        return Some(e / 1e6);
    }
    let charge = read_num(dir, &format!("charge_{kind}"))? / 1e6;
    let volts = read_num(dir, "voltage_min_design")
        .or_else(|| read_num(dir, "voltage_now"))?
        / 1e6;
    Some(charge * volts)
}

fn read_snapshot(dir: &Path) -> Snapshot {
    let voltage_v = read_num(dir, "voltage_now").map(|v| v / 1e6).unwrap_or(0.0);

    let power_w = read_num(dir, "power_now")
        .map(|p| p.abs() / 1e6)
        .or_else(|| read_num(dir, "current_now").map(|c| c.abs() / 1e6 * voltage_v))
        .unwrap_or(0.0);

    let now_wh = energy_wh(dir, "now");
    let full_wh = energy_wh(dir, "full");

    let percent = read_num(dir, "capacity")
        .or_else(|| match (now_wh, full_wh) {
            (Some(n), Some(f)) if f > 0.0 => Some(n / f * 100.0),
            _ => None,
        })
        .unwrap_or(0.0);

    Snapshot {
        percent,
        status: read_text(dir, "status").unwrap_or_else(|| "Unknown".to_string()),
        power_w,
        voltage_v,
        now_wh,
        full_wh,
    }
}

fn fmt_duration(hours: f64) -> String {
    let total_min = (hours * 60.0).round() as u64;
    if total_min > 99 * 60 {
        return "—".to_string();
    }
    let (h, m) = (total_min / 60, total_min % 60);
    if h == 0 {
        format!("{m} min")
    } else {
        format!("{h} h {m} min")
    }
}

fn advanced_rows(dir: &Path) -> Vec<(&'static str, String)> {
    let design = energy_wh(dir, "full_design");
    let full = energy_wh(dir, "full");

    let health = match (design, full) {
        (Some(d), Some(f)) if d > 0.0 => format!("{:.1} %", f / d * 100.0),
        _ => "—".to_string(),
    };
    let wh = |v: Option<f64>| v.map(|v| format!("{v:.1} Wh")).unwrap_or_else(|| "—".to_string());
    let text = |f: &str| read_text(dir, f).unwrap_or_else(|| "—".to_string());

    vec![
        ("Health", health),
        (
            "Cycles",
            read_num(dir, "cycle_count")
                .map(|c| format!("{c:.0}"))
                .unwrap_or_else(|| "Not reported".to_string()),
        ),
        ("Manufacturer", text("manufacturer")),
        ("Model", text("model_name")),
        ("Technology", text("technology")),
        ("Design capacity", wh(design)),
        ("Full charge capacity", wh(full)),
        ("Serial", text("serial_number")),
        (
            "Charge limit",
            read_num(dir, "charge_control_end_threshold")
                .map(|v| format!("{v:.0} %"))
                .unwrap_or_else(|| "Not available".to_string()),
        ),
    ]
}

// ---------------------------------------------------------------------------
// TLP
// ---------------------------------------------------------------------------

fn tlp_installed() -> bool {
    let mut dirs: Vec<PathBuf> = std::env::var_os("PATH")
        .map(|p| std::env::split_paths(&p).collect())
        .unwrap_or_default();
    dirs.extend(["/usr/sbin", "/sbin", "/usr/bin", "/bin"].map(PathBuf::from));
    dirs.iter().any(|d| d.join("tlp").is_file())
}

/// `tlp setcharge 0 <stop> <bat>` as root. A GUI has no terminal for sudo to ask
/// a password on, so pkexec (graphical prompt) goes first and `sudo -n` is the fallback.
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

// ---------------------------------------------------------------------------
// toast (same look as the shell page)
// ---------------------------------------------------------------------------

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

// ---------------------------------------------------------------------------
// drawing
// ---------------------------------------------------------------------------

fn level_color(pct: f64, charging: bool) -> (f64, f64, f64) {
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

fn build_percent_ring(pct: Rc<Cell<f64>>, charging: Rc<Cell<bool>>) -> DrawingArea {
    let ring = DrawingArea::new();
    ring.set_content_width(96);
    ring.set_content_height(96);
    ring.set_halign(Align::Center);
    ring.set_valign(Align::Center);

    ring.set_draw_func(move |area, cr, w, h| {
        #[allow(deprecated)]
        let c = area.style_context().color();

        let (w, h) = (w as f64, h as f64);
        let line = 8.0;
        let r = (w.min(h) - line) / 2.0;
        let (cx, cy) = (w / 2.0, h / 2.0);

        cr.set_line_width(line);
        cr.set_line_cap(gtk4::cairo::LineCap::Round);

        // track, follows the theme's text colour so it shows in light and dark
        cr.set_source_rgba(c.red() as f64, c.green() as f64, c.blue() as f64, 0.12);
        cr.arc(cx, cy, r, 0.0, std::f64::consts::TAU);
        let _ = cr.stroke();

        let frac = (pct.get() / 100.0).clamp(0.0, 1.0);
        if frac > 0.0 {
            let (rr, gg, bb) = level_color(pct.get(), charging.get());
            let start = -std::f64::consts::FRAC_PI_2;
            cr.set_source_rgb(rr, gg, bb);
            cr.arc(cx, cy, r, start, start + std::f64::consts::TAU * frac);
            let _ = cr.stroke();
        }
    });

    ring
}

/// round the y axis up to a "nice" watt value
fn nice_ceil(peak: f64) -> f64 {
    const STEPS: [f64; 9] = [5.0, 10.0, 15.0, 20.0, 30.0, 45.0, 60.0, 90.0, 120.0];
    let want = peak * 1.1;
    STEPS.iter().copied().find(|s| *s >= want).unwrap_or(want.ceil())
}

fn build_power_graph(samples: Rc<RefCell<VecDeque<f64>>>, charging: Rc<Cell<bool>>) -> DrawingArea {
    let area = DrawingArea::new();
    area.add_css_class("bat-graph");
    area.set_content_height(190);
    area.set_hexpand(true);

    area.set_draw_func(move |area, cr, w, h| {
        #[allow(deprecated)]
        let c = area.style_context().color();
        let (fr, fg, fb) = (c.red() as f64, c.green() as f64, c.blue() as f64);

        let (w, h) = (w as f64, h as f64);
        let (pad_l, pad_r, pad_t, pad_b) = (44.0, 10.0, 12.0, 24.0);
        let gw = (w - pad_l - pad_r).max(1.0);
        let gh = (h - pad_t - pad_b).max(1.0);

        let data = samples.borrow();
        let peak = data.iter().copied().fold(0.0_f64, f64::max);
        let y_max = nice_ceil(peak);

        cr.set_font_size(10.0);
        cr.set_line_width(1.0);

        // grid + y labels
        for i in 0..=4 {
            let frac = i as f64 / 4.0;
            let y = pad_t + gh - gh * frac;

            cr.set_source_rgba(fr, fg, fb, 0.10);
            cr.move_to(pad_l, y + 0.5);
            cr.line_to(pad_l + gw, y + 0.5);
            let _ = cr.stroke();

            cr.set_source_rgba(fr, fg, fb, 0.55);
            cr.move_to(4.0, y + 3.5);
            let _ = cr.show_text(&format!("{:.0} W", y_max * frac));
        }

        // x labels
        let minutes = (MAX_SAMPLES as u32 * SAMPLE_SECS) / 60;
        cr.set_source_rgba(fr, fg, fb, 0.55);
        cr.move_to(pad_l, h - 6.0);
        let _ = cr.show_text(&format!("{minutes} min ago"));
        cr.move_to(pad_l + gw - 20.0, h - 6.0);
        let _ = cr.show_text("now");

        let n = data.len();
        if n < 2 {
            return;
        }

        // newest sample sits on the right edge
        let offset = MAX_SAMPLES - n;
        let px = |i: usize| pad_l + gw * ((offset + i) as f64 / (MAX_SAMPLES - 1) as f64);
        let py = |v: f64| pad_t + gh - gh * (v / y_max).clamp(0.0, 1.0);

        let (rr, gg, bb) = if charging.get() { (0.30, 0.78, 0.47) } else { (0.95, 0.70, 0.25) };

        // fill under the line
        cr.move_to(px(0), py(data[0]));
        for (i, v) in data.iter().enumerate().skip(1) {
            cr.line_to(px(i), py(*v));
        }
        cr.line_to(px(n - 1), pad_t + gh);
        cr.line_to(px(0), pad_t + gh);
        cr.close_path();
        cr.set_source_rgba(rr, gg, bb, 0.18);
        let _ = cr.fill();

        // the line itself
        cr.move_to(px(0), py(data[0]));
        for (i, v) in data.iter().enumerate().skip(1) {
            cr.line_to(px(i), py(*v));
        }
        cr.set_line_width(2.0);
        cr.set_line_join(gtk4::cairo::LineJoin::Round);
        cr.set_source_rgb(rr, gg, bb);
        let _ = cr.stroke();
    });

    area
}

// ---------------------------------------------------------------------------
// cards
// ---------------------------------------------------------------------------

fn new_card(icon: Option<&str>, title: &str, subtitle: Option<&str>) -> GtkBox {
    let card = GtkBox::new(Orientation::Vertical, 10);
    card.add_css_class("shortcut-card");
    card.add_css_class("detail-card");
    card.set_hexpand(true);

    if let Some(path) = icon {
        let img = Image::from_file(path);
        img.set_pixel_size(54);
        img.set_halign(Align::Start);
        img.set_css_classes(&["card-icons"]);
        card.append(&img);
    }

    let title = Label::new(Some(title));
    title.add_css_class("shortcut-title");
    title.set_halign(Align::Start);
    title.set_margin_bottom(if subtitle.is_some() { 0 } else { 20 });
    card.append(&title);

    if let Some(sub) = subtitle {
        let sub = Label::new(Some(sub));
        sub.add_css_class("shortcut-desc");
        sub.set_halign(Align::Start);
        sub.set_wrap(true);
        sub.set_xalign(0.0);
        sub.set_margin_bottom(20);
        card.append(&sub);
    }

    card
}

fn add_row(grid: &Grid, i: i32, key: &str, val: &str) -> (Label, Label) {
    let key_lbl = Label::new(Some(key));
    key_lbl.add_css_class("row-caption");
    key_lbl.set_halign(Align::Start);
    key_lbl.set_valign(Align::Start);

    let val_lbl = Label::new(Some(val));
    val_lbl.add_css_class("row-label");
    val_lbl.set_halign(Align::Start);
    val_lbl.set_hexpand(true);
    val_lbl.set_wrap(true);
    val_lbl.set_xalign(0.0);

    grid.attach(&key_lbl, 0, i, 1, 1);
    grid.attach(&val_lbl, 1, i, 1, 1);
    (key_lbl, val_lbl)
}

fn new_grid() -> Grid {
    Grid::builder().row_spacing(8).column_spacing(24).build()
}

/// "Advanced details" card; returns the value labels so the live timer can keep them fresh
fn build_details_card(bat: &Battery) -> (GtkBox, Vec<Label>) {
    let card = new_card(None, "Advanced Details", None);
    let grid = new_grid();

    let vals: Vec<Label> = advanced_rows(&bat.path)
        .into_iter()
        .enumerate()
        .map(|(i, (k, v))| add_row(&grid, i as i32, k, &v).1)
        .collect();

    card.append(&grid);
    (card, vals)
}

fn build_live_card(bat: &Battery, details: Vec<Label>) -> GtkBox {
    let card = new_card(Some(BAT_ICON), "Battery", None);

    let pct = Rc::new(Cell::new(0.0_f64));
    let charging = Rc::new(Cell::new(false));
    let samples: Rc<RefCell<VecDeque<f64>>> =
        Rc::new(RefCell::new(VecDeque::with_capacity(MAX_SAMPLES)));

    // ring with the percentage in the middle
    let ring = build_percent_ring(pct.clone(), charging.clone());
    let pct_lbl = Label::new(Some("--"));
    pct_lbl.add_css_class("row-label");
    pct_lbl.set_halign(Align::Center);
    pct_lbl.set_valign(Align::Center);

    let ring_overlay = Overlay::new();
    ring_overlay.set_child(Some(&ring));
    ring_overlay.add_overlay(&pct_lbl);
    ring_overlay.set_valign(Align::Center);

    // live numbers
    let grid = new_grid();
    let (_, status_lbl) = add_row(&grid, 0, "Status", "—");
    let (_, power_lbl) = add_row(&grid, 1, "Power draw", "—");
    let (time_key, time_lbl) = add_row(&grid, 2, "Time left", "—");
    let (_, volt_lbl) = add_row(&grid, 3, "Voltage", "—");
    let (_, energy_lbl) = add_row(&grid, 4, "Energy", "—");
    grid.set_valign(Align::Center);

    let top = GtkBox::new(Orientation::Horizontal, 28);
    top.set_margin_bottom(10);
    top.append(&ring_overlay);
    top.append(&grid);
    card.append(&top);

    // graph
    let graph_title = Label::new(Some("Power over time"));
    graph_title.add_css_class("row-caption");
    graph_title.set_halign(Align::Start);
    graph_title.set_margin_top(10);
    card.append(&graph_title);

    let graph = build_power_graph(samples.clone(), charging.clone());
    card.append(&graph);

    let path = bat.path.clone();
    let refresh: Rc<dyn Fn()> = {
        let graph = graph.clone();
        let ring = ring.clone();
        Rc::new(move || {
            let s = read_snapshot(&path);
            let status = s.status.to_lowercase();
            let is_charging = status == "charging";
            let is_discharging = status == "discharging";

            charging.set(is_charging);
            pct.set(s.percent);
            ring.queue_draw();

            pct_lbl.set_text(&format!("{:.0}%", s.percent));
            status_lbl.set_text(&s.status);
            power_lbl.set_text(&format!("{:.1} W", s.power_w));
            volt_lbl.set_text(&format!("{:.2} V", s.voltage_v));
            energy_lbl.set_text(&match (s.now_wh, s.full_wh) {
                (Some(n), Some(f)) => format!("{n:.1} / {f:.1} Wh"),
                _ => "—".to_string(),
            });

            let hours = if s.power_w >= 0.5 {
                match (s.now_wh, s.full_wh) {
                    (Some(n), _) if is_discharging => Some(n / s.power_w),
                    (Some(n), Some(f)) if is_charging => Some((f - n).max(0.0) / s.power_w),
                    _ => None,
                }
            } else {
                None
            };
            time_key.set_text(if is_charging { "Time to full" } else { "Time left" });
            time_lbl.set_text(&hours.map(fmt_duration).unwrap_or_else(|| "—".to_string()));

            {
                let mut d = samples.borrow_mut();
                if d.len() >= MAX_SAMPLES {
                    d.pop_front();
                }
                d.push_back(s.power_w);
            }
            graph.queue_draw();

            for (lbl, (_, v)) in details.iter().zip(advanced_rows(&path)) {
                lbl.set_text(&v);
            }
        })
    };

    refresh();

    // stops by itself once the page is gone
    let weak = graph.downgrade();
    glib::timeout_add_seconds_local(SAMPLE_SECS, move || match weak.upgrade() {
        Some(_) => {
            refresh();
            glib::ControlFlow::Continue
        }
        None => glib::ControlFlow::Break,
    });

    card
}

fn build_limit_card(bat: &Battery, toast: &Toast) -> GtkBox {
    let card = new_card(
        None,
        "Charge Limit",
        Some("Stop charging before 100 % to slow down battery ageing. Applied with TLP (tlp setcharge)."),
    );

    let installed = tlp_installed();
    let current = nearest_stop(read_num(&bat.path, "charge_control_end_threshold").unwrap_or(100.0));
    let applied = Rc::new(Cell::new(current));

    let value_lbl = Label::new(Some(&format!("{current:.0} %")));
    value_lbl.add_css_class("shortcut-title");
    value_lbl.set_halign(Align::Start);

    let hint_lbl = Label::new(Some(stop_hint(current)));
    hint_lbl.add_css_class("row-caption");
    hint_lbl.set_halign(Align::Start);
    hint_lbl.set_wrap(true);
    hint_lbl.set_xalign(0.0);

    // slider with detents: it always snaps to 60 / 80 / 100
    let scale = Scale::with_range(Orientation::Horizontal, 60.0, 100.0, 20.0);
    scale.set_draw_value(false);
    scale.set_round_digits(0);
    scale.set_hexpand(true);
    scale.set_margin_top(6);
    for stop in THRESHOLDS {
        scale.add_mark(stop, PositionType::Bottom, Some(&format!("{stop:.0} %")));
    }
    scale.set_value(current);
    scale.set_sensitive(installed);
    scale.set_cursor_from_name(Some("pointer"));

    let apply = Button::with_label("Apply");
    apply.add_css_class("sub-btn");
    apply.set_halign(Align::End);
    apply.set_sensitive(false);
    apply.set_cursor_from_name(Some("pointer"));

    {
        let value_lbl = value_lbl.clone();
        let hint_lbl = hint_lbl.clone();
        let apply = apply.clone();
        let applied = applied.clone();
        scale.connect_change_value(move |scale, _, value| {
            let snapped = nearest_stop(value);
            scale.set_value(snapped);
            value_lbl.set_text(&format!("{snapped:.0} %"));
            hint_lbl.set_text(stop_hint(snapped));
            apply.set_sensitive(installed && (snapped - applied.get()).abs() > 0.5);
            glib::Propagation::Stop
        });
    }

    {
        let scale = scale.clone();
        let applied = applied.clone();
        let toast = toast.clone();
        let bat_name = bat.name.clone();
        apply.connect_clicked(move |btn| {
            let target = nearest_stop(scale.value());
            btn.set_sensitive(false);
            toast.show("Waiting for authentication...");

            let btn = btn.clone();
            let applied = applied.clone();
            let toast = toast.clone();
            let bat_name = bat_name.clone();

            glib::spawn_future_local(async move {
                let name = bat_name.clone();
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

    card.append(&value_lbl);
    card.append(&hint_lbl);
    card.append(&scale);

    if !installed {
        let note = Label::new(Some("TLP isn't installed, so charge limits are unavailable. Install the tlp package to use this."));
        note.add_css_class("shortcut-desc");
        note.set_halign(Align::Start);
        note.set_wrap(true);
        note.set_xalign(0.0);
        note.set_margin_top(10);
        card.append(&note);
    }

    let row = GtkBox::new(Orientation::Horizontal, 0);
    row.set_margin_top(14);
    row.set_halign(Align::End);
    row.append(&apply);
    card.append(&row);

    card
}

fn build_no_battery_card() -> GtkBox {
    let card = new_card(Some(BAT_ICON), "Battery", Some("No battery was detected on this device."));
    card.set_margin_top(10);
    card
}

// ---------------------------------------------------------------------------
// page
// ---------------------------------------------------------------------------

pub fn build_battery_page() -> Overlay {
    let toast = Toast::new();
    let content = GtkBox::new(Orientation::Vertical, 15);

    match find_battery() {
        Some(bat) => {
            let (details_card, detail_labels) = build_details_card(&bat);
            content.append(&build_live_card(&bat, detail_labels));
            content.append(&details_card);
            content.append(&build_limit_card(&bat, &toast));
        }
        None => content.append(&build_no_battery_card()),
    }

    let overlay = Overlay::new();
    overlay.set_child(Some(&page_scroller(&content)));
    overlay.add_overlay(&toast.revealer);
    overlay
}