use gtk4::prelude::*;
use gtk4::{
    Application, ApplicationWindow, Box as GtkBox, Label, ListBox, ListBoxRow,
    Orientation, ScrolledWindow, SearchEntry, Stack, Separator, Switch, Scale, SpinButton,
    Adjustment, ComboBoxText, Frame, Grid, Align, CssProvider, DrawingArea, gdk_pixbuf::Pixbuf,
};
use std::fs;
use std::process::{Command, exit};
use std::rc::Rc;

const APP_ID: &str = "ekah.scu.calibrate";

const TAB_IDS: &[&str] = &[
    "home", "display", "sound", "network", "appearance", "about",
];

struct MenuItem {
    id: &'static str,
    title: &'static str,
}

const MENU_ITEMS: &[MenuItem] = &[
    MenuItem { id: "home", title: "Home" },
    MenuItem { id: "display", title: "Display" },
    MenuItem { id: "sound", title: "Sound" },
    MenuItem { id: "network", title: "Network" },
    MenuItem { id: "appearance", title: "Appearance" },
    MenuItem { id: "about", title: "About" },
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
            background-color: #212020;
        }

        .right-panel {
            all: unset;
            min-width: 150px;
            padding: 10px 20px 10px 10px;
            border-radius: 15px;
            border: 2px solid transparent;
            background-image: linear-gradient(rgb(27, 27, 27), rgb(27, 27, 27)),
                                linear-gradient(0deg, rgba(251, 251, 251, 0.06), rgba(251, 251, 251, 0.06));
            background-origin: border-box;
            background-clip: padding-box, border-box;
            box-shadow: rgba(7, 7, 7, 0.26) 0px 3px 8px;    

            transition: all 200ms cubic-bezier(0.4, 0, 0.2, 1);
        }

        .menu-search {
            margin: 12px 12px 6px 12px;
            background-color: #1c1e2900;
            border: 1px solid #f8f8f935;
            border-radius: 50px;
            color: #d6d6dd;
        }

        .menu-search image,
        .menu-search entry {
            color: #fefefec9;
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
            background-color: #f9f9f931;
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
            background-color: #171922;
            border: 1px solid #22242f;
            border-radius: 12px;
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
        "display" => (
            "Display".to_string(),
            "Configure how your screen looks and behaves.".to_string(),
        ),
        "sound" => (
            "Sound".to_string(),
            "Control audio input, output, and alerts.".to_string(),
        ),
        "network" => (
            "Network".to_string(),
            "Manage Wi-Fi, VPN, and connection settings.".to_string(),
        ),
        "appearance" => (
            "Appearance".to_string(),
            "Personalize the look and feel of your desktop.".to_string(),
        ),
        "about" => (
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

fn format_bytes(bytes: u64) -> String {
    const UNITS: [&str; 5] = ["B", "KB", "MB", "GB", "TB"];
    let mut size = bytes as f64;
    let mut unit_idx = 0;
    while size >= 1024.0 && unit_idx < UNITS.len() - 1 {
        size /= 1024.0;
        unit_idx += 1;
    }
    return format!("{:.1} {}", size, UNITS[unit_idx])
}

fn rounded_rect(cr: &gtk4::cairo::Context, x: f64, y: f64, w: f64, h: f64, r: f64) {
    let r = r.min(w / 2.0).min(h / 2.0).max(0.0);
    cr.new_sub_path();
    cr.arc(x + w - r, y + r, r, -std::f64::consts::FRAC_PI_2, 0.0);
    cr.arc(x + w - r, y + h - r, r, 0.0, std::f64::consts::FRAC_PI_2);
    cr.arc(x + r, y + h - r, r, std::f64::consts::FRAC_PI_2, std::f64::consts::PI);
    cr.arc(x + r, y + r, r, std::f64::consts::PI, std::f64::consts::PI * 1.5);
    cr.close_path();
}

fn build_percentage_bar(fraction: f64, min_width: i32, height: i32) -> DrawingArea {
    let area = DrawingArea::new();
    area.set_content_width(min_width);
    area.set_content_height(height);
    area.set_hexpand(true);
    area.set_valign(Align::Center);

    let fraction = fraction.clamp(0.0, 1.0);

    area.set_draw_func(move |_, cr, w, h| {
        let w = w as f64;
        let h = h as f64;
        let radius = h / 2.0;

        rounded_rect(cr, 0.0, 0.0, w, h, radius);
        cr.set_source_rgba(1.0, 1.0, 1.0, 0.08);
        let _ = cr.fill();

        if fraction <= 0.0 {
            return;
        }

        let fill_w = (w * fraction).max(h.min(w));

        let (r, g, b) = if fraction < 0.6 {
            (0.30, 0.78, 0.47)
        } else if fraction < 0.85 {
            (0.95, 0.70, 0.25)
        } else {
            (0.90, 0.30, 0.30)
        };

        rounded_rect(cr, 0.0, 0.0, fill_w, h, radius);
        cr.set_source_rgb(r, g, b);
        let _ = cr.fill();
    });

    area
}

struct PartitionInfo {
    mount: String,
    used: u64,
    total: u64,
}

fn read_partitions() -> Vec<PartitionInfo> {
    const EXCLUDED_FS: &[&str] = &[
        "tmpfs", "devtmpfs", "squashfs", "overlay", "proc", "sysfs",
        "cgroup", "cgroup2", "debugfs", "tracefs", "mqueue", "hugetlbfs",
        "devpts", "securityfs", "pstore", "bpf", "autofs", "binfmt_misc",
        "configfs", "efivarfs", "rpc_pipefs", "fuse.gvfsd-fuse", "fusectl",
    ];

    let mut partitions = Vec::new();

    if let Ok(output) = Command::new("df").args(["-T", "-B1"]).output() {
        if let Ok(text) = String::from_utf8(output.stdout) {
            for line in text.lines().skip(1) {
                let fields: Vec<&str> = line.split_whitespace().collect();
                if fields.len() < 7 {
                    continue;
                }

                let fs_type = fields[1];
                if EXCLUDED_FS.contains(&fs_type) {
                    continue;
                }

                let total: u64 = fields[2].parse().unwrap_or(0);
                let used: u64 = fields[3].parse().unwrap_or(0);
                if total == 0 {
                    continue;
                }

                let mount = fields[6..].join(" ");

                partitions.push(PartitionInfo { mount, used, total });
            }
        }
    }

    partitions
}

fn read_module_versions() -> Vec<(String, String)> {
    let path = "/var/lib/cynager/info.probe";
    let mut versions = Vec::new();

    if let Ok(contents) = fs::read_to_string(path) {
        let mut in_ver_block = false;
        for line in contents.lines() {
            let trimmed = line.trim();
            if trimmed == ":ver" {
                in_ver_block = true;
                continue;
            }
            if trimmed == ":end" {
                if in_ver_block {
                    break;
                }
                continue;
            }
            if in_ver_block && !trimmed.is_empty() {
                if let Some((name, ver)) = trimmed.split_once(':') {
                    versions.push((name.trim().to_string(), ver.trim().to_string()));
                }
            }
        }
    }

    versions
}

fn read_os_pretty_name() -> String {
    if let Ok(contents) = fs::read_to_string("/etc/os-release") {
        for line in contents.lines() {
            if let Some(rest) = line.strip_prefix("PRETTY_NAME=") {
                return rest.trim_matches('"').to_string();
            }
        }
    }
    "Unknown OS".to_string()
}

fn read_hostname() -> String {
    fs::read_to_string("/proc/sys/kernel/hostname")
        .map(|s| s.trim().to_string())
        .unwrap_or_else(|_| "unknown".to_string())
}

fn read_kernel_version() -> String {
    Command::new("uname")
        .arg("-r")
        .output()
        .ok()
        .and_then(|o| String::from_utf8(o.stdout).ok())
        .map(|s| s.trim().to_string())
        .unwrap_or_else(|| "unknown".to_string())
}

fn read_cpu_model() -> String {
    if let Ok(contents) = fs::read_to_string("/proc/cpuinfo") {
        for line in contents.lines() {
            if line.starts_with("model name") {
                if let Some((_, val)) = line.split_once(':') {
                    return val.trim().to_string();
                }
            }
        }
    }
    "Unknown CPU".to_string()
}

fn read_memory_info() -> String {
    if let Ok(contents) = fs::read_to_string("/proc/meminfo") {
        let mut total_kb: u64 = 0;
        for line in contents.lines() {
            if line.starts_with("MemTotal:") {
                total_kb = line.split_whitespace().nth(1).and_then(|v| v.parse().ok()).unwrap_or(0);
            }
        }
        if total_kb > 0 {
            return format!("{}", format_bytes(total_kb * 1024));
        }
    }
    "unknown".to_string()
}

fn read_gpu_info() -> String {
    let output = Command::new("lspci")
        .arg("-vnn")
        .output();

    match output {
        Ok(out) => {
            let stdout_str = String::from_utf8_lossy(&out.stdout);
            
            let mut gpu_found = false;
            let mut gpu_deet = "";
            for line in stdout_str.lines() {
                if line.to_lowercase().contains("vga compatible controller") {
                    if let (Some(start), Some(end)) = (line.find('['), line.find(']')) {
                        if start < end {
                            let mut model = line[start + 1..end].trim();
                            
                            if model.contains('/') && !model.contains("Radeon") {
                                let remaining_line = &line[end + 1..];
                                if let (Some(s2), Some(e2)) = (remaining_line.find('['), remaining_line.find(']')) {
                                    model = remaining_line[s2 + 1..e2].trim();
                                }
                            }

                            gpu_deet = model;
                            gpu_found = true;
                            break; 
                        }
                    }
                }
            }

            if !gpu_found {
                return format!("No VGA compatible GPU detected in lspci output.");
            } else {
                return gpu_deet.to_string();
            }
        }
        Err(e) => {
            eprintln!("[calibrate] Failed to execute lspci command: {}", e);
            return format!("ERROR");
        }
    }

}

fn read_shell() -> String {
    std::env::var("SHELL").unwrap_or_else(|_| "unknown".to_string())
}

fn build_storage_card() -> GtkBox {
    let card = GtkBox::new(Orientation::Vertical, 10);
    card.add_css_class("shortcut-card");
    card.add_css_class("detail-card");
    card.set_hexpand(true);


    let storageicon = gtk4::Image::from_file("/var/lib/cynager/icons/disk.svg");
    storageicon.set_pixel_size(54);
    storageicon.set_halign(Align::Start);
    storageicon.set_css_classes(&["card-icons"]);

    let title = Label::new(Some("Storage & Partitions"));
    title.add_css_class("shortcut-title");
    title.set_halign(Align::Start);
    title.set_margin_bottom(20);
    card.append(&storageicon);
    card.append(&title);

    let partitions = read_partitions();

    if partitions.is_empty() {
        let empty = Label::new(Some("No partition data available"));
        empty.add_css_class("shortcut-desc");
        empty.set_halign(Align::Start);
        card.append(&empty);
    } else {
        for p in partitions {
            let fraction = if p.total > 0 { p.used as f64 / p.total as f64 } else { 0.0 };

            let row = GtkBox::new(Orientation::Vertical, 4);
            row.set_margin_top(4);

            let top_row = GtkBox::new(Orientation::Horizontal, 8);

            let mount_lbl = Label::new(Some(&p.mount));
            mount_lbl.add_css_class("row-label");
            mount_lbl.set_halign(Align::Start);
            mount_lbl.set_hexpand(true);
            mount_lbl.set_ellipsize(gtk4::pango::EllipsizeMode::Middle);

            let stats_lbl = Label::new(Some(&format!(
                "{} / {}  ({:.0}%)",
                format_bytes(p.used),
                format_bytes(p.total),
                fraction * 100.0
            )));
            stats_lbl.add_css_class("row-caption");
            stats_lbl.set_halign(Align::End);

            top_row.append(&mount_lbl);
            top_row.append(&stats_lbl);

            let bar = build_percentage_bar(fraction, 100, 10);

            row.append(&top_row);
            row.append(&bar);
            card.append(&row);
        }
    }

    card
}

fn build_versions_card() -> GtkBox {
    let card = GtkBox::new(Orientation::Vertical, 10);
    card.add_css_class("shortcut-card");
    card.set_hexpand(true);

    let vericon = gtk4::Image::from_file("/var/lib/cynager/icons/ver.svg");
    vericon.set_pixel_size(54);
    vericon.set_margin_top(20);
    vericon.set_margin_start(20);
    vericon.set_halign(Align::Start);
    vericon.set_css_classes(&["card-icons"]);
    card.append(&vericon);

    let title = Label::new(Some("Modules"));
    title.add_css_class("shortcut-title");
    title.set_margin_start(20);
    title.set_halign(Align::Start);
    title.set_margin_bottom(20);
    card.append(&title);

    let versions = read_module_versions();

    if versions.is_empty() {
        let empty = Label::new(Some("No version data found at info.probe"));
        empty.add_css_class("shortcut-desc");
        empty.set_halign(Align::Start);
        empty.set_wrap(true);
        card.append(&empty);
    } else {
        let cards_box = GtkBox::builder()
            .orientation(Orientation::Horizontal)
            .spacing(20)
            .margin_start(20)
            .margin_end(20)
            .css_classes(["moduleCosBox"])
            .build();

        let box_scr = ScrolledWindow::builder()
            .vscrollbar_policy(gtk4::PolicyType::Never)
            .hscrollbar_policy(gtk4::PolicyType::Always)
            .vexpand(true)
            .hexpand(true)
            .child(&cards_box)
            .margin_bottom(20)
            .build();

        for (module, version) in versions {
            if module != "cynageOS" {
                let name_lbl = Label::new(Some(&module));
                name_lbl.add_css_class("row-label");
                name_lbl.set_halign(Align::Start);
                name_lbl.set_hexpand(true);

                let ver_lbl = Label::new(Some(&version));
                ver_lbl.add_css_class("row-caption");
                ver_lbl.set_halign(Align::End);

                let cardsmodu = GtkBox::builder()
                    .orientation(Orientation::Horizontal)
                    .spacing(10)
                    .css_classes(["moduleCos"])
                    .build();

                let icon = gtk4::Image::from_file(format!("/var/lib/cynager/icons/{}.png", module));
                icon.set_pixel_size(100);
                icon.set_halign(Align::Start);

                let sidebox = GtkBox::new(Orientation::Vertical, 5);
                sidebox.set_valign(Align::Center);
                sidebox.append(
                    &Label::builder()
                        .label(&module)
                        .css_classes(["moduleTitle"])
                        .halign(Align::Start)
                        .build()
                );
                sidebox.append(
                    &Label::builder()
                        .label(&version)
                        .halign(Align::Start)
                        .css_classes(["moduleSub"])
                        .build()
                );

                cardsmodu.append(&icon);
                cardsmodu.append(&sidebox);

                cards_box.append(&cardsmodu);
            }
        }

        card.append(&box_scr);
    }

    card
}

fn build_device_card(username: &str) -> GtkBox {
    let card = GtkBox::new(Orientation::Vertical, 10);
    card.add_css_class("shortcut-card");
    card.add_css_class("detail-card");
    card.set_hexpand(true);

    let devicon = gtk4::Image::from_file("/var/lib/cynager/icons/device.svg");
    devicon.set_pixel_size(54);
    devicon.set_halign(Align::Start);
    devicon.set_css_classes(&["card-icons"]);
    card.append(&devicon);

    let title = Label::new(Some("Device"));
    title.add_css_class("shortcut-title");
    title.set_halign(Align::Start);
    title.set_margin_bottom(20);
    card.append(&title);

    let rows: [(&str, String); 8] = [
        ("User", username.to_string()),
        ("Hostname", read_hostname()),
        ("OS", read_os_pretty_name()),
        ("Kernel", read_kernel_version()),
        ("CPU", read_cpu_model()),
        ("GPU", read_gpu_info()),
        ("Memory", read_memory_info()),
        ("Shell", read_shell()),
    ];

    let grid = Grid::builder()
        .row_spacing(8)
        .column_spacing(24)
        .build();

    for (i, (label_text, value)) in rows.iter().enumerate() {
        let key_lbl = Label::new(Some(label_text));
        key_lbl.add_css_class("row-caption");
        key_lbl.set_halign(Align::Start);
        key_lbl.set_valign(Align::Start);

        let val_lbl = Label::new(Some(value));
        val_lbl.add_css_class("row-label");
        val_lbl.set_halign(Align::Start);
        val_lbl.set_hexpand(true);
        val_lbl.set_wrap(true);
        val_lbl.set_xalign(0.0);

        grid.attach(&key_lbl, 0, i as i32, 1, 1);
        grid.attach(&val_lbl, 1, i as i32, 1, 1);
    }

    card.append(&grid);
    card
}

fn build_round_user_icon(pixbuf: Pixbuf, size: i32) -> DrawingArea {
    let icon = DrawingArea::new();
    icon.set_content_width(size);
    icon.set_content_height(size);

    icon.set_draw_func(move |_, cr, w, h| {
        let w = w as f64;
        let h = h as f64;
        let cx = w / 2.0;
        let cy = h / 2.0;
        let r  = w / 2.0;

        cr.arc(cx, cy, r, 0.0, 2.0 * std::f64::consts::PI);
        cr.clip();

        let pb = pixbuf.scale_simple(w as i32, h as i32, gtk4::gdk_pixbuf::InterpType::Bilinear).unwrap();
        cr.set_source_pixbuf(&pb, 0.0, 0.0);
        cr.paint().unwrap();

        let shine = gtk4::cairo::LinearGradient::new(
            cx * 0.35, cy * 0.10,
            cx * 0.80, cy * 0.75,
        );
        shine.add_color_stop_rgba(0.00, 1.0, 1.0, 1.0, 0.55);
        shine.add_color_stop_rgba(0.40, 1.0, 1.0, 1.0, 0.18);
        shine.add_color_stop_rgba(1.00, 1.0, 1.0, 1.0, 0.00);

        cr.set_source(&shine).unwrap();

        cr.save().unwrap();
        cr.translate(cx, cy);
        cr.scale(r * 0.85, r * 0.55);
        cr.translate(-r * 0.08, -r * 0.80);
        cr.arc(0.0, 0.0, 1.0, 0.0, 2.0 * std::f64::consts::PI);
        cr.restore().unwrap();
        cr.fill().unwrap();

        let rim = gtk4::cairo::LinearGradient::new(cx * 0.4, 0.0, cx * 1.6, r * 0.18);
        rim.add_color_stop_rgba(0.0, 1.0, 1.0, 1.0, 0.00);
        rim.add_color_stop_rgba(0.5, 1.0, 1.0, 1.0, 0.45);
        rim.add_color_stop_rgba(1.0, 1.0, 1.0, 1.0, 0.00);
        cr.set_source(&rim).unwrap();
        cr.arc(cx, cy, r - 0.5, std::f64::consts::PI * 1.15, std::f64::consts::PI * 1.85);
        cr.set_line_width(1.5);
        cr.stroke().unwrap();
    });

    icon
}

fn build_home_page() -> ScrolledWindow {
    let content = GtkBox::new(Orientation::Vertical, 20);

    let final_path = String::from("/usr/share/octobacillus/usericon.png"); 
    let pixbuf = Pixbuf::from_file(&final_path).unwrap();
    let usricon = build_round_user_icon(pixbuf.clone(), 40);

    let usrname = match std::fs::read_to_string("/usr/share/octobacillus/user.octo") {
        Ok(content) => content,
        Err(err) => {
            eprintln!("[ctrl] Error reading file: {}", err);
            "name = user4.0".to_string()
        }
    };

    let name = usrname
        .lines()
        .find(|line| line.trim().starts_with("name"))
        .and_then(|line| line.split_once("="))
        .map(|(_, value)| value.trim().to_string())
        .unwrap_or_else(|| "user4.0".to_string());

    let usrbox = GtkBox::builder()
        .orientation(Orientation::Horizontal)
        .css_classes(["usrcard"])
        .spacing(10)
        .hexpand(false)
        .halign(Align::Start)
        .build();

    usrbox.append(&usricon);
    usrbox.append(
        &Label::builder()
            .label(&name)
            .css_classes(["usrname"])
            .justify(gtk4::Justification::Right)
            .margin_start(10)
            .build()
    );
    
    let cards_box = GtkBox::new(Orientation::Vertical, 15);
    cards_box.append(&build_storage_card());
    cards_box.append(&build_versions_card());
    cards_box.append(&build_device_card(&name));

    content.append(&usrbox);
    content.append(&cards_box);

    // let bg = gtk4::Image::from_file("/var/lib/cynager/icons/cog_bg.svg");
    // bg.set_pixel_size(500);
    // bg.set_hexpand(true);
    // bg.set_vexpand(true);
    // bg.set_halign(Align::Start);
    // bg.set_valign(Align::End);

    // let over = gtk4::Overlay::new();
    // over.set_child(Some(&bg));
    // over.add_overlay(&content);

    // let overbox = GtkBox::new(Orientation::Vertical, 0);
    // overbox.append(&over);

    page_scroller(&content)
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

fn build_appearance_page() -> ScrolledWindow {
    let content = GtkBox::new(Orientation::Vertical, 16);

    let frame = Frame::new(None);
    let list = ListBox::new();
    list.set_selection_mode(gtk4::SelectionMode::None);

    let theme = ComboBoxText::new();
    theme.append_text("Dark");
    theme.append_text("Light");
    theme.append_text("System Default");
    theme.set_active(Some(0));
    list.append(&ListBoxRow::builder()
        .child(&labeled_row("Theme", "Application color scheme", &theme))
        .build());

    list.append(&Separator::new(Orientation::Horizontal));

    let animations = Switch::new();
    animations.set_active(true);
    animations.set_valign(Align::Center);
    list.append(&ListBoxRow::builder()
        .child(&labeled_row("Animations", "Enable window and menu animations", &animations))
        .build());

    list.append(&Separator::new(Orientation::Horizontal));

    let scale_adj = Adjustment::new(1.0, 0.5, 2.0, 0.05, 0.1, 0.0);
    let ui_scale = Scale::new(Orientation::Horizontal, Some(&scale_adj));
    ui_scale.set_size_request(180, -1);
    list.append(&ListBoxRow::builder()
        .child(&labeled_row("UI Scale", "Interface scaling factor", &ui_scale))
        .build());

    frame.set_child(Some(&list));
    content.append(&frame);
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
    panel.set_margin_top(10);
    panel.set_margin_bottom(10);
    panel.set_margin_start(10);
    panel.set_margin_end(10);
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
        .default_width(1000)
        .default_height(900)
        .resizable(true)
        .build();

    let stack = Stack::new();
    stack.set_transition_type(gtk4::StackTransitionType::SlideUpDown);
    stack.set_transition_duration(300);

    let home_page = build_home_page();
    stack.add_titled(&home_page, Some("home"), "Home");

    let display_page = build_display_page();
    stack.add_titled(&display_page, Some("display"), "Display");

    let sound_page = build_sound_page();
    stack.add_titled(&sound_page, Some("sound"), "Sound");

    let network_page = build_network_page();
    stack.add_titled(&network_page, Some("network"), "Network");

    let appearance_page = build_appearance_page();
    stack.add_titled(&appearance_page, Some("appearance"), "Appearance");

    let about_page = build_about_page(&username);
    stack.add_titled(&about_page, Some("about"), "About");

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

fn main() -> gtk4::glib::ExitCode {
    let args: Vec<String> = std::env::args().collect();
    
    if args.len() > 1 && (args[1] == "--version" || args[1] == "-V") {
        println!("{} {}", env!("CARGO_PKG_NAME"), env!("CARGO_PKG_VERSION"));
        exit(1);
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