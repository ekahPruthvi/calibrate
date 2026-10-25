use gtk4::prelude::*;
use gtk4::{
    Align, Box as GtkBox, Button, CheckButton, Frame, Image, Label, ListBox, ListBoxRow,
    Orientation, Overlay, Revealer, RevealerTransitionType, Scale, ScrolledWindow, Stack,
    StackSwitcher, StackTransitionType, ToggleButton, glib,
};
use std::cell::{Cell, RefCell};
use std::process::Command;
use std::rc::Rc;
use std::time::Duration;

use crate::home::page_scroller;

/// Name / description of the virtual sink that mirrors audio to several outputs.
const COMBINED_NAME: &str = "cynager_combined";
const COMBINED_DESC: &str = "Combined Output";

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

// ───────────────────────────── pactl backend ─────────────────────────────
// Works on PulseAudio and on PipeWire (via pipewire-pulse).

#[derive(Clone, Copy, PartialEq)]
enum Kind {
    Output,
    Input,
}

impl Kind {
    fn list_arg(self) -> &'static str {
        match self { Kind::Output => "sinks", Kind::Input => "sources" }
    }
    fn volume_cmd(self) -> &'static str {
        match self { Kind::Output => "set-sink-volume", Kind::Input => "set-source-volume" }
    }
    fn mute_cmd(self) -> &'static str {
        match self { Kind::Output => "set-sink-mute", Kind::Input => "set-source-mute" }
    }
    fn set_default_cmd(self) -> &'static str {
        match self { Kind::Output => "set-default-sink", Kind::Input => "set-default-source" }
    }
    fn get_default_cmd(self) -> &'static str {
        match self { Kind::Output => "get-default-sink", Kind::Input => "get-default-source" }
    }
    fn icon(self, muted: bool) -> &'static str {
        match (self, muted) {
            (Kind::Output, false) => "audio-volume-high-symbolic",
            (Kind::Output, true) => "audio-volume-muted-symbolic",
            (Kind::Input, false) => "audio-input-microphone-symbolic",
            (Kind::Input, true) => "microphone-sensitivity-muted-symbolic",
        }
    }
    fn title(self) -> &'static str {
        match self { Kind::Output => "Output Devices", Kind::Input => "Input Devices" }
    }
    fn subtitle(self) -> &'static str {
        match self {
            Kind::Output => "Select a device to change its volume or mute it.",
            Kind::Input => "Select a microphone to change its level or mute it.",
        }
    }
}

#[derive(Clone)]
struct Dev {
    name: String,
    desc: String,
    volume: u32,
    muted: bool,
}

#[derive(Default)]
struct CombinedState {
    module_ids: Vec<String>,
    slaves: Vec<String>,
}

fn pactl(args: &[&str]) -> Result<String, String> {
    let out = Command::new("pactl")
        .env("LC_ALL", "C")
        .args(args)
        .output()
        .map_err(|e| format!("cannot run pactl: {e}"))?;
    if out.status.success() {
        Ok(String::from_utf8_lossy(&out.stdout).to_string())
    } else {
        Err(String::from_utf8_lossy(&out.stderr).trim().to_string())
    }
}

/// First channel's volume in percent from a line like
/// `Volume: front-left: 65536 / 100% / 0.00 dB, front-right: ...`
fn parse_volume_percent(line: &str) -> u32 {
    line.split('/')
        .nth(1)
        .map(|s| s.trim().trim_end_matches('%'))
        .and_then(|s| s.parse::<u32>().ok())
        .unwrap_or(0)
}

fn list_devices(kind: Kind) -> Vec<Dev> {
    let out = pactl(&["list", kind.list_arg()]).unwrap_or_default();
    let mut devs: Vec<Dev> = Vec::new();

    for line in out.lines() {
        let line = line.trim();
        if line.starts_with("Sink #") || line.starts_with("Source #") {
            devs.push(Dev { name: String::new(), desc: String::new(), volume: 0, muted: false });
        } else if let Some(d) = devs.last_mut() {
            if let Some(v) = line.strip_prefix("Name:") {
                d.name = v.trim().to_string();
            } else if let Some(v) = line.strip_prefix("Description:") {
                d.desc = v.trim().to_string();
            } else if line.starts_with("Volume:") {
                d.volume = parse_volume_percent(line);
            } else if let Some(v) = line.strip_prefix("Mute:") {
                d.muted = v.trim() == "yes";
            }
        }
    }

    // hide the virtual combined sink and the "monitor of ..." pseudo microphones
    devs.retain(|d| !d.name.is_empty() && d.name != COMBINED_NAME && !d.name.ends_with(".monitor"));
    devs
}

fn default_device(kind: Kind) -> String {
    pactl(&[kind.get_default_cmd()]).unwrap_or_default().trim().to_string()
}

fn combined_state() -> CombinedState {
    let mut st = CombinedState::default();
    let out = pactl(&["list", "short", "modules"]).unwrap_or_default();
    for line in out.lines() {
        let mut parts = line.split('\t');
        let id = parts.next().unwrap_or("");
        let name = parts.next().unwrap_or("");
        let args = parts.next().unwrap_or("");
        if name == "module-combine-sink" && args.contains(COMBINED_NAME) {
            st.module_ids.push(id.to_string());
            if let Some(list) = args.split_whitespace().find_map(|t| t.strip_prefix("slaves=")) {
                st.slaves = list.split(',').map(|s| s.trim_matches('\'').to_string()).collect();
            }
        }
    }
    st
}

fn unload_combined(st: &CombinedState) {
    for id in &st.module_ids {
        let _ = pactl(&["unload-module", id]);
    }
}

fn load_combined(slaves: &[String]) -> Result<(), String> {
    let slaves_arg = format!("slaves={}", slaves.join(","));
    let name_arg = format!("sink_name={COMBINED_NAME}");
    let props_arg = format!("sink_properties=device.description='{COMBINED_DESC}'");
    pactl(&["load-module", "module-combine-sink", &name_arg, &slaves_arg, &props_arg]).map(|_| ())
}

// ───────────────────────────── device card ─────────────────────────────

fn style_mute(btn: &ToggleButton, icon: &Image, lbl: &Label, kind: Kind, muted: bool) {
    icon.set_icon_name(Some(kind.icon(muted)));
    lbl.set_text(if muted { "Unmute" } else { "Mute" });
    if muted {
        btn.remove_css_class("sub-btn");
        btn.add_css_class("destructive-action");
    } else {
        btn.remove_css_class("destructive-action");
        btn.add_css_class("sub-btn");
    }
}

/// Card with the device names on top and a slider + mute / default buttons below.
/// The slider always edits whichever device is selected in the list.
/// Returns the card and a function that re-reads everything from pactl.
fn build_device_card(kind: Kind, toast: &Toast) -> (Frame, Rc<dyn Fn()>) {
    let frame = Frame::new(None);
    let outer = GtkBox::new(Orientation::Vertical, 5);

    let title = Label::new(Some(kind.title()));
    title.add_css_class("frame-title");
    title.set_margin_start(20);
    title.set_margin_top(20);
    title.set_margin_end(20);
    title.set_halign(Align::Start);

    let sub = Label::new(Some(kind.subtitle()));
    sub.add_css_class("frame-subtitle");
    sub.set_margin_start(20);
    sub.set_margin_end(20);
    sub.set_margin_bottom(10);
    sub.set_halign(Align::Start);

    // device names
    let list = ListBox::new();
    list.set_selection_mode(gtk4::SelectionMode::Single);
    list.add_css_class("menu-list");
    list.set_margin_start(15);
    list.set_margin_end(15);

    let status = Label::new(None);
    status.add_css_class("frame-subtitle");
    status.set_margin_start(20);
    status.set_margin_end(20);
    status.set_halign(Align::Start);
    status.set_visible(false);

    // slider + buttons
    let controls = GtkBox::new(Orientation::Horizontal, 12);
    controls.set_margin_start(20);
    controls.set_margin_end(20);
    controls.set_margin_top(10);
    controls.set_margin_bottom(20);

    let scale = Scale::with_range(Orientation::Horizontal, 0.0, 100.0, 1.0);
    scale.set_draw_value(false);
    scale.set_hexpand(true);
    scale.set_margin_start(10);
    scale.set_valign(Align::Center);

    let pct = Label::new(Some("0%"));
    pct.add_css_class("row-caption");
    pct.set_margin_start(10);
    pct.set_width_chars(4);
    pct.set_xalign(1.0);

    let mute_icon = Image::from_icon_name(kind.icon(false));
    let mute_lbl = Label::new(Some("Mute"));
    let mute_inner = GtkBox::new(Orientation::Horizontal, 8);
    mute_inner.set_halign(Align::Center);
    mute_inner.append(&mute_icon);
    mute_inner.append(&mute_lbl);
    let mute = ToggleButton::new();
    mute.set_child(Some(&mute_inner));
    mute.add_css_class("sub-btn");
    mute.set_valign(Align::Center);
    mute.set_cursor_from_name(Some("pointer"));

    let default_btn = Button::new();
    default_btn.set_child(Some(&Label::new(Some("Set as default"))));
    default_btn.add_css_class("sub-btn");
    default_btn.set_valign(Align::Center);
    default_btn.set_cursor_from_name(Some("pointer"));

    controls.append(&scale);
    controls.append(&pct);
    controls.append(&mute);
    controls.append(&default_btn);

    outer.append(&title);
    outer.append(&sub);
    outer.append(&list);
    outer.append(&status);
    outer.append(&controls);
    frame.set_child(Some(&outer));

    // shared state
    let devices: Rc<RefCell<Vec<Dev>>> = Rc::new(RefCell::new(Vec::new()));
    let default: Rc<RefCell<String>> = Rc::new(RefCell::new(String::new()));
    let selected: Rc<Cell<Option<usize>>> = Rc::new(Cell::new(None));
    let updating = Rc::new(Cell::new(false)); // true while we set widgets from code
    let generation = Rc::new(Cell::new(0u32));

    // push the selected device's values into the slider / buttons
    let sync: Rc<dyn Fn()> = {
        let devices = devices.clone();
        let default = default.clone();
        let selected = selected.clone();
        let updating = updating.clone();
        let scale = scale.clone();
        let pct = pct.clone();
        let mute = mute.clone();
        let mute_icon = mute_icon.clone();
        let mute_lbl = mute_lbl.clone();
        let default_btn = default_btn.clone();
        let controls = controls.clone();
        Rc::new(move || {
            let current = selected.get().and_then(|i| devices.borrow().get(i).cloned());
            let Some(d) = current else {
                controls.set_sensitive(false);
                return;
            };
            controls.set_sensitive(true);
            updating.set(true);
            scale.set_value(d.volume.min(100) as f64);
            pct.set_text(&format!("{}%", d.volume));
            mute.set_active(d.muted);
            style_mute(&mute, &mute_icon, &mute_lbl, kind, d.muted);
            updating.set(false);
            default_btn.set_sensitive(*default.borrow() != d.name);
        })
    };

    // rebuild the list from pactl, keeping the current selection if it still exists
    let refresh: Rc<dyn Fn()> = {
        let list = list.clone();
        let status = status.clone();
        let devices = devices.clone();
        let default = default.clone();
        let selected = selected.clone();
        let updating = updating.clone();
        let sync = sync.clone();
        Rc::new(move || {
            updating.set(true);

            let prev_name = selected.get().and_then(|i| devices.borrow().get(i).map(|d| d.name.clone()));

            while let Some(r) = list.first_child() {
                list.remove(&r);
            }

            let devs = list_devices(kind);
            let def = default_device(kind);

            if devs.is_empty() {
                let empty = Label::new(Some("No devices found (is PipeWire / PulseAudio running?)"));
                empty.add_css_class("row-caption");
                empty.set_margin_top(10);
                empty.set_margin_bottom(10);
                list.append(&ListBoxRow::builder().child(&empty).activatable(false).selectable(false).build());
            }

            for d in &devs {
                let row = GtkBox::new(Orientation::Horizontal, 8);

                let name = Label::new(Some(if d.desc.is_empty() { &d.name } else { &d.desc }));
                name.set_halign(Align::Start);
                name.set_xalign(0.0);
                name.set_hexpand(true);
                name.set_ellipsize(gtk4::pango::EllipsizeMode::End);
                row.append(&name);

                if d.name == def {
                    let badge = Label::new(Some("Default"));
                    badge.set_halign(Align::End);
                    row.append(&badge);
                }

                list.append(&ListBoxRow::builder().child(&row).activatable(false).build());
            }

            // keep previous selection, else the default device, else the first one
            let sel = prev_name
                .and_then(|n| devs.iter().position(|d| d.name == n))
                .or_else(|| devs.iter().position(|d| d.name == def))
                .or(if devs.is_empty() { None } else { Some(0) });

            if let Some(i) = sel {
                if let Some(r) = list.row_at_index(i as i32) {
                    list.select_row(Some(&r));
                }
            }

            if kind == Kind::Output && def == COMBINED_NAME {
                status.set_text("Currently playing through the combined output");
                status.set_visible(true);
            } else {
                status.set_visible(false);
            }

            *devices.borrow_mut() = devs;
            *default.borrow_mut() = def;
            selected.set(sel);
            updating.set(false);
            sync();
        })
    };
    refresh();

    // clicking a name -> the slider now edits that device
    {
        let selected = selected.clone();
        let updating = updating.clone();
        let sync = sync.clone();
        list.connect_row_selected(move |_, row| {
            if updating.get() {
                return;
            }
            let Some(row) = row else { return };
            selected.set(Some(row.index() as usize));
            sync();
        });
    }

    // slider (debounced so dragging doesn't spawn a pactl process per pixel)
    {
        let devices = devices.clone();
        let selected = selected.clone();
        let updating = updating.clone();
        let generation = generation.clone();
        let pct = pct.clone();
        scale.connect_value_changed(move |s| {
            if updating.get() {
                return;
            }
            let v = s.value().round() as u32;
            pct.set_text(&format!("{v}%"));

            let Some(i) = selected.get() else { return };
            let name = {
                let mut devs = devices.borrow_mut();
                let Some(d) = devs.get_mut(i) else { return };
                d.volume = v;
                d.name.clone()
            };

            generation.set(generation.get().wrapping_add(1));
            let id = generation.get();
            let generation = generation.clone();
            glib::timeout_add_local_once(Duration::from_millis(80), move || {
                if generation.get() == id {
                    if let Err(e) = pactl(&[kind.volume_cmd(), &name, &format!("{v}%")]) {
                        eprintln!("[calibrate] failed to set volume on {name}: {e}");
                    }
                }
            });
        });
    }

    // mute / unmute
    {
        let devices = devices.clone();
        let selected = selected.clone();
        let updating = updating.clone();
        let mute_icon = mute_icon.clone();
        let mute_lbl = mute_lbl.clone();
        mute.connect_toggled(move |b| {
            if updating.get() {
                return;
            }
            let m = b.is_active();
            style_mute(b, &mute_icon, &mute_lbl, kind, m);

            let Some(i) = selected.get() else { return };
            let name = {
                let mut devs = devices.borrow_mut();
                let Some(d) = devs.get_mut(i) else { return };
                d.muted = m;
                d.name.clone()
            };
            if let Err(e) = pactl(&[kind.mute_cmd(), &name, if m { "1" } else { "0" }]) {
                eprintln!("[calibrate] failed to (un)mute {name}: {e}");
            }
        });
    }

    // make the selected device the system default
    {
        let devices = devices.clone();
        let selected = selected.clone();
        let toast = toast.clone();
        let refresh = refresh.clone();
        default_btn.connect_clicked(move |_| {
            let Some(name) = selected.get().and_then(|i| devices.borrow().get(i).map(|d| d.name.clone())) else {
                return;
            };
            match pactl(&[kind.set_default_cmd(), &name]) {
                Ok(_) => {
                    // a plain output replaces any combined output
                    if kind == Kind::Output {
                        unload_combined(&combined_state());
                        let _ = pactl(&[kind.set_default_cmd(), &name]);
                    }
                    toast.show(if kind == Kind::Output { "Default output changed" } else { "Default input changed" });
                }
                Err(e) => toast.show(&format!("Failed: {e}")),
            }
            refresh();
        });
    }

    // re-sync whenever the page becomes visible (tab switch etc.)
    {
        let refresh = refresh.clone();
        frame.connect_map(move |_| refresh());
    }

    (frame, refresh)
}

// ───────────────────────────── combined output card ─────────────────────────────

fn build_combined_card(toast: &Toast, on_applied: Rc<dyn Fn()>) -> Frame {
    let frame = Frame::new(None);
    let outer = GtkBox::new(Orientation::Vertical, 5);

    let title = Label::new(Some("Combined Output"));
    title.add_css_class("frame-title");
    title.set_margin_start(20);
    title.set_margin_top(20);
    title.set_margin_end(20);
    title.set_halign(Align::Start);

    let sub = Label::new(Some(
        "Tick two or more devices and press Apply to play through all of them at once. One device simply switches the output.",
    ));
    sub.add_css_class("frame-subtitle");
    sub.set_margin_start(20);
    sub.set_margin_end(20);
    sub.set_margin_bottom(10);
    sub.set_halign(Align::Start);
    sub.set_xalign(0.0);
    sub.set_wrap(true);

    let checks_box = GtkBox::new(Orientation::Vertical, 8);
    checks_box.set_margin_start(20);
    checks_box.set_margin_end(20);
    checks_box.set_margin_top(5);
    checks_box.set_margin_bottom(5);

    let checks: Rc<RefCell<Vec<(String, CheckButton)>>> = Rc::new(RefCell::new(Vec::new()));

    let refresh: Rc<dyn Fn()> = {
        let checks_box = checks_box.clone();
        let checks = checks.clone();
        Rc::new(move || {
            while let Some(c) = checks_box.first_child() {
                checks_box.remove(&c);
            }
            checks.borrow_mut().clear();

            let devs = list_devices(Kind::Output);
            let st = combined_state();

            if devs.is_empty() {
                let empty = Label::new(Some("No output devices found"));
                empty.add_css_class("row-caption");
                empty.set_halign(Align::Start);
                checks_box.append(&empty);
            }

            for d in devs {
                let label = if d.desc.is_empty() { d.name.clone() } else { d.desc.clone() };
                let c = CheckButton::with_label(&label);
                c.set_active(st.slaves.contains(&d.name));
                checks_box.append(&c);
                checks.borrow_mut().push((d.name, c));
            }
        })
    };
    refresh();

    let apply_btn = Button::new();
    apply_btn.set_child(Some(&Label::new(Some("Apply"))));
    apply_btn.add_css_class("sub-btn");
    apply_btn.set_halign(Align::End);
    apply_btn.set_margin_start(20);
    apply_btn.set_margin_end(20);
    apply_btn.set_margin_top(10);
    apply_btn.set_margin_bottom(20);
    apply_btn.set_cursor_from_name(Some("pointer"));

    {
        let checks = checks.clone();
        let toast = toast.clone();
        apply_btn.connect_clicked(move |_| {
            let selected: Vec<String> = checks
                .borrow()
                .iter()
                .filter(|(_, c)| c.is_active())
                .map(|(n, _)| n.clone())
                .collect();

            if selected.is_empty() {
                toast.show("Select at least one output device");
                return;
            }

            // clean slate so the slave list is exactly the selection
            unload_combined(&combined_state());

            if selected.len() == 1 {
                match pactl(&[Kind::Output.set_default_cmd(), &selected[0]]) {
                    Ok(_) => toast.show("Output switched"),
                    Err(e) => toast.show(&format!("Failed: {e}")),
                }
            } else {
                match load_combined(&selected) {
                    Ok(()) => match pactl(&[Kind::Output.set_default_cmd(), COMBINED_NAME]) {
                        Ok(_) => toast.show(&format!("Playing through {} devices", selected.len())),
                        Err(e) => toast.show(&format!("Failed: {e}")),
                    },
                    Err(e) => toast.show(&format!("Could not combine devices: {e}")),
                }
            }
            on_applied();
        });
    }

    outer.append(&title);
    outer.append(&sub);
    outer.append(&checks_box);
    outer.append(&apply_btn);
    frame.set_child(Some(&outer));

    {
        let refresh = refresh.clone();
        frame.connect_map(move |_| refresh());
    }

    frame
}

// ───────────────────────────── page ─────────────────────────────

pub fn build_sound_page() -> Overlay {
    let content = GtkBox::new(Orientation::Vertical, 16);

    // header card
    let card = GtkBox::new(Orientation::Vertical, 10);
    card.add_css_class("shortcut-card");
    card.add_css_class("detail-card");
    card.set_margin_top(10);
    card.set_hexpand(true);

    let infoicon = Image::from_file("/var/lib/cynager/icons/soundmed.svg");
    infoicon.set_pixel_size(54);
    infoicon.set_halign(Align::Center);
    infoicon.set_css_classes(&["card-icons"]);

    let title = Label::new(Some("Sound and Audio Settings"));
    title.add_css_class("shortcut-title"); 
    title.set_halign(Align::Center);

    let subtitle = Label::new(Some("Configure your audio devices."));
    subtitle.add_css_class("shortcut-desc");
    subtitle.set_halign(Align::Center);

    card.append(&infoicon);
    card.append(&title);
    card.append(&subtitle);
    content.append(&card);

    let toast = Toast::new();

    // Output tab: device card + combined output card
    let output_page = GtkBox::new(Orientation::Vertical, 16);
    let (output_card, refresh_output) = build_device_card(Kind::Output, &toast);
    let combined_card = build_combined_card(&toast, refresh_output.clone());
    output_page.append(&output_card);
    output_page.append(&combined_card);

    // Input tab
    let input_page = GtkBox::new(Orientation::Vertical, 16);
    let (input_card, _refresh_input) = build_device_card(Kind::Input, &toast);
    input_page.append(&input_card);

    let tabs = Stack::new();
    tabs.set_transition_type(StackTransitionType::SlideLeftRight);
    tabs.set_hexpand(true);
    tabs.add_titled(&output_page, Some("output"), "Output");
    tabs.add_titled(&input_page, Some("input"), "Input");

    let switcher = StackSwitcher::builder()
        .stack(&tabs)
        .halign(Align::Start)
        .build();

    content.append(&switcher);
    content.append(&tabs);

    let overlay = Overlay::new();
    let scroller: ScrolledWindow = page_scroller(&content);
    overlay.set_child(Some(&scroller));
    overlay.add_overlay(&toast.revealer);
    overlay
}