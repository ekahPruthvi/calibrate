use gtk4::prelude::*;
use gtk4::{
    Box as GtkBox, Label,
    Orientation, ScrolledWindow, Grid, Align, DrawingArea, gdk_pixbuf::Pixbuf,
};
use std::fs;
use std::process::Command;

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

pub fn rounded_rect(cr: &gtk4::cairo::Context, x: f64, y: f64, w: f64, h: f64, r: f64) {
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

pub fn build_home_page() -> ScrolledWindow {
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

    page_scroller(&content)
}

pub fn page_scroller(content: &GtkBox) -> ScrolledWindow {
    content.add_css_class("settings-page");
    ScrolledWindow::builder()
        .vscrollbar_policy(gtk4::PolicyType::Always)
        .hscrollbar_policy(gtk4::PolicyType::Never)
        .vexpand(true)
        .hexpand(true)
        .child(content)
        .build()
}