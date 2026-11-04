use gtk4::prelude::*;
use gtk4::{
    Align, Box as GtkBox, Button, Entry, Frame, Image, Label, ListBox, ListBoxRow, Orientation,
    Overlay, Revealer, RevealerTransitionType, ScrolledWindow, Switch, Window, gio, glib,
};
use std::cell::{Cell, RefCell};
use std::net::IpAddr;
use std::process::Command;
use std::rc::Rc;
use std::time::Duration;

use crate::home::page_scroller;

const HIDDEN_PW: &str = "••••••••";

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

// ───────────────────────────── nmcli backend ─────────────────────────────

#[derive(Clone)]
struct KnownNet {
    name: String,
    uuid: String,
    active: bool,
}

fn nmcli(args: &[&str]) -> Result<String, String> {
    let out = Command::new("nmcli")
        .env("LC_ALL", "C")
        .args(args)
        .output()
        .map_err(|e| format!("cannot run nmcli: {e}"))?;
    if out.status.success() {
        Ok(String::from_utf8_lossy(&out.stdout).to_string())
    } else {
        let err = String::from_utf8_lossy(&out.stderr).trim().to_string();
        if err.is_empty() {
            Err(String::from_utf8_lossy(&out.stdout).trim().to_string())
        } else {
            Err(err)
        }
    }
}

/// Splits one line of `nmcli -t` output on unescaped colons.
fn split_terse(line: &str) -> Vec<String> {
    let mut parts = vec![String::new()];
    let mut chars = line.chars();
    while let Some(c) = chars.next() {
        match c {
            '\\' => {
                if let Some(n) = chars.next() {
                    parts.last_mut().unwrap().push(n);
                }
            }
            ':' => parts.push(String::new()),
            _ => parts.last_mut().unwrap().push(c),
        }
    }
    parts
}

fn wifi_enabled() -> bool {
    nmcli(&["radio", "wifi"]).map(|s| s.trim() == "enabled").unwrap_or(false)
}

fn set_wifi_enabled(on: bool) -> Result<(), String> {
    nmcli(&["radio", "wifi", if on { "on" } else { "off" }]).map(|_| ())
}

fn known_networks() -> Vec<KnownNet> {
    let out = nmcli(&["-t", "-f", "NAME,UUID,TYPE,DEVICE", "connection", "show"]).unwrap_or_default();
    let mut nets = Vec::new();
    for line in out.lines() {
        let f = split_terse(line);
        if f.len() < 4 {
            continue;
        }
        if f[2] == "802-11-wireless" || f[2] == "wifi" {
            nets.push(KnownNet {
                name: f[0].clone(),
                uuid: f[1].clone(),
                active: !f[3].is_empty(),
            });
        }
    }
    // connected first, then alphabetical
    nets.sort_by(|a, b| b.active.cmp(&a.active).then(a.name.to_lowercase().cmp(&b.name.to_lowercase())));
    nets
}

/// All values of one nmcli field for a connection (multi-valued fields are split up).
fn nm_values(uuid: &str, field: &str) -> Vec<String> {
    nmcli(&["-g", field, "connection", "show", uuid])
        .unwrap_or_default()
        .lines()
        .flat_map(|l| l.split(" | "))
        .flat_map(|l| l.split(','))
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty() && s != "--")
        .collect()
}

struct IpInfo {
    /// [method, address, gateway, dns] for IPv4 and IPv6
    v4: [String; 4],
    v6: [String; 4],
    dns_config: String,
}

fn or_dash(v: Vec<String>) -> String {
    if v.is_empty() { "—".to_string() } else { v.join(", ") }
}

fn family_info(uuid: &str, fam: &str, up: &str) -> [String; 4] {
    let pick = |runtime: &str, config: &str| {
        let r = nm_values(uuid, runtime);
        if r.is_empty() { nm_values(uuid, config) } else { r }
    };
    [
        or_dash(nm_values(uuid, &format!("{fam}.method"))),
        or_dash(pick(&format!("{up}.ADDRESS"), &format!("{fam}.addresses"))),
        or_dash(pick(&format!("{up}.GATEWAY"), &format!("{fam}.gateway"))),
        or_dash(pick(&format!("{up}.DNS"), &format!("{fam}.dns"))),
    ]
}

fn ip_info(uuid: &str) -> IpInfo {
    let mut dns_cfg = nm_values(uuid, "ipv4.dns");
    dns_cfg.extend(nm_values(uuid, "ipv6.dns"));
    IpInfo {
        v4: family_info(uuid, "ipv4", "IP4"),
        v6: family_info(uuid, "ipv6", "IP6"),
        dns_config: dns_cfg.join(", "),
    }
}

// ───────────────────────────── settings dialog ─────────────────────────────

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

fn info_row(label: &str) -> (GtkBox, Label) {
    let row = GtkBox::new(Orientation::Horizontal, 12);
    let l = Label::new(Some(label));
    l.add_css_class("row-label");
    l.set_halign(Align::Start);
    l.set_xalign(0.0);
    l.set_width_chars(10);

    let v = Label::new(Some("…"));
    v.add_css_class("row-caption");
    v.set_halign(Align::Start);
    v.set_xalign(0.0);
    v.set_hexpand(true);
    v.set_wrap(true);
    v.set_selectable(true);

    row.append(&l);
    row.append(&v);
    (row, v)
}

fn open_settings(parent: Option<Window>, net: KnownNet, toast: Toast, on_changed: Rc<dyn Fn()>) {
    let win = Window::builder()
        .title(format!("{} — Network settings", net.name))
        .modal(true)
        .default_width(480)
        .default_height(680)
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

    let heading = Label::new(Some(&net.name));
    heading.add_css_class("shortcut-title");
    heading.set_halign(Align::Start);
    content.append(&heading);

    // ── saved password ──
    let (pw_frame, pw_box) = section("Saved password", "Stored in this network's connection profile.");
    let pw_row = GtkBox::new(Orientation::Horizontal, 8);
    let pw_val = Label::new(Some(HIDDEN_PW));
    pw_val.add_css_class("row-caption");
    pw_val.set_halign(Align::Start);
    pw_val.set_xalign(0.0);
    pw_val.set_hexpand(true);
    pw_val.set_wrap(true);
    let pw_btn = Button::with_label("Show");
    pw_btn.add_css_class("sub-btn");
    pw_btn.add_css_class("sub-btn-small");
    pw_btn.set_cursor_from_name(Some("pointer"));
    pw_row.append(&pw_val);
    pw_row.append(&pw_btn);
    pw_box.append(&pw_row);
    content.append(&pw_frame);

    {
        let shown = Rc::new(Cell::new(false));
        let uuid = net.uuid.clone();
        let pw_val = pw_val.clone();
        pw_btn.connect_clicked(move |btn| {
            if shown.get() {
                pw_val.set_text(HIDDEN_PW);
                pw_val.set_selectable(false);
                btn.set_label("Show");
                shown.set(false);
                return;
            }
            let uuid = uuid.clone();
            let pw_val = pw_val.clone();
            let btn = btn.clone();
            let shown = shown.clone();
            glib::spawn_future_local(async move {
                let res = gio::spawn_blocking(move || {
                    nmcli(&["-s", "-g", "802-11-wireless-security.psk", "connection", "show", &uuid])
                })
                .await;
                match res {
                    Ok(Ok(s)) => {
                        let s = s.trim().to_string();
                        if s.is_empty() {
                            pw_val.set_text("No password stored (open network or kept in the keyring)");
                        } else {
                            pw_val.set_text(&s);
                            pw_val.set_selectable(true);
                        }
                        btn.set_label("Hide");
                        shown.set(true);
                    }
                    Ok(Err(e)) => pw_val.set_text(&format!("Unavailable: {e}")),
                    Err(_) => {}
                }
            });
        });
    }

    // ── DNS ──
    let (dns_frame, dns_box) = section(
        "DNS",
        "Comma-separated IPv4 / IPv6 servers. Leave empty to use the ones provided automatically.",
    );
    let dns_entry = Entry::new();
    dns_entry.set_placeholder_text(Some("e.g. 1.1.1.1, 8.8.8.8, 2606:4700:4700::1111"));
    dns_entry.set_hexpand(true);

    let dns_status = Label::new(None);
    dns_status.add_css_class("row-caption");
    dns_status.set_halign(Align::Start);
    dns_status.set_xalign(0.0);
    dns_status.set_wrap(true);
    dns_status.set_visible(false);

    let dns_apply = Button::with_label("Apply");
    dns_apply.add_css_class("sub-btn");
    dns_apply.add_css_class("sub-btn-small");
    dns_apply.set_halign(Align::End);
    dns_apply.set_cursor_from_name(Some("pointer"));

    dns_box.append(&dns_entry);
    dns_box.append(&dns_status);
    dns_box.append(&dns_apply);
    content.append(&dns_frame);

    {
        let uuid = net.uuid.clone();
        let active = net.active;
        let dns_entry = dns_entry.clone();
        let dns_status = dns_status.clone();
        dns_apply.connect_clicked(move |btn| {
            let text = dns_entry.text().to_string();
            let servers: Vec<String> = text
                .split(|c: char| c == ',' || c.is_whitespace())
                .filter(|s| !s.is_empty())
                .map(String::from)
                .collect();
            if let Some(bad) = servers.iter().find(|s| s.parse::<IpAddr>().is_err()) {
                dns_status.set_text(&format!("Invalid address: {bad}"));
                dns_status.set_visible(true);
                return;
            }
            let (v6, v4): (Vec<String>, Vec<String>) = servers.into_iter().partition(|s| s.contains(':'));

            btn.set_sensitive(false);
            dns_status.set_text("Applying…");
            dns_status.set_visible(true);

            let uuid = uuid.clone();
            let btn = btn.clone();
            let dns_status = dns_status.clone();
            glib::spawn_future_local(async move {
                let res = gio::spawn_blocking(move || -> Result<(), String> {
                    let args: Vec<String> = vec![
                        "connection".into(),
                        "modify".into(),
                        uuid.clone(),
                        "ipv4.dns".into(),
                        v4.join(" "),
                        "ipv4.ignore-auto-dns".into(),
                        if v4.is_empty() { "no".into() } else { "yes".into() },
                        "ipv6.dns".into(),
                        v6.join(" "),
                        "ipv6.ignore-auto-dns".into(),
                        if v6.is_empty() { "no".into() } else { "yes".into() },
                    ];
                    let refs: Vec<&str> = args.iter().map(|s| s.as_str()).collect();
                    nmcli(&refs)?;
                    if active {
                        // re-activate so the new DNS takes effect right away
                        nmcli(&["connection", "up", "uuid", &uuid])?;
                    }
                    Ok(())
                })
                .await;
                btn.set_sensitive(true);
                match res {
                    Ok(Ok(())) => dns_status.set_text("DNS saved"),
                    Ok(Err(e)) => dns_status.set_text(&format!("Failed: {e}")),
                    Err(_) => dns_status.set_text("Failed"),
                }
            });
        });
    }

    // ── IPv4 / IPv6 ──
    let names = ["Method", "Address", "Gateway", "DNS"];
    let (v4_frame, v4_box) = section("IPv4", "Current values when connected, saved values otherwise.");
    let (v6_frame, v6_box) = section("IPv6", "Current values when connected, saved values otherwise.");
    let mut v4_labels = Vec::new();
    let mut v6_labels = Vec::new();
    for n in names {
        let (r4, l4) = info_row(n);
        v4_box.append(&r4);
        v4_labels.push(l4);
        let (r6, l6) = info_row(n);
        v6_box.append(&r6);
        v6_labels.push(l6);
    }
    content.append(&v4_frame);
    content.append(&v6_frame);

    {
        let uuid = net.uuid.clone();
        let dns_entry = dns_entry.clone();
        glib::spawn_future_local(async move {
            if let Ok(info) = gio::spawn_blocking(move || ip_info(&uuid)).await {
                for (l, v) in v4_labels.iter().zip(info.v4.iter()) {
                    l.set_text(v);
                }
                for (l, v) in v6_labels.iter().zip(info.v6.iter()) {
                    l.set_text(v);
                }
                dns_entry.set_text(&info.dns_config);
            }
        });
    }

    // ── forget ──
    let forget = Button::with_label("Forget Network");
    forget.add_css_class("destructive-action");
    forget.set_halign(Align::End);
    forget.set_cursor_from_name(Some("pointer"));
    content.append(&forget);

    {
        let armed = Rc::new(Cell::new(false));
        let uuid = net.uuid.clone();
        let name = net.name.clone();
        let win = win.clone();
        forget.connect_clicked(move |btn| {
            if !armed.get() {
                armed.set(true);
                btn.set_label("Click again to confirm");
                let btn = btn.clone();
                let armed = armed.clone();
                glib::timeout_add_local_once(Duration::from_secs(4), move || {
                    armed.set(false);
                    btn.set_label("Forget Network");
                });
                return;
            }
            btn.set_sensitive(false);
            let uuid = uuid.clone();
            let name = name.clone();
            let win = win.clone();
            let toast = toast.clone();
            let on_changed = on_changed.clone();
            glib::spawn_future_local(async move {
                let res = gio::spawn_blocking(move || nmcli(&["connection", "delete", "uuid", &uuid])).await;
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

// ───────────────────────────── wifi card ─────────────────────────────

fn build_net_row(net: &KnownNet, toast: &Toast, trigger: Rc<dyn Fn()>) -> ListBoxRow {
    let row = GtkBox::new(Orientation::Horizontal, 8);
    row.set_margin_top(6);

    let name = Label::new(Some(&net.name));
    name.set_halign(Align::Start);
    name.set_xalign(0.0);
    name.set_hexpand(true);
    name.set_ellipsize(gtk4::pango::EllipsizeMode::End);

    row.append(&name);

    let connect = Button::with_label(if net.active { "Disconnect" } else { "Connect" });
    connect.set_css_classes(if net.active { &["destructive-action", "sub-btn-small"] } else { &["sub-btn", "sub-btn-small"] });
    connect.set_valign(Align::Center);
    connect.set_cursor_from_name(Some("pointer"));

    let settings = Button::new();
    settings.set_child(Some(
        &Image::builder()
            .file("/var/lib/cynager/icons/cog.svg")
            .pixel_size(22)
            .build()
    ));
    settings.add_css_class("flat");
    settings.add_css_class("ghost-gear");

    row.append(&connect);
    row.append(&settings);

    {
        let net = net.clone();
        let toast = toast.clone();
        let trigger = trigger.clone();
        connect.connect_clicked(move |btn| {
            btn.set_sensitive(false);
            btn.set_label(if net.active { "Disconnecting…" } else { "Connecting…" });
            let net = net.clone();
            let toast = toast.clone();
            let trigger = trigger.clone();
            glib::spawn_future_local(async move {
                let n = net.clone();
                let res = gio::spawn_blocking(move || {
                    let verb = if n.active { "down" } else { "up" };
                    nmcli(&["connection", verb, "uuid", &n.uuid])
                })
                .await;
                match res {
                    Ok(Ok(_)) => toast.show(&if net.active {
                        format!("Disconnected from {}", net.name)
                    } else {
                        format!("Connected to {}", net.name)
                    }),
                    Ok(Err(e)) => toast.show(&format!("Failed: {e}")),
                    Err(_) => {}
                }
                trigger();
            });
        });
    }

    {
        let net = net.clone();
        let toast = toast.clone();
        settings.connect_clicked(move |btn| {
            let parent = btn.root().and_then(|r| r.downcast::<Window>().ok());
            open_settings(parent, net.clone(), toast.clone(), trigger.clone());
        });
    }

    ListBoxRow::builder().child(&row).activatable(false).selectable(false).build()
}

/// Header card with the Wi-Fi switch. The switch is wired up in `build_wifi_card`,
/// which owns the refresh logic.
fn build_wifi_head() -> (Frame, Switch) {
    let frame = Frame::new(None);
    let outer = GtkBox::new(Orientation::Vertical, 5);

    // header: title + toggle
    let header = GtkBox::new(Orientation::Horizontal, 12);
    header.set_margin_start(20);
    header.set_margin_end(20);
    header.set_margin_top(20);

    let text_box = GtkBox::new(Orientation::Vertical, 2);
    text_box.set_hexpand(true);
    let title = Label::new(Some("Wi-Fi"));
    title.add_css_class("frame-title");
    title.set_halign(Align::Start);
    let sub = Label::new(Some("Configure and inspect wireless networking."));
    sub.add_css_class("frame-subtitle");
    sub.set_margin_bottom(20);
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

fn build_wifi_card(toast: &Toast, switch: &Switch) -> Frame {
    let frame = Frame::new(None);
    let outer = GtkBox::new(Orientation::Vertical, 5);

    let hint = Label::new(Some("Wi-Fi is turned off."));
    hint.add_css_class("row-caption");
    hint.set_halign(Align::Center);
    hint.set_margin_start(20);
    hint.set_margin_top(20);
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
        .height_request(400)
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
                let Ok((enabled, nets)) = gio::spawn_blocking(|| (wifi_enabled(), known_networks())).await
                else {
                    return;
                };

                // reflect the real radio state without re-triggering the handler
                if let Some(sw) = switch.upgrade() {
                    updating.set(true);
                    sw.set_active(enabled);
                    updating.set(false);
                }

                hint.set_visible(!enabled);
                list.set_sensitive(enabled);

                while let Some(r) = list.first_child() {
                    list.remove(&r);
                }

                if nets.is_empty() {
                    let empty = Label::new(Some("No saved Wi-Fi networks"));
                    empty.add_css_class("row-caption");
                    empty.set_margin_top(10);
                    empty.set_margin_bottom(10);
                    list.append(&ListBoxRow::builder().child(&empty).activatable(false).selectable(false).build());
                }

                for n in &nets {
                    list.append(&build_net_row(n, &toast, trigger.clone()));
                }
            });
        })
    };
    *slot.borrow_mut() = Some(refresh.clone());

    // toggle the wifi radio
    {
        let toast = toast.clone();
        let updating = updating.clone();
        let refresh = refresh.clone();
        switch.connect_active_notify(move |s| {
            if updating.get() {
                return; // programmatic change from refresh()
            }
            let on = s.is_active();
            s.set_sensitive(false); // no overlapping toggles while nmcli runs
            let s = s.clone();
            let toast = toast.clone();
            let refresh = refresh.clone();
            glib::spawn_future_local(async move {
                match gio::spawn_blocking(move || set_wifi_enabled(on)).await {
                    Ok(Ok(())) => toast.show(if on { "Wi-Fi turned on" } else { "Wi-Fi turned off" }),
                    Ok(Err(e)) => toast.show(&format!("Failed: {e}")),
                    Err(_) => {}
                }
                s.set_sensitive(true);
                // re-read the real state (also reverts the switch if nmcli failed)
                refresh();
                if on {
                    // give NetworkManager a moment to autoconnect, then update again
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

    frame
}

// ───────────────────────────── ethernet card ─────────────────────────────

#[derive(Clone)]
struct EthDev {
    device: String,
    /// raw NetworkManager state: connected, connecting (...), disconnected, unavailable, ...
    state: String,
    connection: String,
    ip: Option<String>,
}

impl EthDev {
    fn is_connected(&self) -> bool {
        self.state.starts_with("connected")
    }
    fn is_connecting(&self) -> bool {
        self.state.starts_with("connecting") || self.state.starts_with("disconnecting")
    }
    /// no cable / carrier
    fn is_unavailable(&self) -> bool {
        self.state.starts_with("unavailable")
    }
}

fn ethernet_devices() -> Vec<EthDev> {
    let out = nmcli(&["-t", "-f", "DEVICE,TYPE,STATE,CONNECTION", "device", "status"]).unwrap_or_default();
    let mut devs = Vec::new();
    for line in out.lines() {
        let f = split_terse(line);
        if f.len() < 4 || f[1] != "ethernet" {
            continue;
        }
        let mut dev = EthDev {
            device: f[0].clone(),
            state: f[2].clone(),
            connection: f[3].clone(),
            ip: None,
        };
        if dev.is_connected() {
            dev.ip = nmcli(&["-g", "IP4.ADDRESS", "device", "show", &dev.device])
                .unwrap_or_default()
                .lines()
                .flat_map(|l| l.split(" | "))
                .map(|s| s.trim().to_string())
                .find(|s| !s.is_empty() && s != "--");
        }
        devs.push(dev);
    }
    devs.sort_by(|a, b| b.is_connected().cmp(&a.is_connected()).then(a.device.cmp(&b.device)));
    devs
}

fn build_eth_row(dev: &EthDev) -> ListBoxRow {
    let row = GtkBox::new(Orientation::Horizontal, 10);
    row.set_margin_top(6);

    let icon = Image::from_icon_name("network-wired-symbolic");
    icon.set_pixel_size(20);
    icon.set_valign(Align::Center);
    icon.add_css_class("card-icons-smol");
    row.append(&icon);

    let text = GtkBox::new(Orientation::Vertical, 2);
    text.set_hexpand(true);
    text.set_valign(Align::Center);

    let name = Label::new(Some(&dev.device));
    name.set_halign(Align::Start);
    name.set_xalign(0.0);
    name.set_ellipsize(gtk4::pango::EllipsizeMode::End);
    text.append(&name);

    let caption = if dev.is_connected() {
        let mut parts = vec!["Connected".to_string()];
        if !dev.connection.is_empty() && dev.connection != "--" {
            parts.push(dev.connection.clone());
        }
        if let Some(ip) = &dev.ip {
            parts.push(ip.clone());
        }
        parts.join(" · ")
    } else if dev.is_connecting() {
        "Working…".to_string()
    } else if dev.is_unavailable() {
        "Cable unplugged".to_string()
    } else {
        "Not connected".to_string()
    };
    let cap = Label::new(Some(&caption));
    cap.add_css_class("row-caption");
    cap.set_halign(Align::Start);
    cap.set_xalign(0.0);
    text.append(&cap);
    row.append(&text);

    ListBoxRow::builder().child(&row).activatable(false).selectable(false).build()
}

/// How often the card re-reads the cable / link state while it is on screen.
const ETH_POLL_SECS: u32 = 4;

fn build_eth_card() -> Frame {
    let frame = Frame::new(None);

    let list = ListBox::new();
    list.set_selection_mode(gtk4::SelectionMode::None);
    list.add_css_class("sub-list");
    list.set_margin_top(10);
    list.set_margin_start(15);
    list.set_margin_end(15);
    list.set_margin_bottom(20);
    frame.set_child(Some(&list));

    let busy = Rc::new(Cell::new(false));
    let last: Rc<RefCell<Option<String>>> = Rc::new(RefCell::new(None));

    let refresh: Rc<dyn Fn()> = {
        let list = list.clone();
        let busy = busy.clone();
        let last = last.clone();
        Rc::new(move || {
            if busy.replace(true) {
                return; // previous read still running
            }
            let list = list.clone();
            let busy = busy.clone();
            let last = last.clone();
            glib::spawn_future_local(async move {
                let res = gio::spawn_blocking(ethernet_devices).await;
                busy.set(false);
                let Ok(devs) = res else {
                    return;
                };

                // only rebuild the rows when something actually changed
                let sig = devs
                    .iter()
                    .map(|d| format!("{}|{}|{}|{:?}", d.device, d.state, d.connection, d.ip))
                    .collect::<Vec<_>>()
                    .join("\n");
                if last.borrow().as_deref() == Some(sig.as_str()) {
                    return;
                }
                *last.borrow_mut() = Some(sig);

                while let Some(r) = list.first_child() {
                    list.remove(&r);
                }

                if devs.is_empty() {
                    let empty = Label::new(Some("No Ethernet adapter found"));
                    empty.add_css_class("row-caption");
                    empty.set_margin_top(10);
                    empty.set_margin_bottom(10);
                    list.append(&ListBoxRow::builder().child(&empty).activatable(false).selectable(false).build());
                }

                for d in &devs {
                    list.append(&build_eth_row(d));
                }
            });
        })
    };

    // refresh immediately whenever the page is shown
    {
        let refresh = refresh.clone();
        frame.connect_map(move |_| refresh());
    }

    // light poll: only does work while the card is visible, stops once it is destroyed
    {
        let weak = frame.downgrade();
        glib::timeout_add_seconds_local(ETH_POLL_SECS, move || match weak.upgrade() {
            Some(f) => {
                if f.is_mapped() {
                    refresh();
                }
                glib::ControlFlow::Continue
            }
            None => glib::ControlFlow::Break,
        });
    }

    frame
}

// ───────────────────────────── add networks card ─────────────────────────────

/// A small non-interactive "ghost" pill used in the illustration.
fn ghost_pill(icon: Option<&str>, title: Option<&str>, caption: Option<&str>) -> GtkBox {
    let pill = GtkBox::new(Orientation::Horizontal, 8);
    pill.add_css_class("ghost-pill");
    pill.set_valign(Align::Center);
    pill.set_can_target(false);

    if let Some(icon) = icon {
        let img = Image::from_icon_name(icon);
        img.set_pixel_size(18);
        pill.append(&img);
    }
    if title.is_some() || caption.is_some() {
        let col = GtkBox::new(Orientation::Vertical, 0);
        if let Some(t) = title {
            let l = Label::new(Some(t));
            l.add_css_class("ghost-pill-title");
            l.set_halign(Align::Start);
            col.append(&l);
        }
        if let Some(c) = caption {
            let l = Label::new(Some(c));
            l.add_css_class("ghost-pill-caption");
            l.set_halign(Align::Start);
            col.append(&l);
        }
        pill.append(&col);
    }
    pill
}

/// "Add networks" card: explains how to open the control panel from the
/// time capsule, with a ghost mock-up of the capsule and the panel below it.
fn build_add_network_card() -> Frame {
    let (frame, inner) = section(
        "Add networks",
        "To connect to a new Wi-Fi network, open the control panel by clicking the \
         status icons on the time capsule at the top of the screen, then choose \
         Wifi to pick a network and enter its password.",
    );

    // numbered steps
    let steps = [
        "1.  Click the status icons on the time capsule.",
        "2.  In the control panel, click the Wifi button.",
        "3.  Select a network from the list and enter its password.",
    ];
    for s in steps {
        let l = Label::new(Some(s));
        l.add_css_class("row-caption");
        l.set_halign(Align::Start);
        l.set_xalign(0.0);
        l.set_wrap(true);
        inner.append(&l);
    }

    // ── ghost illustration ──
    let ghost = GtkBox::new(Orientation::Vertical, 12);
    ghost.add_css_class("ghost-box");
    ghost.set_margin_top(8);
    ghost.set_halign(Align::Fill);
    ghost.set_can_target(false);
    ghost.set_opacity(0.6);

    // time capsule (top, centred)
    let capsule = GtkBox::new(Orientation::Horizontal, 10);
    capsule.add_css_class("ghost-capsule");
    capsule.set_halign(Align::Center);

    let cos_icon = Image::from_file("/var/lib/cynager/icons/cos.svg");
    cos_icon.set_pixel_size(16);
    cos_icon.set_margin_end(20);
    capsule.append(&cos_icon);

    let clock = Label::new(Some("10:09 PM"));
    clock.add_css_class("ghost-clock");
    capsule.append(&clock);

    // status icons — these are the ones to click, so highlight them
    let status = GtkBox::new(Orientation::Horizontal, 8);
    status.add_css_class("ghost-highlight");
    for icon in [
        "network-wireless-symbolic",
        "battery-full-symbolic",
    ] {
        let img = Image::from_icon_name(icon);
        img.set_pixel_size(16);
        status.append(&img);
    }
    capsule.append(&status);
    ghost.append(&capsule);

    // pointer hint between capsule and panel
    let arrow = Label::new(Some("▼  click the status icons"));
    arrow.add_css_class("ghost-hint");
    arrow.set_halign(Align::Center);
    ghost.append(&arrow);

    // control panel row, wrapped so it stays inside the card
    let panel = GtkBox::new(Orientation::Horizontal, 8);
    panel.add_css_class("ghost-panel");
    panel.set_halign(Align::Center);
    panel.append(&ghost_pill(Some("avatar-default-symbolic"), Some("user"), None));
    panel.append(&ghost_pill(Some("network-wireless-symbolic"), Some("Wifi"), Some("and Networks")));
    panel.append(&ghost_pill(Some("airplane-mode-symbolic"), None, None));
    panel.append(&ghost_pill(Some("audio-volume-high-symbolic"), Some("50%"), None));
    ghost.append(&panel);

    inner.append(&ghost);
    frame
}

// ───────────────────────────── page ─────────────────────────────

pub fn build_net_page() -> Overlay {
    let content = GtkBox::new(Orientation::Vertical, 16);

    // header card
    let card = GtkBox::new(Orientation::Vertical, 10);
    card.add_css_class("shortcut-card");
    card.add_css_class("detail-card");
    card.set_margin_top(10);
    card.set_hexpand(true);

    let infoicon = Image::from_file("/var/lib/cynager/icons/disconnected.svg");
    infoicon.set_pixel_size(54);
    infoicon.set_halign(Align::Center);
    infoicon.set_css_classes(&["card-icons"]);

    let title = Label::new(Some("Network and internet Settings"));
    title.add_css_class("shortcut-title");
    title.set_halign(Align::Center);

    let subtitle = Label::new(Some("Select or edit your networks."));
    subtitle.add_css_class("shortcut-desc");
    subtitle.set_halign(Align::Center);

    card.append(&infoicon);
    card.append(&title);
    card.append(&subtitle);
    content.append(&card);

    let toast = Toast::new();

    let (wifi_head, wifi_switch) = build_wifi_head();
    content.append(&wifi_head);

    let known_title = Label::new(Some("Known Networks"));
    known_title.add_css_class("frame-title");
    known_title.set_halign(Align::Start);
    known_title.set_margin_start(20);
    known_title.set_margin_top(20);
    content.append(&known_title);
    // wifi card
    content.append(&build_wifi_card(&toast, &wifi_switch));

    // ethernet: title + subtitle before the card
    let eth_head = GtkBox::new(Orientation::Vertical, 2);
    eth_head.set_margin_top(20);
    eth_head.set_margin_start(20);

    let eth_title = Label::new(Some("Ethernet"));
    eth_title.add_css_class("frame-title");
    eth_title.set_halign(Align::Start);

    let eth_sub = Label::new(Some("Wired connections and cable status."));
    eth_sub.add_css_class("frame-subtitle");
    eth_sub.set_halign(Align::Start);

    eth_head.append(&eth_title);
    eth_head.append(&eth_sub);
    content.append(&eth_head);
    content.append(&build_eth_card());
    

    // add networks card
    content.append(&build_add_network_card());

    let overlay = Overlay::new();
    let scroller: ScrolledWindow = page_scroller(&content);
    overlay.set_child(Some(&scroller));
    overlay.add_overlay(&toast.revealer);
    overlay
}