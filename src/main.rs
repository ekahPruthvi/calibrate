use gtk4::prelude::*;
use gtk4::{
    Application, ApplicationWindow, Box as GtkBox, Label, ListBox, ListBoxRow,
    Orientation, ScrolledWindow, SearchEntry, Stack, Separator, Switch, Scale, SpinButton,
    Adjustment, ComboBoxText, Frame, Grid, Align, CssProvider, DrawingArea, gdk_pixbuf::Pixbuf,
    Button, Dialog, DropTarget, StackSwitcher, gio, gdk, glib,
};
use std::fs;
use std::process::{Command, exit};
use std::{rc::Rc, path::PathBuf};

use niri_ipc::{Request, Response, socket::Socket};
use infoprober::{parse, Entry, Value};

use gtk4:: gdk_pixbuf::{InterpType};
use std::cell::RefCell;

use self_update::cargo_crate_version;

const APP_ID: &str = "ekah.scu.calibrate";

const TAB_IDS: &[&str] = &[
    "home", "appearance", "shellset", "display", "sound", "net", "blue", 
    "Storage", "bat", "shortcuts", "pills", "apps", "user", "abt",
];

mod home;
mod appearence;

struct MenuItem {
    id: &'static str,
    title: &'static str,
}

const MENU_ITEMS: &[MenuItem] = &[
    MenuItem { id: "home", title: "Home" },
    MenuItem { id: "appearance", title: "Appearance" },
    MenuItem { id: "shellset", title: "Shell" },
    MenuItem { id: "display", title: "Display" },
    MenuItem { id: "sound", title: "Sound" },
    MenuItem { id: "net", title: "Network" },
    MenuItem { id: "blue", title: "Bluetooth" },
    MenuItem { id: "Storage", title: "Storage" },
    MenuItem { id: "bat", title: "Battery" },
    MenuItem { id: "shortcuts", title: "Shortcuts" },
    MenuItem { id: "pills", title: "Pills" },
    MenuItem { id: "apps", title: "Applications" },
    MenuItem { id: "user", title: "User" },
    MenuItem { id: "abt", title: "About" },
];

fn read_username() -> String {
    let path = "/usr/share/octobacillus/user.octo";
    if let Ok(contents) = fs::read_to_string(path) {
        for line in contents.lines() {
            let line = line.trim();
            if line.starts_with("name") {
                if let Some(val) = line.splitn(2, '=').nth(1) {
                    return val.trim().to_string();
                }
            }
        }
    }
    "ekah".to_string()
}

fn parse_tab_arg() -> Option<String> {
    let args: Vec<String> = std::env::args().collect();
    let mut iter = args.iter().skip(1).peekable();

    while let Some(arg) = iter.next() {
        let candidate = if let Some(rest) = arg.strip_prefix("--tab=") {
            Some(rest.to_string())
        } else if arg == "--tab" || arg == "-t" {
            iter.next().cloned()
        } else if !arg.starts_with('-') {
            Some(arg.clone())
        } else {
            None
        };

        if let Some(c) = candidate {
            let c = c.to_lowercase();
            if TAB_IDS.contains(&c.as_str()) {
                return Some(c);
            } else {
                eprintln!(
                    "calibrate: unknown tab '{}', valid tabs are: {}",
                    c,
                    TAB_IDS.join(", ")
                );
            }
        }
    }
    None
}

fn load_css() {
    let css = CssProvider::new();
    css.load_from_data(
        r#"
        window {
            background-color: #202120;
        }

        .right-panel {
            all: unset;
            min-width: 120px;
            padding: 10px 20px 10px 10px;
            border-radius: 0px;
            background-color: rgba(251, 248, 248, 0.12);
            box-shadow: inset 8px 0px 8px -4px rgba(0, 0, 0, 0.1);

            transition: all 200ms cubic-bezier(0.4, 0, 0.2, 1);
        }

        .menu-search {
            all: unset;
            padding: 10px;
            margin: 12px 12px 6px 12px;
            background-color: #1c1e2900;
            border: 1px solid #f8f8f935;
            border-radius: 18px;
            color: #d6d6dd;
        }

        .menu-search:focus-within {
            background-color: #000000b2;
            border: 1px solid #f8f8f935;
        }

        .menu-search image,
        .menu-search entry {
            color: #fefefec9;
            margin-right: 5px;
        }

        .menu-list {
            background-color: transparent;
            padding: 5px;   
        }

        .menu-list row {
            all: unset;
            padding: 10px 12px;
            border-radius: 8px;
            color: #ffffffd2;
            background-color: transparent;
            border: 1px solid transparent;
        }

        .menu-list row:selected {
            background-color: #ffffff22;
            color: #ffffff;
        }

        .menu-list row:hover {
            background-color: #ffffff00;
            border: 1px solid #ffffff2f;
        }

        .page-header-bar {
            padding: 10px;
            border-top: 1px solid #ffffff17;
        }

        .settings-page {
            background-color: #ece4e400;
            padding: 24px;
        }

        .page-title {
            font-size: 12px;
            font-weight: 400;
            color: #f2f2f56e;
        }

        .page-subtitle {
            color: #8a8a99;
            font-size: 13px;
        }

        frame {
            all: unset;
            min-width: 150px;
            border-radius: 30px;
            border: 2px solid #ffffff0e;
            background-color: #ffffff0f;
            padding: 0;
        }

        frame > label {
            color: #8a8a99;
        }

        .shortcut-card {
            all: unset;
            min-width: 150px;
            border-radius: 30px;
            border: 2px solid #ffffff0e;
            background-color: #ffffff0f;
        }
        
        .usrcard {
            all: unset;
            min-width: 150px;
            padding: 10px;
            border-radius: 50px;
            border: 1px solid #0e0d0dea;
            background-color: rgba(255, 255, 255, 0.88);
            box-shadow: rgba(0, 0, 0, 0.24) 0px 3px 8px;

            transition: all 200ms cubic-bezier(0.4, 0, 0.2, 1);
        }

        .shortcut-card:hover {
            background-color: #f2f3f913;
        }

        .detail-card {
            padding: 20px;
        }

        .shortcut-title {
            font-weight: 600;
            font-size: 22px;
            color: #f2f2f5;
        }

        .shortcut-desc {
            color: #8a8a99;
            font-size: 12px;
        }

        .frame-title {
            font-weight: 600;
            font-size: 18px;
            color: #f2f2f5;
        }

        .frame-subtitle {
            font-weight: 300;
            font-size: 12px;
            color: #f2f2f567;
        }

        .row-label {
            color: #d6d6dd;
        }

        .row-caption {
            color: #ffffffbd;
            font-size: 11px;
            font-weight: 900;
        }

        scrollbar {
            background-color: transparent;
        }

        scrollbar slider {
            background-color: #3a3d5203;
            border-radius: 8px;
            min-width: 8px;
            min-height: 8px;
            border: 2px solid transparent;
        }

        scrollbar slider:hover {
            background-color: #4d517000;
        }

        scrollbar slider:active {
            background-color: #6c70a000;
        }

        scrollbar trough {
            background-color: #14161f00;
            border-radius: 8px;
        }

        .usrname {
            font-size: 26px;
            font-weight: 500;
            color: black;
        }

        .flat {
            all: unset;
        }

        .card-icons {
            padding: 5px;
            background-color: #ffffff62;
            border-radius: 20px;
            box-shadow: rgba(50, 50, 93, 0.25) 0px 6px 12px -2px, rgba(0, 0, 0, 0.3) 0px 3px 7px -3px;
        }

        .moduleCos {
            padding: 20px;
            min-width: 300px;
            background-color: #0000005b;
            border: 1px solid #ffffff39;
            border-radius: 20px;
        }

        .moduleTitle {
            font-size: 18px;
            font-weight: 400;
        }

        .moduleSub {
            font-size: 12px;
            font-weight: 300;
            color: #ffffff8d;
        }

        stackswitcher {
            all: unset;
            padding: 6px;
            background-color: transparent;
        }

        stackswitcher button {
            all: unset;
            color: #d8dee9;
            border-radius: 10px;
            padding: 10px;
            background-color: transparent;
            
            transition: background-color 200ms ease-in-out, 
                        color 200ms ease-in-out, 
                        transform 150ms cubic-bezier(0.25, 1, 0.5, 1);
        }
        
        stackswitcher button:checked {
            background-color: #222222;
            border-radius: 15px;
            color: #eceff4;
            font-weight: bold;
        }

        stackswitcher button:active {
            transform: scale(0.96);
            transition: transform 50ms ease-out;
        }

        .wallpaperPrev {
            background-color: black;
            border-radius: 20px;
            box-shadow: rgba(0, 0, 0, 0.19) 0px 10px 20px, rgba(0, 0, 0, 0.23) 0px 6px 6px;
        }

        .wallpaperPrev > img {
            border-radius: 20px;
        }

        .wall-thumb {
            all:unset;
            border-radius: 10px;
            box-shadow: rgba(0, 0, 0, 0.16) 0px 3px 6px, rgba(0, 0, 0, 0.23) 0px 3px 6px;
            transition: all 200ms cubic-bezier(0.4, 0, 0.2, 1);
        }

        .wall-thumb:hover {
            transform: scale(1.03);
        }

        .wall-thumb:active {
            transform: scale(0.9);
        }

        .sub-btn {
            all: unset;
            min-width: 160px;
            border-radius: 15px;
            border: none;
            padding: 10px;

            background: linear-gradient(#fff2, #0001),  #d8ff7c;
            box-shadow:
                1px 1px 2px -1px #fff inset,
                0 2px 1px #00000010,
                0 4px 2px #00000010,
                0 8px 4px #00000010,
                0 16px 8px #00000010,
                0 32px 16px #00000010;
            color: black;
            transition:
                transform var(--duration) var(--timing-function),
                filter var(--duration) var(--timing-function);
        }

        .sub-btn:hover {
            transform: scale(1.03);
        }

        .sub-btn:active {
            transform: scale(0.9);
        }

        .destructive-action {
            all: unset;
            min-width: 160px;
            border-radius: 15px;
            border: none;
            padding: 10px;

            background: linear-gradient(#fff2, #0001),  #ff7c7c;
            box-shadow:
                1px 1px 2px -1px #fff inset,
                0 2px 1px #00000010,
                0 4px 2px #00000010,
                0 8px 4px #00000010,
                0 16px 8px #00000010,
                0 32px 16px #00000010;
            color: black;
            transition:
                transform var(--duration) var(--timing-function),
                filter var(--duration) var(--timing-function);
        }

        .destructive-action:hover {
            background: linear-gradient(#fff2, #0001),  #e76e6e;
        }

        .destructive-action:active {
            transform: scale(0.9);
        }

        .drop-zone {
            border: 2px dashed alpha(currentColor, 0.35);
            border-radius: 12px;
            background-color: alpha(currentColor, 0.03);
            transition: all 200ms ease;
        }
        .drop-zone.drag-active {
            border-color: @accent_color;
            background-color: alpha(@accent_color, 0.08);
        }
        .drop-zone.success {
            border-color: @success_color;
            background-color: alpha(@success_color, 0.08);
        }
        .tick {
            color: @success_color;
            font-size: 48px;
        }

        .popoverd {
            padding: 2px;
            border: 2px solid transparent;
            background-image: 
                linear-gradient(rgb(6, 6, 6), rgb(6, 6, 6)),
                linear-gradient(0deg, rgb(9, 9, 9), rgba(61, 61, 61, 0.686));
            background-origin: border-box;
            background-clip: padding-box, border-box;
            border-radius: 15px;
            transition: all 0.5s ease;
        }

        "#,
    );
    gtk4::style_context_add_provider_for_display(
        &gtk4::gdk::Display::default().expect("Could not connect to display"),
        &css,
        gtk4::STYLE_PROVIDER_PRIORITY_APPLICATION,
    );
}

fn page_scroller(content: &GtkBox) -> ScrolledWindow {
    content.add_css_class("settings-page");
    ScrolledWindow::builder()
        .vscrollbar_policy(gtk4::PolicyType::Always)
        .hscrollbar_policy(gtk4::PolicyType::Never)
        .vexpand(true)
        .hexpand(true)
        .child(content)
        .build()
}

fn build_placeholder_page(title: &str, message: &str) -> ScrolledWindow {
    let content = GtkBox::new(Orientation::Vertical, 16);

    let frame = Frame::new(None);
    let inner = GtkBox::new(Orientation::Vertical, 8);
    inner.set_margin_top(40);
    inner.set_margin_bottom(40);
    inner.set_margin_start(20);
    inner.set_margin_end(20);
    inner.set_halign(Align::Center);
    inner.set_valign(Align::Center);

    let title_lbl = Label::new(Some(title));
    title_lbl.add_css_class("shortcut-title");
    title_lbl.set_halign(Align::Center);

    let msg_lbl = Label::new(Some(message));
    msg_lbl.add_css_class("shortcut-desc");
    msg_lbl.set_halign(Align::Center);
    msg_lbl.set_justify(gtk4::Justification::Center);
    msg_lbl.set_wrap(true);

    inner.append(&title_lbl);
    inner.append(&msg_lbl);

    frame.set_child(Some(&inner));
    content.append(&frame);
    page_scroller(&content)
}

fn build_page_header() -> (GtkBox, Label, Label) {
    let header = GtkBox::new(Orientation::Horizontal, 4);
    header.add_css_class("page-header-bar");

    let title_lbl = Label::new(None);
    title_lbl.add_css_class("page-title");
    title_lbl.set_halign(Align::Start);
    // title_lbl.set_justify(gtk4::Justification::Right);

    let subtitle_lbl = Label::new(None);
    subtitle_lbl.add_css_class("page-subtitle");
    subtitle_lbl.set_halign(Align::Start);

    header.append(&title_lbl);
    // header.append(&subtitle_lbl);
    header.set_margin_start(10);

    (header, title_lbl, subtitle_lbl)
}

fn page_meta(id: &str, username: &str) -> (String, String) {
    match id {
        "home" => (
            format!("Porfile: [{}] - Calibrate by SCU", username),
            "Jump straight into a settings category below.".to_string(),
        ),
        "appearance" => (
            "Appearance".to_string(),
            "Personalize the look and feel of your desktop.".to_string(),
        ),
        "shellset" => (
            "Shell".to_string(),
            "Configure shell behavior and startup options.".to_string(),
        ),
        "display" => (
            "Display".to_string(),
            "Configure how your screen looks and behaves.".to_string(),
        ),
        "sound" => (
            "Sound".to_string(),
            "Control audio input, output, and alerts.".to_string(),
        ),
        "net" => (
            "Network".to_string(),
            "Manage Wi-Fi, VPN, and connection settings.".to_string(),
        ),
        "blue" => (
            "Bluetooth".to_string(),
            "Manage paired devices and Bluetooth visibility.".to_string(),
        ),
        "Storage" => (
            "Storage".to_string(),
            "Review disk usage across mounted partitions.".to_string(),
        ),
        "bat" => (
            "Battery".to_string(),
            "Power usage and battery health settings.".to_string(),
        ),
        "shortcuts" => (
            "Shortcuts".to_string(),
            "Customize keyboard shortcuts and hotkeys.".to_string(),
        ),
        "pills" => (
            "Pills".to_string(),
            "Placeholder section — not yet defined.".to_string(),
        ),
        "apps" => (
            "Applications".to_string(),
            "Manage installed applications and defaults.".to_string(),
        ),
        "user" => (
            "User".to_string(),
            "Account details and user preferences.".to_string(),
        ),
        "abt" => (
            "About".to_string(),
            "System and version information.".to_string(),
        ),
        _ => ("Settings".to_string(), String::new()),
    }
}

fn labeled_row(label_text: &str, caption: &str, widget: &impl IsA<gtk4::Widget>) -> GtkBox {
    let row = GtkBox::new(Orientation::Horizontal, 12);
    row.set_margin_top(10);
    row.set_margin_bottom(10);
    row.set_margin_start(14);
    row.set_margin_end(14);

    let text_box = GtkBox::new(Orientation::Vertical, 2);
    let lbl = Label::new(Some(label_text));
    lbl.add_css_class("row-label");
    lbl.set_halign(Align::Start);

    let cap = Label::new(Some(caption));
    cap.add_css_class("row-caption");
    cap.set_halign(Align::Start);

    text_box.append(&lbl);
    text_box.append(&cap);
    text_box.set_hexpand(true);

    row.append(&text_box);
    row.append(widget);
    row
}

fn build_display_page() -> ScrolledWindow {
    let content = GtkBox::new(Orientation::Vertical, 16);

    let frame = Frame::new(None);
    let list = ListBox::new();
    list.set_selection_mode(gtk4::SelectionMode::None);

    let brightness = Scale::with_range(Orientation::Horizontal, 0.0, 100.0, 1.0);
    brightness.set_value(72.0);
    brightness.set_size_request(180, -1);
    list.append(&ListBoxRow::builder()
        .child(&labeled_row("Brightness", "Adjust screen brightness", &brightness))
        .build());

    list.append(&Separator::new(Orientation::Horizontal));

    let night_light = Switch::new();
    night_light.set_active(true);
    night_light.set_valign(Align::Center);
    list.append(&ListBoxRow::builder()
        .child(&labeled_row("Night Light", "Reduce blue light after sunset", &night_light))
        .build());

    list.append(&Separator::new(Orientation::Horizontal));

    let resolution = ComboBoxText::new();
    resolution.append_text("1920 x 1080");
    resolution.append_text("2560 x 1440");
    resolution.append_text("3840 x 2160");
    resolution.set_active(Some(0));
    list.append(&ListBoxRow::builder()
        .child(&labeled_row("Resolution", "Native display resolution", &resolution))
        .build());

    frame.set_child(Some(&list));
    content.append(&frame);
    page_scroller(&content)
}

fn build_sound_page() -> ScrolledWindow {
    let content = GtkBox::new(Orientation::Vertical, 16);

    let frame = Frame::new(None);
    let list = ListBox::new();
    list.set_selection_mode(gtk4::SelectionMode::None);

    let volume = Scale::with_range(Orientation::Horizontal, 0.0, 100.0, 1.0);
    volume.set_value(58.0);
    volume.set_size_request(180, -1);
    list.append(&ListBoxRow::builder()
        .child(&labeled_row("Output Volume", "Master output level", &volume))
        .build());

    list.append(&Separator::new(Orientation::Horizontal));

    let output_device = ComboBoxText::new();
    output_device.append_text("Built-in Speakers");
    output_device.append_text("Headphones");
    output_device.append_text("HDMI Output");
    output_device.set_active(Some(0));
    list.append(&ListBoxRow::builder()
        .child(&labeled_row("Output Device", "Where sound is played", &output_device))
        .build());

    list.append(&Separator::new(Orientation::Horizontal));

    let mute_alerts = Switch::new();
    mute_alerts.set_valign(Align::Center);
    list.append(&ListBoxRow::builder()
        .child(&labeled_row("Mute Alert Sounds", "Silence system notification sounds", &mute_alerts))
        .build());

    frame.set_child(Some(&list));
    content.append(&frame);
    page_scroller(&content)
}

fn build_network_page() -> ScrolledWindow {
    let content = GtkBox::new(Orientation::Vertical, 16);

    let frame = Frame::new(None);
    let list = ListBox::new();
    list.set_selection_mode(gtk4::SelectionMode::None);

    let wifi = Switch::new();
    wifi.set_active(true);
    wifi.set_valign(Align::Center);
    list.append(&ListBoxRow::builder()
        .child(&labeled_row("Wi-Fi", "Enable wireless networking", &wifi))
        .build());

    list.append(&Separator::new(Orientation::Horizontal));

    let vpn = Switch::new();
    vpn.set_valign(Align::Center);
    list.append(&ListBoxRow::builder()
        .child(&labeled_row("VPN", "Route traffic through VPN", &vpn))
        .build());

    list.append(&Separator::new(Orientation::Horizontal));

    let adjustment = Adjustment::new(3128.0, 0.0, 65535.0, 1.0, 10.0, 0.0);
    let proxy_port = SpinButton::new(Some(&adjustment), 1.0, 0);
    list.append(&ListBoxRow::builder()
        .child(&labeled_row("Proxy Port", "Local proxy listening port", &proxy_port))
        .build());

    frame.set_child(Some(&list));
    content.append(&frame);
    page_scroller(&content)
}

fn build_shell_page() -> ScrolledWindow {
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

    let subtitle = Label::new(Some("Control and setup cynageOS the way you want."));
    subtitle.add_css_class("shortcut-desc");
    subtitle.set_halign(Align::Center);

    card.append(&infoicon);
    card.append(&title);
    card.append(&subtitle);

    content.append(&card);


    let outframe = Frame::new(None);

    let outbox = GtkBox::new(Orientation::Vertical, 10);
    let outtitle = Label::new(Some("Output"));
    outtitle.add_css_class("frame-title");
    outtitle.set_halign(Align::Start);

    let wallhead = GtkBox::new(Orientation::Horizontal, 5);


    outbox.append(&outtitle);
    outbox.append(&wallhead);

    outframe.set_child(Some(&outbox));

    content.append(&outframe);
    page_scroller(&content)
}

fn build_about_page(username: &str) -> ScrolledWindow {
    let content = GtkBox::new(Orientation::Vertical, 16);

    let frame = Frame::new(None);
    let list = ListBox::new();
    list.set_selection_mode(gtk4::SelectionMode::None);

    let rows: [(&str, String); 4] = [
        ("User", username.to_string()),
        ("Application", "Calibrate".to_string()),
        ("Version", "0.1.0".to_string()),
        ("Toolkit", "GTK 4 / gtk-rs".to_string()),
    ];

    for (i, (label_text, value)) in rows.iter().enumerate() {
        let value_lbl = Label::new(Some(value));
        value_lbl.add_css_class("row-caption");
        list.append(&ListBoxRow::builder()
            .child(&labeled_row(label_text, "", &value_lbl))
            .build());
        if i != rows.len() - 1 {
            list.append(&Separator::new(Orientation::Horizontal));
        }
    }

    frame.set_child(Some(&list));
    content.append(&frame);
    page_scroller(&content)
}

fn build_right_panel(stack: &Stack) -> (GtkBox, ListBox) {
    let panel = GtkBox::new(Orientation::Vertical, 0);
    panel.add_css_class("right-panel");
    panel.set_size_request(220, -1);
    // panel.set_margin_top(10);
    // panel.set_margin_bottom(10);
    panel.set_margin_start(10);
    // panel.set_margin_end(10);
    panel.set_hexpand(false);

    let search = SearchEntry::new();
    search.add_css_class("menu-search");
    search.set_placeholder_text(Some("Search in settings"));

    let list = ListBox::new();
    list.set_selection_mode(gtk4::SelectionMode::Single);
    list.add_css_class("menu-list");

    for item in MENU_ITEMS {
        let row = ListBoxRow::new();
        row.set_widget_name(item.id);
        row.set_margin_bottom(5);

        let label = Label::new(Some(item.title));
        label.set_halign(Align::Start);
        row.set_child(Some(&label));

        list.append(&row);
    }

    let search_for_filter = search.clone();
    list.set_filter_func(move |row| {
        let query = search_for_filter.text().to_lowercase();
        if query.is_empty() {
            return true;
        }
        row.child()
            .and_then(|w| w.downcast::<Label>().ok())
            .map(|lbl| lbl.text().to_lowercase().contains(&query))
            .unwrap_or(true)
    });

    let list_for_search = list.clone();
    search.connect_search_changed(move |_| {
        list_for_search.invalidate_filter();
    });

    let stack_for_activate = stack.clone();
    list.connect_row_activated(move |_, row| {
        stack_for_activate.set_visible_child_name(&row.widget_name());
    });

    let list_scroller = ScrolledWindow::builder()
        .vscrollbar_policy(gtk4::PolicyType::Automatic)
        .hscrollbar_policy(gtk4::PolicyType::Never)
        .vexpand(true)
        .child(&list)
        .build();

    panel.append(&search);
    panel.append(&list_scroller);

    (panel, list)
}

fn build_ui(app: &Application, initial_tab: Option<String>) {
    load_css();

    let username = read_username();

    let window = ApplicationWindow::builder()
        .application(app)
        .title("Calibrate")
        .default_width(1100)
        .default_height(600)
        .resizable(true)
        .build();

    window.connect_map(|_| println!("ready"));

    let stack = Stack::new();
    stack.set_transition_type(gtk4::StackTransitionType::SlideUpDown);
    stack.set_transition_duration(300);

    let home_page = home::build_home_page();
    stack.add_titled(&home_page, Some("home"), "Home");

    let appearance_page = appearence::build_appearance_page(&window);
    stack.add_titled(&appearance_page, Some("appearance"), "Appearance");

    let shellset_page = build_shell_page();
    stack.add_titled(&shellset_page, Some("shellset"), "Shell");

    let display_page = build_display_page();
    stack.add_titled(&display_page, Some("display"), "Display");

    let sound_page = build_sound_page();
    stack.add_titled(&sound_page, Some("sound"), "Sound");

    let network_page = build_network_page();
    stack.add_titled(&network_page, Some("net"), "Network");

    let blue_page = build_placeholder_page("Bluetooth", "Bluetooth settings are coming soon.");
    stack.add_titled(&blue_page, Some("blue"), "Bluetooth");

    let storage_page = build_placeholder_page("Storage", "Detailed storage settings are coming soon.");
    stack.add_titled(&storage_page, Some("Storage"), "Storage");

    let battery_page = build_placeholder_page("Battery", "Battery settings are coming soon.");
    stack.add_titled(&battery_page, Some("bat"), "Battery");

    let shortcuts_page = build_placeholder_page("Shortcuts", "Keyboard shortcut settings are coming soon.");
    stack.add_titled(&shortcuts_page, Some("shortcuts"), "Shortcuts");

    let pills_page = build_placeholder_page("Pills", "This section is not yet defined.");
    stack.add_titled(&pills_page, Some("pills"), "Pills");

    let apps_page = build_placeholder_page("Applications", "Application settings are coming soon.");
    stack.add_titled(&apps_page, Some("apps"), "Applications");

    let user_page = build_placeholder_page("User", "User account settings are coming soon.");
    stack.add_titled(&user_page, Some("user"), "User");

    let about_page = build_about_page(&username);
    stack.add_titled(&about_page, Some("abt"), "About");

    let (header, title_lbl, subtitle_lbl) = build_page_header();

    let (right_panel, menu_list) = build_right_panel(&stack);

    let sync_for_change: Rc<dyn Fn(&str)> = {
        let title_lbl = title_lbl.clone();
        let subtitle_lbl = subtitle_lbl.clone();
        let username = username.clone();
        let menu_list = menu_list.clone();
        Rc::new(move |id: &str| {
            let (title, subtitle) = page_meta(id, &username);
            title_lbl.set_text(&title);
            subtitle_lbl.set_text(&subtitle);

            let mut idx = 0;
            while let Some(row) = menu_list.row_at_index(idx) {
                if row.widget_name() == id {
                    menu_list.select_row(Some(&row));
                    break;
                }
                idx += 1;
            }
        })
    };

    let sync_for_signal = sync_for_change.clone();
    stack.connect_notify_local(Some("visible-child-name"), move |stack, _| {
        if let Some(name) = stack.visible_child_name() {
            sync_for_signal(&name);
        }
    });

    let content_area = GtkBox::new(Orientation::Vertical, 0);
    content_area.set_hexpand(true);
    content_area.append(&stack);
    content_area.append(&header);

    let main_box = GtkBox::new(Orientation::Horizontal, 0);
    main_box.append(&content_area);
    main_box.append(&right_panel);

    window.set_child(Some(&main_box));

    let target = initial_tab.unwrap_or_else(|| "home".to_string());
    stack.set_visible_child_name(&target);
    sync_for_change(&target);

    window.present();
}

struct UpdateOutcome {
    updated: bool,
    version: String,
}

const CALIBRATE_BIN_PATH: &str = "/usr/bin/calibrate";

fn ensure_writable(path: &str) -> Result<(), String> {
    use std::ffi::CString;

    let c_path = CString::new(path).map_err(|e| e.to_string())?;

    let path_obj = std::path::Path::new(path);
    if path_obj.exists() {
        let writable = unsafe { libc::access(c_path.as_ptr(), libc::W_OK) == 0 };
        if !writable {
            return Err(format!(
                "no write permission for '{}'. Try: sudo calibrate --update",
                path
            ));
        }
    }

    if let Some(parent) = path_obj.parent() {
        let c_parent = CString::new(parent.to_string_lossy().to_string())
            .map_err(|e| e.to_string())?;
        let dir_writable = unsafe { libc::access(c_parent.as_ptr(), libc::W_OK) == 0 };
        if !dir_writable {
            return Err(format!(
                "no write permission for directory '{}'. Try: sudo calibrate --update",
                parent.display()
            ));
        }
    }

    Ok(())
}

fn check_for_updates() -> Result<UpdateOutcome, Box<dyn std::error::Error>> {
    let status = self_update::backends::github::Update::configure()
        .repo_owner("ekahPruthvi")
        .repo_name("calibrate")
        .bin_name("calibrate")
        .target("x86_64")
        .bin_install_path(CALIBRATE_BIN_PATH)
        .show_download_progress(true)
        .no_confirm(true)
        .current_version(cargo_crate_version!())
        .build()?
        .update()?;

    Ok(UpdateOutcome {
        updated: status.is_updated(),
        version: status.version().to_string(),
    })
}

fn run_update() -> i32 {
    println!("Checking for updates...");

    if let Err(msg) = ensure_writable(CALIBRATE_BIN_PATH) {
        eprintln!("Cannot update calibrate: {}", msg);
        return 1;
    }

    match check_for_updates() {
        Ok(outcome) => {
            if outcome.updated {
                println!("calibrate updated to version {}", outcome.version);
            } else {
                println!("calibrate is already up to date ({})", outcome.version);
            }
            0
        }
        Err(e) => {
            eprintln!("Update failed: {}", e);
            1
        }
    }
}

fn main() -> gtk4::glib::ExitCode {
    let args: Vec<String> = std::env::args().collect();
    
    if args.len() > 1 && (args[1] == "--version" || args[1] == "-V") {
        println!("{} {}", env!("CARGO_PKG_NAME"), env!("CARGO_PKG_VERSION"));
        exit(1);
    } else if args.len() > 1 && (args[1] == "--update" || args[1] == "-U") {
        exit(run_update());
    }

    let initial_tab = parse_tab_arg();

    let app = Application::builder()
        .application_id(APP_ID)
        .build();

    app.connect_activate(move |app| {
        build_ui(app, initial_tab.clone());
    });

    app.run_with_args::<&str>(&[])
}