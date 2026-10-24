use gtk4::prelude::*;
use gtk4::{
    Align, Box as GtkBox, Button, Calendar, CenterBox, Dialog, DrawingArea, DropTarget, Entry, FlowBox, Frame,
    GestureClick, Grid, Image, Label, Orientation, Overlay, Revealer, RevealerTransitionType,
    ScrolledWindow, SelectionMode, gdk, gio, glib,
};
use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::fs;
use std::os::unix::process::CommandExt;
use std::process::{Command, Stdio};
use std::rc::Rc;
use std::time::Duration;

use infoprober::{parse, Value};

use crate::appearence::get_monitors;
use crate::home::page_scroller;

const CFGPATH: &str = "/var/lib/cynager/info.probe";

const CAPSULE_BIN: &str = "capsule";

// ---------------------------------------------------------------------------
// config helpers
// ---------------------------------------------------------------------------

fn read_set_str(key: &str) -> Option<String> {
    let src = fs::read_to_string(CFGPATH).ok()?;
    let doc = parse(&src).ok()?;
    doc.get("set", key)
        .and_then(|v| v.as_str())
        .map(|s| s.to_string())
}

fn write_set_str(key: &str, val: &str) -> Result<(), String> {
    let src = fs::read_to_string(CFGPATH).map_err(|e| format!("failed to read cfg: {e}"))?;
    let mut doc = parse(&src).map_err(|e| format!("failed to parse cfg: {e}"))?;

    let set = doc.section_or_insert("set");
    match set.get_mut(key) {
        Some(v) => *v = Value::from(val.to_string()),
        None => set.push(key, val.to_string()),
    }

    doc.validate().map_err(|e| format!("not writing cfg: {e}"))?;
    fs::write(CFGPATH, doc.render()).map_err(|e| format!("failed to write cfg: {e}"))?;

    println!("[calibrate] {key} updated");
    Ok(())
}

fn read_shellout() -> Option<String> {
    read_set_str("shellout")
}

fn write_shellout(monitor: &str) -> Result<(), String> {
    write_set_str("shellout", monitor)
}

/// `widgets :{ cal :true ... }` -> { "cal": true, ... }
fn read_widget_states() -> HashMap<String, bool> {
    let mut out = HashMap::new();
    let Ok(src) = fs::read_to_string(CFGPATH) else { return out };
    let Ok(doc) = parse(&src) else { return out };

    if let Some(map) = doc.get("set", "widgets").and_then(|v| v.as_map()) {
        for e in map.iter() {
            out.insert(e.key.to_string(), e.value.as_str() == Some("true"));
        }
    }
    out
}

/// `nocsd :{ steam :true harbor :false }` -> ["steam"] (apps with nocsd true)
fn read_nocsd_apps() -> Vec<String> {
    let Ok(src) = fs::read_to_string(CFGPATH) else { return Vec::new() };
    let Ok(doc) = parse(&src) else { return Vec::new() };

    let mut out = Vec::new();
    if let Some(map) = doc.get("set", "nocsd").and_then(|v| v.as_map()) {
        for e in map.iter() {
            if e.value.as_str() == Some("true") {
                out.push(e.key.to_string());
            }
        }
    }
    out
}

/// Flip one entry inside `widgets :{ ... }` in the `:set` section, e.g.
/// `cal :true` -> `cal :false`. Edited as text so the rest of the file
/// (comments, ordering, spacing) stays exactly as it was.
fn write_widget_state(key: &str, val: bool) -> Result<(), String> {
    let src = fs::read_to_string(CFGPATH).map_err(|e| format!("failed to read cfg: {e}"))?;
    let new_val = if val { "true" } else { "false" };

    let mut lines: Vec<String> = src.split('\n').map(|l| l.to_string()).collect();
    let mut in_widgets = false;
    let mut entry_indent: Option<String> = None;
    let mut close_idx: Option<usize> = None;
    let mut done = false;

    for i in 0..lines.len() {
        let line = lines[i].clone();
        let trimmed = line.trim();

        if !in_widgets {
            if trimmed.starts_with("widgets") && trimmed.contains(":{") {
                in_widgets = true;
            }
            continue;
        }

        if trimmed == "}" {
            close_idx = Some(i);
            break;
        }

        let indent: String = line.chars().take_while(|c| c.is_whitespace()).collect();
        if trimmed.split_whitespace().next() == Some(key) {
            lines[i] = format!("{indent}{key} :{new_val}");
            done = true;
            break;
        }
        entry_indent.get_or_insert(indent);
    }

    if !done {
        // key wasn't there yet, add it just before the closing brace
        let idx = close_idx.ok_or_else(|| "no widgets block in cfg".to_string())?;
        let indent = entry_indent.unwrap_or_else(|| "        ".to_string());
        lines.insert(idx, format!("{indent}{key} :{new_val}"));
    }

    let out = lines.join("\n");
    parse(&out).map_err(|e| format!("not writing cfg: {e}"))?;
    fs::write(CFGPATH, out).map_err(|e| format!("failed to write cfg: {e}"))?;

    println!("[calibrate] widget {key} -> {new_val}");
    Ok(())
}

// ---------------------------------------------------------------------------
// capsule process
// ---------------------------------------------------------------------------

fn restart_capsule() {
    std::thread::spawn(|| {
        let _ = Command::new("pkill").args(["-x", CAPSULE_BIN]).status();
        std::thread::sleep(Duration::from_millis(400));

        match Command::new(CAPSULE_BIN)
            .process_group(0)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
        {
            Ok(mut child) => {
                let _ = child.wait();
            }
            Err(e) => eprintln!("[calibrate] failed to relaunch {CAPSULE_BIN}: {e}"),
        }
    });
}

fn capsule_running() -> bool {
    Command::new("pgrep")
        .args(["-x", CAPSULE_BIN])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}

// ---------------------------------------------------------------------------
// toast
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
// monitors + status bar
// ---------------------------------------------------------------------------

fn build_monitor_button(name: &str, make: &str) -> Button {
    let name_lbl = Label::new(Some(name));
    name_lbl.add_css_class("monitor-btn-label");
    name_lbl.set_halign(Align::Center);
    name_lbl.set_valign(Align::Center);

    let btn = Button::new();
    btn.set_height_request(200);
    btn.set_width_request(300);
    btn.set_child(Some(&name_lbl));
    btn.add_css_class("monitor-btn");
    btn.set_tooltip_text(Some(make));
    btn.set_cursor_from_name(Some("pointer"));
    btn
}

fn mark_selected(buttons: &[(String, Button)], selected: &str) {
    for (name, btn) in buttons {
        if name == selected {
            btn.add_css_class("monitor-selected");
        } else {
            btn.remove_css_class("monitor-selected");
        }
    }
}

fn build_status_bar(toast: &Toast) -> GtkBox {
    let dead_lbl = Label::new(Some("Capsule shell is dead :("));
    dead_lbl.set_halign(Align::Start);
    dead_lbl.set_margin_start(10);
    dead_lbl.set_hexpand(true);

    let restart_btn = Button::with_label("Restart");
    restart_btn.add_css_class("destructive-action");
    restart_btn.set_cursor_from_name(Some("pointer"));

    let dead_bar = GtkBox::new(Orientation::Horizontal, 12);
    dead_bar.add_css_class("capsule-dead");
    dead_bar.set_hexpand(true);
    dead_bar.append(&dead_lbl);
    dead_bar.append(&restart_btn);

    let update = |bar: &GtkBox| {
        bar.set_visible(!capsule_running());
    };
    update(&dead_bar);

    restart_btn.connect_clicked({
        let toast = toast.clone();
        move |_| {
            toast.show("Restarting capsule...");
            restart_capsule();
        }
    });

    let weak = dead_bar.downgrade();
    glib::timeout_add_local(Duration::from_secs(2), move || match weak.upgrade() {
        Some(bar) => {
            update(&bar);
            glib::ControlFlow::Continue
        }
        None => glib::ControlFlow::Break,
    });

    dead_bar
}

// ---------------------------------------------------------------------------
// ghost widgets
// ---------------------------------------------------------------------------

fn ghost_block(w: i32, h: i32, extra: &str) -> GtkBox {
    GtkBox::builder()
        .css_classes(["ghost-block", extra])
        .width_request(w)
        .height_request(h)
        .build()
}

fn ghost_shell(kind: &str, enabled: bool) -> GtkBox {
    let g = GtkBox::builder()
        .orientation(Orientation::Vertical)
        .spacing(6)
        .css_classes(["ghost-widget", kind])
        .halign(Align::Center)
        .width_request(120)
        .height_request(120)
        .overflow(gtk4::Overflow::Hidden)
        .build();
    if !enabled {
        g.add_css_class("ghost-widget-off");
    }
    g
}

fn ghost_calendar(enabled: bool) -> GtkBox {
    let g = ghost_shell("ghost-cal", enabled);
    g.set_width_request(270);
    g.set_height_request(240);

    // real GTK calendar, purely decorative: it never takes clicks or focus,
    // so a click on it falls through to the widget toggle
    let cal = Calendar::new();
    cal.add_css_class("ghost-calendar");
    cal.set_can_target(false);
    cal.set_can_focus(false);
    cal.set_focusable(false);
    cal.set_halign(Align::Center);
    cal.set_valign(Align::Center);
    cal.set_vexpand(true);
    cal.set_height_request(100);
    g.append(&cal);
    g
}

fn ghost_system(enabled: bool) -> GtkBox {
    let g = ghost_shell("ghost-sys", enabled);
    g.set_valign(Align::Center);
    let mus = Image::from_file("/var/lib/cynager/icons/audio.svg");
    mus.set_pixel_size(56);
    mus.set_hexpand(true);
    mus.set_vexpand(true);
    mus.set_halign(Align::Center);
    mus.set_valign(Align::Center);
    let vyln = GtkBox::builder()
        .css_classes(["ghost-vynl"])
        .height_request(150)
        .width_request(150)
        .halign(Align::Start)
        .valign(Align::Center)
        .build();
    vyln.append(&mus);
    g.append(&vyln);
    g
}

fn ghost_battery(enabled: bool) -> GtkBox {
    const LEVEL: f64 = 57.0;

    let g = ghost_shell("ghost-bat", enabled);
    g.set_halign(Align::Center);
    g.set_valign(Align::Center);
    g.set_height_request(150);
    g.set_width_request(150);

    let row = GtkBox::new(Orientation::Horizontal, 0);
    row.set_hexpand(true);
    row.set_vexpand(true);

    // left column: ring on top, "57 %" in the bottom-left corner
    let left = GtkBox::new(Orientation::Vertical, 0);
    left.set_hexpand(true);
    left.set_vexpand(true);

    // ring, coloured by CSS `color` on .ghost-battery-ring
    let ring = DrawingArea::new();
    ring.add_css_class("ghost-battery-ring");
    ring.set_content_width(28);
    ring.set_content_height(28);
    ring.set_halign(Align::Start);
    ring.set_valign(Align::Start);
    ring.set_draw_func(|area, cr, w, h| {
        #[allow(deprecated)]
        let c = area.style_context().color();
        let (w, h) = (w as f64, h as f64);
        let line = 3.0;
        let r = (w.min(h) - line) / 2.0;
        let start = -std::f64::consts::FRAC_PI_2;
        let end = start + 2.0 * std::f64::consts::PI * (LEVEL / 100.0);

        cr.set_line_width(line);
        cr.set_line_cap(gtk4::cairo::LineCap::Round);
        cr.set_source_rgba(c.red() as f64, c.green() as f64, c.blue() as f64, c.alpha() as f64);
        cr.arc(w / 2.0, h / 2.0, r, start, end);
        let _ = cr.stroke();
    });

    let pct_row = GtkBox::new(Orientation::Horizontal, 2);
    pct_row.set_halign(Align::Start);
    pct_row.set_valign(Align::End);
    pct_row.set_vexpand(true);

    let pct = Label::new(Some("57"));
    pct.add_css_class("ghost-battery-pct");
    pct.set_valign(Align::End);

    let sym = Label::new(Some("%"));
    sym.add_css_class("ghost-battery-sym");
    sym.set_valign(Align::End);

    pct_row.append(&pct);
    pct_row.append(&sym);

    left.append(&ring);
    left.append(&pct_row);

    row.append(&left);
    g.append(&row);
    g
}

fn ghost_app_drawer(enabled: bool) -> GtkBox {
    let g = ghost_shell("ghost-appd", enabled);
    g.set_halign(Align::Center);
    g.set_valign(Align::Center);
    g.set_height_request(150);
    g.set_width_request(150);
    let grid = Grid::new();
    grid.set_row_spacing(10);
    grid.set_column_spacing(10);
    grid.set_halign(Align::Center);
    grid.set_valign(Align::Center);
    grid.set_vexpand(true);
    for r in 0..2 {
        for c in 0..3 {
            grid.attach(&ghost_block(46, 46, "ghost-app"), c, r, 1, 1);
        }
    }
    g.append(&grid);
    g
}

fn ghost_git(enabled: bool) -> GtkBox {
    let g = ghost_shell("ghost-appd", enabled);
    let grid = Grid::new();
    g.set_halign(Align::Center);
    g.set_valign(Align::Center);
    grid.set_row_spacing(3);
    grid.set_column_spacing(3);
    grid.set_halign(Align::Center);
    grid.set_valign(Align::Center);
    grid.set_vexpand(true);
    for r in 0..5 {
        for c in 0..12 {
            let level = (r * 7 + c * 3 + r * c) % 4;
            grid.attach(&ghost_block(9, 9, &format!("ghost-git-{level}")), c, r, 1, 1);
        }
    }
    g.append(&grid);
    g
}

fn set_sticker_image(img: &Image, path: &str) {
    if !path.is_empty() && std::path::Path::new(path).is_file() {
        img.set_from_file(Some(path));
    } else {
        img.set_icon_name(Some("image-x-generic-symbolic"));
    }
}

fn ghost_sticker(enabled: bool, path: &str) -> (GtkBox, Image) {
    let g = ghost_shell("ghost-stick", enabled);

    let img = Image::new();
    img.set_pixel_size(120);
    img.set_halign(Align::Center);
    img.set_valign(Align::Center);
    img.set_vexpand(true);
    set_sticker_image(&img, path);

    g.append(&img);
    (g, img)
}

fn open_setting_dialog(
    anchor: &impl IsA<gtk4::Widget>,
    title: &str,
    hint: &str,
    key: &'static str,
    initial: &str,
    secret: bool,
    toast: &Toast,
    on_saved: impl Fn(&str) + 'static,
) {
    let mut builder = Dialog::builder().modal(true).title(title);
    if let Some(win) = anchor.root().and_then(|r| r.downcast::<gtk4::Window>().ok()) {
        builder = builder.transient_for(&win);
    }
    let dialog = builder.build();
    dialog.add_button("Save", gtk4::ResponseType::Accept);

    let outer = GtkBox::new(Orientation::Vertical, 8);
    outer.set_margin_top(16);
    outer.set_margin_bottom(16);
    outer.set_margin_start(16);
    outer.set_margin_end(16);

    let heading = Label::new(Some(title));
    heading.add_css_class("title");
    outer.append(&heading);

    let hint_lbl = Label::new(Some(hint));
    hint_lbl.add_css_class("dim-label");
    hint_lbl.set_halign(Align::Start);
    outer.append(&hint_lbl);

    let entry = Entry::new();
    entry.set_width_chars(40);
    entry.set_activates_default(true);
    if secret {
        entry.set_visibility(false);
        entry.set_placeholder_text(Some("Paste new token"));
    } else {
        entry.set_text(initial);
    }
    outer.append(&entry);

    dialog.content_area().append(&outer);
    dialog.set_default_response(gtk4::ResponseType::Accept);

    let toast = toast.clone();
    let title_owned = title.to_string();
    dialog.connect_response(move |dialog, response| {
        if response == gtk4::ResponseType::Accept {
            let val = entry.text().trim().to_string();
            if !val.is_empty() {
                match write_set_str(key, &val) {
                    Ok(()) => {
                        on_saved(&val);
                        toast.show(&format!("{title_owned} saved"));
                    }
                    Err(e) => {
                        eprintln!("[calibrate] {e}");
                        toast.show("Couldn't save setting");
                    }
                }
            }
        }
        dialog.close();
    });

    dialog.present();
}

const STICKER_EXTS: [&str; 6] = ["png", "jpg", "jpeg", "webp", "bmp", "gif"];

fn install_sticker(src: &std::path::Path) -> Result<String, String> {
    let ext = src
        .extension()
        .and_then(|e| e.to_str())
        .map(|e| e.to_lowercase())
        .filter(|e| STICKER_EXTS.contains(&e.as_str()))
        .ok_or_else(|| "not a supported image".to_string())?;

    let mut dir = glib::home_dir();
    dir.push(".config");
    dir.push("sticker");
    fs::create_dir_all(&dir).map_err(|e| format!("failed to create sticker dir: {e}"))?;

    let dest = dir.join(format!("stick.{ext}"));

    let same = match (fs::canonicalize(src), fs::canonicalize(&dest)) {
        (Ok(a), Ok(b)) => a == b,
        _ => false,
    };
    if !same {
        fs::copy(src, &dest).map_err(|e| format!("failed to copy sticker: {e}"))?;
    }

    for e in STICKER_EXTS {
        let old = dir.join(format!("stick.{e}"));
        if old != dest {
            let _ = fs::remove_file(old);
        }
    }

    let dest_str = dest
        .to_str()
        .ok_or_else(|| "sticker path is not valid UTF-8".to_string())?
        .to_string();
    write_set_str("sticker", &dest_str)?;
    Ok(dest_str)
}

fn open_sticker_dialog(anchor: &impl IsA<gtk4::Widget>, toast: &Toast, img: &Image) {
    let mut builder = Dialog::builder().modal(true).title("Sticker");
    if let Some(win) = anchor.root().and_then(|r| r.downcast::<gtk4::Window>().ok()) {
        builder = builder.transient_for(&win);
    }
    let dialog = builder.build();
    dialog.connect_response(|d, _| d.close());

    let outer = GtkBox::new(Orientation::Vertical, 8);
    outer.set_margin_top(16);
    outer.set_margin_bottom(16);
    outer.set_margin_start(16);
    outer.set_margin_end(16);

    let heading = Label::new(Some("Change Sticker"));
    heading.add_css_class("title");
    outer.append(&heading);

    let drop_box = GtkBox::new(Orientation::Vertical, 12);
    drop_box.set_halign(Align::Fill);
    drop_box.set_valign(Align::Fill);
    drop_box.set_hexpand(true);
    drop_box.set_vexpand(true);
    drop_box.set_size_request(360, 220);
    drop_box.add_css_class("drop-zone");

    let icon = Image::from_icon_name("image-x-generic-symbolic");
    icon.set_pixel_size(48);
    icon.set_vexpand(true);
    icon.set_valign(Align::End);

    let hint = Label::new(Some("Drag & drop an image here"));
    hint.add_css_class("dim-label");
    hint.set_vexpand(true);
    hint.set_valign(Align::Start);

    drop_box.append(&icon);
    drop_box.append(&hint);

    let target = DropTarget::new(gio::File::static_type(), gdk::DragAction::COPY);

    {
        let drop_box = drop_box.clone();
        target.connect_enter(move |_, _, _| {
            drop_box.add_css_class("drag-active");
            gdk::DragAction::COPY
        });
    }
    {
        let drop_box = drop_box.clone();
        target.connect_leave(move |_| {
            drop_box.remove_css_class("drag-active");
        });
    }
    {
        let drop_box = drop_box.clone();
        let icon = icon.clone();
        let hint = hint.clone();
        let dialog = dialog.clone();
        let toast = toast.clone();
        let img = img.clone();

        target.connect_drop(move |_, value, _, _| {
            let Ok(file) = value.get::<gio::File>() else { return false };
            let Some(path) = file.path() else { return false };

            match install_sticker(&path) {
                Ok(dest) => {
                    set_sticker_image(&img, &dest);
                    toast.show("Sticker updated");

                    drop_box.remove_css_class("drag-active");
                    drop_box.add_css_class("success");
                    drop_box.remove(&icon);
                    drop_box.remove(&hint);

                    let tick = Label::new(Some("\u{2713}"));
                    tick.add_css_class("tick");
                    tick.set_vexpand(true);
                    tick.set_valign(Align::End);
                    let done = Label::new(Some("Added"));
                    done.add_css_class("dim-label");
                    done.set_vexpand(true);
                    done.set_valign(Align::Start);
                    drop_box.append(&tick);
                    drop_box.append(&done);

                    let dialog = dialog.clone();
                    glib::timeout_add_local_once(Duration::from_millis(650), move || {
                        dialog.close();
                    });
                    true
                }
                Err(e) => {
                    eprintln!("[calibrate] {e}");
                    drop_box.remove_css_class("drag-active");
                    toast.show("Couldn't set sticker (images only)");
                    false
                }
            }
        });
    }

    drop_box.add_controller(target);
    outer.append(&drop_box);
    dialog.content_area().append(&outer);
    dialog.present();
}

/// columns in the widgets grid (fixed so the shared border lines up)
const WIDGET_COLS: u32 = 3;

fn build_widgets_frame(toast: &Toast) -> Frame {
    let frame = Frame::new(None);
    let boxv = GtkBox::new(Orientation::Vertical, 10);

    let title = Label::new(Some("Widgets"));
    title.add_css_class("frame-title");
    title.set_halign(Align::Start);
    title.set_margin_start(20);
    title.set_margin_top(20);
    title.set_margin_end(20);

    let subtitle = Label::new(Some("Widgets shown on the Capsule shell. Dimmed ones are turned off."));
    subtitle.add_css_class("frame-subtitle");
    subtitle.set_halign(Align::Start);
    subtitle.set_margin_start(20);
    subtitle.set_margin_end(20);

    boxv.append(&title);
    boxv.append(&subtitle);

    let states = read_widget_states();
    let on = |k: &str| states.get(k).copied().unwrap_or(false);
    let sticker_path = read_set_str("sticker").unwrap_or_default();

    let flow = FlowBox::new();
    flow.set_selection_mode(SelectionMode::None);
    flow.set_homogeneous(true);
    flow.set_activate_on_single_click(false);
    flow.set_min_children_per_line(WIDGET_COLS);
    flow.set_max_children_per_line(WIDGET_COLS);
    flow.set_row_spacing(0);
    flow.set_column_spacing(0);
    flow.set_halign(Align::Start);
    flow.add_css_class("widget-grid");
    flow.set_valign(Align::Start);
    flow.set_hexpand(true);
    flow.set_margin_top(10);
    flow.set_margin_bottom(20);
    flow.set_margin_start(20);
    flow.set_margin_end(20);

    let (stick_ghost, stick_img) = ghost_sticker(on("stick"), &sticker_path);

    let items: Vec<(&str, &str, GtkBox)> = vec![
        ("appd", "App Drawer", ghost_app_drawer(on("appd"))),
        ("bat", "Battery", ghost_battery(on("bat"))),
        ("cal", "Calendar", ghost_calendar(on("cal"))),
        ("git", "Git", ghost_git(on("git"))),
        ("sys", "Music", ghost_system(on("sys"))),
        ("stick", "Sticker", stick_ghost),
    ];

    let total = items.len() as u32;
    let rows = (total + WIDGET_COLS - 1) / WIDGET_COLS;

    for (idx, (key, name, ghost)) in items.into_iter().enumerate() {
        let idx = idx as u32;
        let cell = GtkBox::new(Orientation::Vertical, 8);
        cell.add_css_class("widget-cell");
        cell.set_margin_top(12);
        cell.set_margin_bottom(12);
        cell.set_margin_start(12);
        cell.set_margin_end(12);
        ghost.set_halign(Align::Center);
        cell.append(&ghost);
        ghost.set_cursor_from_name(Some("pointer"));
        ghost.set_tooltip_text(Some("Click to toggle"));

        // name centred, gear (if any) pinned to the right end
        let label_row = CenterBox::new();
        label_row.set_hexpand(true);

        let lbl = Label::new(Some(name));
        lbl.add_css_class("frame-subtitle");
        if !on(key) {
            lbl.add_css_class("ghost-label-off");
        }
        label_row.set_center_widget(Some(&lbl));

        // click the ghost -> flip true/false in info.probe
        {
            let state = Rc::new(Cell::new(on(key)));
            let click = GestureClick::new();
            click.set_button(gdk::BUTTON_PRIMARY);
            let ghost_c = ghost.clone();
            let lbl_c = lbl.clone();
            let toast = toast.clone();
            click.connect_released(move |_, _, _, _| {
                let next = !state.get();
                match write_widget_state(key, next) {
                    Ok(()) => {
                        state.set(next);
                        if next {
                            ghost_c.remove_css_class("ghost-widget-off");
                            lbl_c.remove_css_class("ghost-label-off");
                        } else {
                            ghost_c.add_css_class("ghost-widget-off");
                            lbl_c.add_css_class("ghost-label-off");
                        }
                        toast.show(&format!(
                            "{name} {}",
                            if next { "enabled" } else { "disabled" }
                        ));
                    }
                    Err(e) => {
                        eprintln!("[calibrate] {e}");
                        toast.show("Couldn't change widget");
                    }
                }
            });
            ghost.add_controller(click);
        }

        if key == "git" || key == "stick" {
            let gear = Button::new();
            gear.set_child(Some(
                &Image::builder()
                    .file("/var/lib/cynager/icons/cog.svg")
                    .pixel_size(32)
                    .build()
            ));
            gear.add_css_class("flat");
            gear.add_css_class("ghost-gear");
            gear.set_cursor_from_name(Some("pointer"));
            gear.set_tooltip_text(Some(&format!("{name} settings")));

            let toast = toast.clone();
            if key == "git" {
                gear.connect_clicked(move |b| {
                    open_setting_dialog(
                        b,
                        "Git token",
                        "Personal access token used by the Git widget.",
                        "git",
                        "",
                        true,
                        &toast,
                        |_| {},
                    );
                });
            } else {
                let img = stick_img.clone();
                gear.connect_clicked(move |b| open_sticker_dialog(b, &toast, &img));
            }
            label_row.set_end_widget(Some(&gear));
        }

        cell.append(&label_row);

        let cell_frame = GtkBox::new(Orientation::Vertical, 0);
        cell_frame.add_css_class("widget-cell-frame");
        if idx % WIDGET_COLS == WIDGET_COLS - 1 || idx == total - 1 {
            cell_frame.add_css_class("last-col");
        }
        if idx / WIDGET_COLS == rows - 1 {
            cell_frame.add_css_class("last-row");
        }
        cell_frame.append(&cell);
        flow.insert(&cell_frame, -1);
    }

    boxv.append(&flow);
    frame.set_child(Some(&boxv));
    frame
}

// ---------------------------------------------------------------------------
// page
// ---------------------------------------------------------------------------

/// `on_troubleshoot` is called when the Troubleshoot button is clicked;
/// pass the closure that switches the app to the Help tab.
/// `on_apps` is called when an app icon in the Server Side Decorations frame is
/// clicked; pass the closure that switches the app to the Applications tab.
/// Icon for an app name from info.probe: its .desktop entry's icon if there
/// is one, else a theme icon with that name, else a generic fallback.
fn app_icon(app: &str) -> Image {
    let img = Image::new();
    img.set_pixel_size(48);
    img.set_halign(Align::Center);

    if let Some(icon) = gio::DesktopAppInfo::new(&format!("{app}.desktop")).and_then(|i| i.icon()) {
        img.set_from_gicon(&icon);
    } else if gdk::Display::default()
        .map(|d| gtk4::IconTheme::for_display(&d).has_icon(app))
        .unwrap_or(false)
    {
        img.set_icon_name(Some(app));
    } else {
        img.set_icon_name(Some("application-x-executable"));
    }
    img
}

fn build_ssd_frame(on_apps: impl Fn() + 'static) -> Frame {
    let frame = Frame::new(None);
    let boxv = GtkBox::new(Orientation::Vertical, 10);

    let title = Label::new(Some("Server Side Decorations"));
    title.add_css_class("frame-title");
    title.set_halign(Align::Start);
    title.set_margin_start(20);
    title.set_margin_top(20);
    title.set_margin_end(20);

    let subtitle = Label::new(Some("Window Control Buttons (SSD) and window resize button disabled for the below applications.\nTo change them please goto the specific application page and change them"));
    subtitle.add_css_class("frame-subtitle");
    subtitle.set_halign(Align::Start);
    subtitle.set_margin_start(20);
    subtitle.set_margin_end(20);

    boxv.append(&title);
    boxv.append(&subtitle);

    // app icons with the name below; click -> Applications page
    let on_apps: Rc<dyn Fn()> = Rc::new(on_apps);

    let flow = FlowBox::new();
    flow.set_selection_mode(SelectionMode::None);
    flow.set_homogeneous(false);
    flow.set_activate_on_single_click(false);
    flow.set_row_spacing(12);
    flow.set_column_spacing(12);
    flow.set_halign(Align::Start);
    flow.set_margin_start(20);
    flow.set_margin_end(20);
    flow.set_margin_top(6);
    flow.set_margin_bottom(20);

    let apps = read_nocsd_apps();
    if apps.is_empty() {
        let none = Label::new(Some("No apps have server side decorations disabled."));
        none.add_css_class("frame-subtitle");
        none.set_halign(Align::Start);
        none.set_margin_start(20);
        none.set_margin_bottom(20);
        boxv.append(&none);
    } else {
        for app in &apps {
            let icon = app_icon(app);

            let name = Label::new(Some(app));
            name.add_css_class("frame-subtitle");
            name.set_halign(Align::Center);

            let cell = GtkBox::new(Orientation::Vertical, 6);
            cell.set_margin_top(6);
            cell.set_margin_bottom(6);
            cell.set_margin_start(6);
            cell.set_margin_end(6);
            cell.append(&icon);
            cell.append(&name);

            let btn = Button::new();
            btn.set_child(Some(&cell));
            btn.add_css_class("flat");
            btn.add_css_class("ssd-app");
            btn.set_cursor_from_name(Some("pointer"));
            btn.set_tooltip_text(Some(&format!("{app} settings")));

            let on_apps = on_apps.clone();
            btn.connect_clicked(move |_| on_apps());

            flow.insert(&btn, -1);
        }
        boxv.append(&flow);
    }

    frame.set_child(Some(&boxv));
    frame
}

pub fn build_shell_page(
    on_troubleshoot: impl Fn() + 'static,
    on_apps: impl Fn() + 'static,
) -> Overlay {
    let content = GtkBox::new(Orientation::Vertical, 16);

    let card = GtkBox::new(Orientation::Vertical, 10);
    card.add_css_class("shortcut-card");
    card.add_css_class("detail-card");
    card.set_margin_top(10);
    card.set_hexpand(true);

    let infoicon = gtk4::Image::from_file("/var/lib/cynager/icons/shell.svg");
    infoicon.set_pixel_size(54);
    infoicon.set_halign(Align::Center);
    infoicon.set_css_classes(&["card-icons"]);

    let title = Label::new(Some("Shell Settings"));
    title.add_css_class("shortcut-title");
    title.set_halign(Align::Center);

    let subtitle = Label::new(Some("Configure the Capsule shell the way you want."));
    subtitle.add_css_class("shortcut-desc");
    subtitle.set_halign(Align::Center);

    card.append(&infoicon);
    card.append(&title);
    card.append(&subtitle);

    let toast = Toast::new();
    card.append(&build_status_bar(&toast));

    content.append(&card);

    // "Something not working right?" -> Help tab
    let helpframe = Frame::new(None);

    let helprow = GtkBox::new(Orientation::Horizontal, 12);
    helprow.set_margin_top(16);
    helprow.set_margin_bottom(16);
    helprow.set_margin_start(20);
    helprow.set_margin_end(20);

    let helplbl = Label::new(Some("Something not working right?"));
    helplbl.add_css_class("frame-title");
    helplbl.set_halign(Align::Start);
    helplbl.set_valign(Align::Center);
    helplbl.set_hexpand(true);

    let helpbtn = Button::with_label("Troubleshoot");
    helpbtn.add_css_class("sub-btn");
    helpbtn.set_valign(Align::Center);
    helpbtn.set_cursor_from_name(Some("pointer"));
    helpbtn.connect_clicked(move |_| on_troubleshoot());

    helprow.append(&helplbl);
    helprow.append(&helpbtn);
    helpframe.set_child(Some(&helprow));

    let outframe = Frame::new(None);

    let outbox = GtkBox::new(Orientation::Vertical, 10);

    let outtitle = Label::new(Some("Main Shell Output"));
    outtitle.add_css_class("frame-title");
    outtitle.set_halign(Align::Start);
    outtitle.set_margin_start(20);
    outtitle.set_margin_top(20);
    outtitle.set_margin_end(20);

    let outsubtitle = Label::new(Some("Choose your main display"));
    outsubtitle.add_css_class("frame-subtitle");
    outsubtitle.set_halign(Align::Start);
    outsubtitle.set_margin_start(20);
    outsubtitle.set_margin_end(20);

    let current = read_shellout().unwrap_or_else(|| "unknown".to_string());

    let monrow = GtkBox::new(Orientation::Horizontal, 10);
    monrow.set_margin_start(20);
    monrow.set_margin_end(20);
    monrow.set_margin_bottom(20);
    monrow.set_halign(Align::Start);

    let buttons: Rc<Vec<(String, Button)>> = Rc::new(
        get_monitors()
            .into_iter()
            .map(|(name, make)| {
                let btn = build_monitor_button(&name, &make);
                monrow.append(&btn);
                (name, btn)
            })
            .collect(),
    );

    mark_selected(&buttons, &current);

    for (name, btn) in buttons.iter() {
        let name = name.clone();
        let buttons = buttons.clone();
        let toast = toast.clone();

        btn.connect_clicked(move |_| match write_shellout(&name) {
            Ok(()) => {
                mark_selected(&buttons, &name);
                toast.show("Shell display output changed");
                restart_capsule();
            }
            Err(e) => {
                eprintln!("[calibrate] {e}");
                toast.show("Couldn't change shell display output");
            }
        });
    }

    outbox.append(&outtitle);
    outbox.append(&outsubtitle);
    outbox.append(&monrow);

    outframe.set_child(Some(&outbox));

    content.append(&outframe);
    content.append(&build_ssd_frame(on_apps));
    content.append(&build_widgets_frame(&toast));
    content.append(&helpframe);

    let overlay = Overlay::new();
    let scroller: ScrolledWindow = page_scroller(&content);
    overlay.set_child(Some(&scroller));
    overlay.add_overlay(&toast.revealer);
    overlay
}