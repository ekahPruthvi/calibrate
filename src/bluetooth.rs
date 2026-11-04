use gtk4::prelude::*;
use gtk4::{
    Align, Box as GtkBox, Button, Frame, Image, Label, ListBox, ListBoxRow, Orientation, Overlay,
    Revealer, RevealerTransitionType, ScrolledWindow, Switch, Window, gio, glib,
};
use std::cell::{Cell, RefCell};
use std::process::Command;
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

// ───────────────────────────── bluetoothctl backend ─────────────────────────────

#[derive(Clone)]
struct BtDevice {
    mac: String,
    name: String,
    connected: bool,
    trusted: bool,
    icon: String,
    battery: Option<String>,
}

#[derive(Clone)]
struct Nearby {
    mac: String,
    name: String,
}

/// Removes ANSI colour codes and readline markers that bluetoothctl may print.
fn strip_ansi(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut chars = s.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '\x1b' => {
                if chars.peek() == Some(&'[') {
                    chars.next();
                    for n in chars.by_ref() {
                        if n.is_ascii_alphabetic() {
                            break;
                        }
                    }
                }
            }
            '\x01' | '\x02' | '\r' => {}
            _ => out.push(c),
        }
    }
    out
}

fn btctl(args: &[&str]) -> Result<String, String> {
    let out = Command::new("bluetoothctl")
        .env("LC_ALL", "C")
        .args(args)
        .output()
        .map_err(|e| format!("cannot run bluetoothctl: {e}"))?;
    let stdout = strip_ansi(&String::from_utf8_lossy(&out.stdout));
    if out.status.success() {
        Ok(stdout)
    } else {
        let err = strip_ansi(&String::from_utf8_lossy(&out.stderr)).trim().to_string();
        Err(if err.is_empty() { stdout.trim().to_string() } else { err })
    }
}

/// bluetoothctl often exits 0 even when the action failed, so look at the output too.
fn btctl_action(args: &[&str]) -> Result<String, String> {
    let out = btctl(args)?;
    if let Some(line) = out
        .lines()
        .find(|l| l.contains("Failed") || l.contains("not available") || l.contains("No default controller"))
    {
        return Err(line.trim().to_string());
    }
    Ok(out)
}

/// `Device AA:BB:CC:DD:EE:FF Some Name` lines -> (mac, name)
fn parse_devices(out: &str) -> Vec<(String, String)> {
    out.lines()
        .filter_map(|l| {
            let l = l.trim();
            let rest = l.strip_prefix("Device ")?;
            let mut it = rest.splitn(2, ' ');
            let mac = it.next()?.to_string();
            let name = it.next().unwrap_or("").trim().to_string();
            Some((mac, name))
        })
        .collect()
}

fn info_field(info: &str, key: &str) -> Option<String> {
    info.lines().find_map(|l| {
        l.trim()
            .strip_prefix(key)
            .and_then(|r| r.strip_prefix(':'))
            .map(|v| v.trim().to_string())
    })
}

fn yes(v: Option<String>) -> bool {
    v.map(|s| s == "yes").unwrap_or(false)
}

/// Err = no usable adapter, Ok(powered)
fn adapter_powered() -> Result<bool, String> {
    let out = btctl(&["show"])?;
    if out.contains("No default controller") || out.trim().is_empty() {
        return Err("No Bluetooth adapter found".to_string());
    }
    Ok(yes(info_field(&out, "Powered")))
}

fn set_powered(on: bool) -> Result<(), String> {
    btctl_action(&["power", if on { "on" } else { "off" }]).map(|_| ())
}

fn paired_list() -> Vec<(String, String)> {
    // newer bluez: `devices Paired`, older: `paired-devices`
    if let Ok(out) = btctl(&["devices", "Paired"]) {
        let v = parse_devices(&out);
        if !v.is_empty() {
            return v;
        }
    }
    btctl(&["paired-devices"]).map(|o| parse_devices(&o)).unwrap_or_default()
}

fn paired_devices() -> Vec<BtDevice> {
    let mut devs: Vec<BtDevice> = paired_list()
        .into_iter()
        .map(|(mac, fallback)| {
            let info = btctl(&["info", &mac]).unwrap_or_default();
            let name = info_field(&info, "Alias")
                .or_else(|| info_field(&info, "Name"))
                .filter(|n| !n.is_empty())
                .unwrap_or(fallback);
            // "Battery Percentage: 0x50 (80)"
            let battery = info_field(&info, "Battery Percentage").and_then(|b| {
                let start = b.find('(')? + 1;
                let end = b.find(')')?;
                Some(format!("{}%", &b[start..end]))
            });
            BtDevice {
                connected: yes(info_field(&info, "Connected")),
                trusted: yes(info_field(&info, "Trusted")),
                icon: info_field(&info, "Icon").unwrap_or_default(),
                battery,
                name,
                mac,
            }
        })
        .collect();
    devs.sort_by(|a, b| {
        b.connected
            .cmp(&a.connected)
            .then(a.name.to_lowercase().cmp(&b.name.to_lowercase()))
    });
    devs
}

/// Scans for a few seconds and returns devices that are not paired yet.
fn scan_nearby() -> Result<Vec<Nearby>, String> {
    let scan = btctl(&["--timeout", "8", "scan", "on"])?;
    if scan.contains("Failed to start discovery") || scan.contains("No default controller") {
        return Err("Turn Bluetooth on first".to_string());
    }
    let paired: Vec<String> = paired_list().into_iter().map(|(m, _)| m).collect();
    let all = btctl(&["devices"]).map(|o| parse_devices(&o)).unwrap_or_default();
    Ok(all
        .into_iter()
        .filter(|(mac, name)| {
            !paired.contains(mac)
                && !name.is_empty()
                // unnamed devices are listed with their address as the name
                && name.replace('-', ":") != *mac
        })
        .map(|(mac, name)| Nearby { mac, name })
        .collect())
}

fn type_icon(icon: &str) -> &'static str {
    match icon {
        "audio-headset" => "audio-headset-symbolic",
        "audio-headphones" => "audio-headphones-symbolic",
        "audio-card" => "audio-speakers-symbolic",
        "input-keyboard" => "input-keyboard-symbolic",
        "input-mouse" => "input-mouse-symbolic",
        "input-gaming" => "input-gaming-symbolic",
        "input-tablet" => "input-tablet-symbolic",
        "phone" => "phone-symbolic",
        "computer" => "computer-symbolic",
        _ => "bluetooth-symbolic",
    }
}

fn type_name(icon: &str) -> String {
    match icon {
        "audio-headset" => "Headset",
        "audio-headphones" => "Headphones",
        "audio-card" => "Speaker",
        "input-keyboard" => "Keyboard",
        "input-mouse" => "Mouse",
        "input-gaming" => "Game controller",
        "input-tablet" => "Tablet",
        "phone" => "Phone",
        "computer" => "Computer",
        _ => "Other",
    }
    .to_string()
}

// ───────────────────────────── device dialog ─────────────────────────────

fn section(title: &str, subtitle: &str) -> (Frame, GtkBox) {
    let frame = Frame::new(None);
    let inner = GtkBox::new(Orientation::Vertical, 8);
    inner.set_margin_top(16);
    inner.set_margin_bottom(16);
    inner.set_margin_start(20);
    inner.set_margin_end(20);

    let t = Label::new(Some(title));
    t.add_css_class("frame-title");
    t.set_halign(Align::Start);

    let s = Label::new(Some(subtitle));
    s.add_css_class("frame-subtitle");
    s.set_halign(Align::Start);
    s.set_xalign(0.0);
    s.set_wrap(true);

    inner.append(&t);
    inner.append(&s);
    frame.set_child(Some(&inner));
    (frame, inner)
}

fn info_row(label: &str, value: &str) -> GtkBox {
    let row = GtkBox::new(Orientation::Horizontal, 12);
    let l = Label::new(Some(label));
    l.add_css_class("row-label");
    l.set_halign(Align::Start);
    l.set_xalign(0.0);
    l.set_width_chars(10);

    let v = Label::new(Some(value));
    v.add_css_class("row-caption");
    v.set_halign(Align::Start);
    v.set_xalign(0.0);
    v.set_hexpand(true);
    v.set_wrap(true);
    v.set_selectable(true);

    row.append(&l);
    row.append(&v);
    row
}

fn open_device(parent: Option<Window>, dev: BtDevice, toast: Toast, on_changed: Rc<dyn Fn()>) {
    let win = Window::builder()
        .title(format!("{} — Bluetooth device", dev.name))
        .modal(true)
        .default_width(440)
        .default_height(480)
        .build();
    if let Some(p) = &parent {
        win.set_transient_for(Some(p));
    }

    let content = GtkBox::new(Orientation::Vertical, 16);
    content.add_css_class("settings-page");
    content.set_margin_top(16);
    content.set_margin_bottom(16);
    content.set_margin_start(16);
    content.set_margin_end(16);

    let heading = Label::new(Some(&dev.name));
    heading.add_css_class("shortcut-title");
    heading.set_halign(Align::Start);
    content.append(&heading);

    // ── details ──
    let (frame, details) = section("Details", "Information reported by this device.");
    details.append(&info_row("Address", &dev.mac));
    details.append(&info_row("Type", &type_name(&dev.icon)));
    details.append(&info_row("Status", if dev.connected { "Connected" } else { "Not connected" }));
    details.append(&info_row("Trusted", if dev.trusted { "Yes" } else { "No" }));
    details.append(&info_row("Battery", dev.battery.as_deref().unwrap_or("—")));
    content.append(&frame);

    // ── forget ──
    let forget = Button::with_label("Forget Device");
    forget.add_css_class("destructive-action");
    forget.set_halign(Align::End);
    forget.set_cursor_from_name(Some("pointer"));
    content.append(&forget);

    {
        let armed = Rc::new(Cell::new(false));
        let mac = dev.mac.clone();
        let name = dev.name.clone();
        let win = win.clone();
        forget.connect_clicked(move |btn| {
            if !armed.get() {
                armed.set(true);
                btn.set_label("Click again to confirm");
                let btn = btn.clone();
                let armed = armed.clone();
                glib::timeout_add_local_once(Duration::from_secs(4), move || {
                    armed.set(false);
                    btn.set_label("Forget Device");
                });
                return;
            }
            btn.set_sensitive(false);
            let mac = mac.clone();
            let name = name.clone();
            let win = win.clone();
            let toast = toast.clone();
            let on_changed = on_changed.clone();
            glib::spawn_future_local(async move {
                let res = gio::spawn_blocking(move || btctl_action(&["remove", &mac])).await;
                match res {
                    Ok(Ok(_)) => toast.show(&format!("Forgot {name}")),
                    Ok(Err(e)) => toast.show(&format!("Failed: {e}")),
                    Err(_) => {}
                }
                win.close();
                on_changed();
            });
        });
    }

    let scroller = ScrolledWindow::builder()
        .hscrollbar_policy(gtk4::PolicyType::Never)
        .vexpand(true)
        .hexpand(true)
        .child(&content)
        .build();
    win.set_child(Some(&scroller));
    win.present();
}

// ───────────────────────────── paired devices card ─────────────────────────────

fn build_device_row(dev: &BtDevice, toast: &Toast, trigger: Rc<dyn Fn()>) -> ListBoxRow {
    let row = GtkBox::new(Orientation::Horizontal, 10);
    row.set_margin_top(6);

    let icon = Image::from_icon_name(type_icon(&dev.icon));
    icon.set_pixel_size(20);
    icon.set_valign(Align::Center);
    icon.add_css_class("card-icons-smol");
    row.append(&icon);

    let text = GtkBox::new(Orientation::Vertical, 2);
    text.set_hexpand(true);
    text.set_valign(Align::Center);

    let name = Label::new(Some(&dev.name));
    name.set_halign(Align::Start);
    name.set_xalign(0.0);
    name.set_ellipsize(gtk4::pango::EllipsizeMode::End);
    text.append(&name);

    let caption = match (dev.connected, &dev.battery) {
        (true, Some(b)) => Some(format!("Connected · {b}")),
        (true, None) => Some("Connected".to_string()),
        (false, _) => None,
    };
    if let Some(c) = caption {
        let cap = Label::new(Some(&c));
        cap.add_css_class("row-caption");
        cap.set_halign(Align::Start);
        cap.set_xalign(0.0);
        text.append(&cap);
    }
    row.append(&text);

    let connect = Button::with_label(if dev.connected { "Disconnect" } else { "Connect" });
    connect.add_css_class(if dev.connected { "destructive-action" } else { "sub-btn" });
    connect.add_css_class("sub-btn-small");
    connect.set_valign(Align::Center);
    connect.set_cursor_from_name(Some("pointer"));

    let settings = Button::new();
    settings.set_child(Some(
        &Image::builder()
            .file("/var/lib/cynager/icons/cog.svg")
            .pixel_size(22)
            .build(),
    ));
    settings.add_css_class("flat");
    settings.add_css_class("ghost-gear");

    row.append(&connect);
    row.append(&settings);

    {
        let dev = dev.clone();
        let toast = toast.clone();
        let trigger = trigger.clone();
        connect.connect_clicked(move |btn| {
            btn.set_sensitive(false);
            btn.set_label(if dev.connected { "Disconnecting…" } else { "Connecting…" });
            let dev = dev.clone();
            let toast = toast.clone();
            let trigger = trigger.clone();
            glib::spawn_future_local(async move {
                let d = dev.clone();
                let res = gio::spawn_blocking(move || {
                    let verb = if d.connected { "disconnect" } else { "connect" };
                    btctl_action(&[verb, &d.mac])
                })
                .await;
                match res {
                    Ok(Ok(_)) => toast.show(&if dev.connected {
                        format!("Disconnected from {}", dev.name)
                    } else {
                        format!("Connected to {}", dev.name)
                    }),
                    Ok(Err(e)) => toast.show(&format!("Failed: {e}")),
                    Err(_) => {}
                }
                trigger();
            });
        });
    }

    {
        let dev = dev.clone();
        let toast = toast.clone();
        settings.connect_clicked(move |btn| {
            let parent = btn.root().and_then(|r| r.downcast::<Window>().ok());
            open_device(parent, dev.clone(), toast.clone(), trigger.clone());
        });
    }

    ListBoxRow::builder().child(&row).activatable(false).selectable(false).build()
}

/// Header card with the Bluetooth switch. The switch is wired up in `build_bt_card`,
/// which owns the refresh logic.
fn build_bt_header() -> (Frame, Switch) {
    let frame = Frame::new(None);
    let outer = GtkBox::new(Orientation::Vertical, 5);

    let header = GtkBox::new(Orientation::Horizontal, 12);
    header.set_margin_start(20);
    header.set_margin_end(20);
    header.set_margin_top(20);

    let text_box = GtkBox::new(Orientation::Vertical, 2);
    text_box.set_hexpand(true);
    let title = Label::new(Some("Bluetooth"));
    title.add_css_class("frame-title");
    title.set_halign(Align::Start);
    let sub = Label::new(Some("Manage paired devices and Bluetooth visibility."));
    sub.set_margin_bottom(20);
    sub.add_css_class("frame-subtitle");
    sub.set_halign(Align::Start);
    text_box.append(&title);
    text_box.append(&sub);

    let switch = Switch::new();
    switch.set_valign(Align::Center);
    switch.set_cursor_from_name(Some("pointer"));

    header.append(&text_box);
    header.append(&switch);


    outer.append(&header);
    frame.set_child(Some(&outer));

    (frame, switch)
}

/// Returns the card and a function that reloads its device list.
fn build_bt_card(toast: &Toast, switch: &Switch) -> (Frame, Rc<dyn Fn()>) {
    let frame = Frame::new(None);
    let outer = GtkBox::new(Orientation::Vertical, 5);

    let hint = Label::new(Some("Bluetooth is turned off."));
    hint.add_css_class("row-caption");
    hint.set_halign(Align::Center);
    hint.set_margin_top(20);
    hint.set_margin_start(20);
    hint.set_visible(false);

    let list = ListBox::new();
    list.set_selection_mode(gtk4::SelectionMode::None);
    list.add_css_class("sub-list");
    list.set_margin_start(15);
    list.set_margin_end(15);
    list.set_margin_bottom(20);

    let listscroller = ScrolledWindow::builder()
        .hscrollbar_policy(gtk4::PolicyType::Never)
        .vexpand(true)
        .hexpand(true)
        .max_content_height(300)
        .min_content_height(80)
        .child(&list)
        .build();

    outer.append(&hint);
    outer.append(&listscroller);
    frame.set_child(Some(&outer));

    let updating = Rc::new(Cell::new(false));

    // refresh <-> rows need each other, so go through a shared slot
    let slot: Rc<RefCell<Option<Rc<dyn Fn()>>>> = Rc::new(RefCell::new(None));
    let trigger: Rc<dyn Fn()> = {
        let slot = slot.clone();
        Rc::new(move || {
            let f = slot.borrow().clone();
            if let Some(f) = f {
                f();
            }
        })
    };

    let refresh: Rc<dyn Fn()> = {
        let list = list.clone();
        let hint = hint.clone();
        let updating = updating.clone();
        let toast = toast.clone();
        let trigger = trigger.clone();
        let switch = switch.downgrade(); // weak: the switch handler holds `refresh`
        Rc::new(move || {
            let list = list.clone();
            let hint = hint.clone();
            let switch = switch.clone();
            let updating = updating.clone();
            let toast = toast.clone();
            let trigger = trigger.clone();
            glib::spawn_future_local(async move {
                let Ok((state, devs)) = gio::spawn_blocking(|| {
                    let state = adapter_powered();
                    let devs = if matches!(state, Ok(true)) { paired_devices() } else { Vec::new() };
                    (state, devs)
                })
                .await
                else {
                    return;
                };

                let (enabled, _) = match &state {
                    Ok(p) => (*p, true),
                    Err(_) => (false, false),
                };

                // reflect the real adapter state without re-triggering the handler
                if let Some(sw) = switch.upgrade() {
                    updating.set(true);
                    sw.set_active(enabled);
                    updating.set(false);
                    sw.set_sensitive(state.is_ok()); // no adapter -> nothing to toggle
                }

                hint.set_text(match &state {
                    Err(e) => e.as_str(),
                    Ok(_) => "Bluetooth is turned off.",
                });
                hint.set_visible(!enabled);
                list.set_sensitive(enabled);

                while let Some(r) = list.first_child() {
                    list.remove(&r);
                }

                if enabled && devs.is_empty() {
                    let empty = Label::new(Some("No paired devices"));
                    empty.set_vexpand(true);
                    empty.set_valign(Align::Center);
                    empty.add_css_class("row-caption");
                    empty.set_margin_top(10);
                    empty.set_margin_bottom(10);
                    list.append(&ListBoxRow::builder().child(&empty).activatable(false).selectable(false).build());
                }

                for d in &devs {
                    list.append(&build_device_row(d, &toast, trigger.clone()));
                }
            });
        })
    };
    *slot.borrow_mut() = Some(refresh.clone());

    // toggle adapter power
    {
        let toast = toast.clone();
        let updating = updating.clone();
        let refresh = refresh.clone();
        switch.connect_active_notify(move |s| {
            if updating.get() {
                return; // programmatic change from refresh()
            }
            let on = s.is_active();
            s.set_sensitive(false); // no overlapping toggles while bluetoothctl runs
            let s = s.clone();
            let toast = toast.clone();
            let refresh = refresh.clone();
            glib::spawn_future_local(async move {
                match gio::spawn_blocking(move || set_powered(on)).await {
                    Ok(Ok(())) => toast.show(if on { "Bluetooth turned on" } else { "Bluetooth turned off" }),
                    Ok(Err(e)) => toast.show(&format!("Failed: {e}")),
                    Err(_) => {}
                }
                s.set_sensitive(true);
                // re-read the real state (also reverts the switch if the command failed)
                refresh();
                if on {
                    // give known devices a moment to auto-reconnect, then update again
                    let refresh = refresh.clone();
                    glib::timeout_add_local_once(Duration::from_secs(2), move || refresh());
                }
            });
        });
    }

    {
        let refresh = refresh.clone();
        frame.connect_map(move |_| refresh());
    }

    (frame, refresh)
}

// ───────────────────────────── add devices card ─────────────────────────────

fn build_nearby_row(dev: &Nearby, toast: &Toast, on_paired: Rc<dyn Fn()>) -> ListBoxRow {
    let row = GtkBox::new(Orientation::Horizontal, 8);
    row.set_margin_top(6);

    let name = Label::new(Some(&dev.name));
    name.set_halign(Align::Start);
    name.set_xalign(0.0);
    name.set_hexpand(true);
    name.set_ellipsize(gtk4::pango::EllipsizeMode::End);
    row.append(&name);

    let pair = Button::with_label("Pair");
    pair.add_css_class("sub-btn");
    pair.add_css_class("sub-btn-small");
    pair.set_valign(Align::Center);
    pair.set_cursor_from_name(Some("pointer"));
    row.append(&pair);

    {
        let dev = dev.clone();
        let toast = toast.clone();
        pair.connect_clicked(move |btn| {
            btn.set_sensitive(false);
            btn.set_label("Pairing…");
            let btn = btn.clone();
            let dev = dev.clone();
            let toast = toast.clone();
            let on_paired = on_paired.clone();
            glib::spawn_future_local(async move {
                let mac = dev.mac.clone();
                let res = gio::spawn_blocking(move || -> Result<(), String> {
                    btctl_action(&["--timeout", "40", "pair", &mac])?;
                    // best effort: trust so it reconnects on its own, then connect
                    let _ = btctl(&["trust", &mac]);
                    let _ = btctl(&["connect", &mac]);
                    Ok(())
                })
                .await;
                match res {
                    Ok(Ok(())) => {
                        toast.show(&format!("Paired with {}", dev.name));
                        on_paired();
                    }
                    Ok(Err(e)) => {
                        toast.show(&format!("Failed: {e}"));
                        btn.set_sensitive(true);
                        btn.set_label("Pair");
                    }
                    Err(_) => {}
                }
            });
        });
    }

    ListBoxRow::builder().child(&row).activatable(false).selectable(false).build()
}

fn build_add_device_card(toast: &Toast, on_paired: Rc<dyn Fn()>) -> Frame {
    let frame = Frame::new(None);
    let inner = GtkBox::new(Orientation::Vertical, 8);
    inner.set_margin_top(16);
    inner.set_margin_bottom(16);
    inner.set_margin_start(20);
    inner.set_margin_end(20);
    frame.set_child(Some(&inner));

    // title + subtitle on the left, Scan button on the right end of the same row
    let head = GtkBox::new(Orientation::Horizontal, 12);

    let text_box = GtkBox::new(Orientation::Vertical, 2);
    text_box.set_hexpand(true);
    text_box.set_valign(Align::Center);

    let t = Label::new(Some("Add devices"));
    t.add_css_class("frame-title");
    t.set_halign(Align::Start);

    let sub = Label::new(Some(
        "Put your device in pairing mode, then press Scan. Devices that are not paired yet \
         will show up below.",
    ));
    sub.add_css_class("frame-subtitle");
    sub.set_halign(Align::Start);
    sub.set_xalign(0.0);
    sub.set_wrap(true);

    text_box.append(&t);
    text_box.append(&sub);
    head.append(&text_box);

    let status = Label::new(None);
    status.add_css_class("row-caption");
    status.set_halign(Align::Start);
    status.set_xalign(0.0);
    status.set_wrap(true);
    status.set_visible(false);

    let scan = Button::with_label("Scan");
    scan.add_css_class("sub-btn");
    scan.set_halign(Align::End);
    scan.set_valign(Align::Center);
    scan.set_cursor_from_name(Some("pointer"));
    head.append(&scan);
    inner.append(&head);

    let list = ListBox::new();
    list.set_selection_mode(gtk4::SelectionMode::None);
    list.add_css_class("sub-list");

    inner.append(&status);
    inner.append(&list);

    {
        let status = status.clone();
        let list = list.clone();
        let toast = toast.clone();
        scan.connect_clicked(move |btn| {
            btn.set_sensitive(false);
            btn.set_label("Scanning…");
            status.set_text("Searching for nearby devices…");
            status.set_visible(true);

            let btn = btn.clone();
            let status = status.clone();
            let list = list.clone();
            let toast = toast.clone();
            let on_paired = on_paired.clone();
            glib::spawn_future_local(async move {
                let res = gio::spawn_blocking(scan_nearby).await;
                btn.set_sensitive(true);
                btn.set_label("Scan again");

                while let Some(r) = list.first_child() {
                    list.remove(&r);
                }

                match res {
                    Ok(Ok(devs)) => {
                        if devs.is_empty() {
                            status.set_text("No new devices found");
                        } else {
                            status.set_visible(false);
                        }
                        for d in &devs {
                            list.append(&build_nearby_row(d, &toast, on_paired.clone()));
                        }
                    }
                    Ok(Err(e)) => status.set_text(&format!("Scan failed: {e}")),
                    Err(_) => status.set_text("Scan failed"),
                }
            });
        });
    }

    frame
}

// ───────────────────────────── page ─────────────────────────────

pub fn build_bt_page() -> Overlay {
    let content = GtkBox::new(Orientation::Vertical, 16);

    // header card
    let card = GtkBox::new(Orientation::Vertical, 10);
    card.add_css_class("shortcut-card");
    card.add_css_class("detail-card");
    card.set_margin_top(10);
    card.set_hexpand(true);

    let infoicon = Image::from_icon_name("bluetooth-symbolic");
    infoicon.set_pixel_size(54);
    infoicon.set_halign(Align::Center);
    infoicon.set_css_classes(&["card-icons"]);

    let title = Label::new(Some("Bluetooth Settings"));
    title.add_css_class("shortcut-title");
    title.set_halign(Align::Center);

    let subtitle = Label::new(Some("Connect, pair and manage your devices."));
    subtitle.add_css_class("shortcut-desc");
    subtitle.set_halign(Align::Center);

    card.append(&infoicon);
    card.append(&title);
    card.append(&subtitle);
    content.append(&card);

    let toast = Toast::new();

    let (bt_head, bt_switch) = build_bt_header();
    content.append(&bt_head);
    // paired devices card
    let paired_title = Label::new(Some("Paired Devices"));
    paired_title.add_css_class("frame-title");
    paired_title.set_halign(Align::Start);
    paired_title.set_margin_start(20);
    paired_title.set_margin_top(20);
    content.append(&paired_title);

    let (bt_card, refresh) = build_bt_card(&toast, &bt_switch);
    content.append(&bt_card);

    // add devices card
    content.append(&build_add_device_card(&toast, refresh));

    let overlay = Overlay::new();
    let scroller: ScrolledWindow = page_scroller(&content);
    overlay.set_child(Some(&scroller));
    overlay.add_overlay(&toast.revealer);
    overlay
}