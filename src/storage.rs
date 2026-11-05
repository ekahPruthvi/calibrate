use gtk4::prelude::*;
use gtk4::{
    Align, Box as GtkBox, Button, CheckButton, Frame, Image, Label, Orientation, Overlay,
    ProgressBar, Revealer, RevealerTransitionType, ScrolledWindow, Separator, gio, glib,
};
use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::fs;
use std::os::unix::fs::MetadataExt;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::rc::Rc;
use std::time::{Duration, SystemTime};

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

fn human(bytes: u64) -> String {
    const UNITS: [&str; 5] = ["B", "KB", "MB", "GB", "TB"];
    let mut v = bytes as f64;
    let mut i = 0;
    while v >= 1000.0 && i < UNITS.len() - 1 {
        v /= 1000.0;
        i += 1;
    }
    if i == 0 {
        format!("{bytes} B")
    } else if v >= 100.0 {
        format!("{:.0} {}", v, UNITS[i])
    } else {
        format!("{:.1} {}", v, UNITS[i])
    }
}

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

fn clear_children(b: &GtkBox) {
    while let Some(c) = b.first_child() {
        b.remove(&c);
    }
}

fn caption(text: &str) -> Label {
    let l = Label::new(Some(text));
    l.add_css_class("row-caption");
    l.set_halign(Align::Start);
    l.set_xalign(0.0);
    l.set_wrap(true);
    l
}

fn have(cmd: &str) -> bool {
    std::env::var_os("PATH")
        .map(|p| std::env::split_paths(&p).any(|d| d.join(cmd).is_file()))
        .unwrap_or(false)
}

fn home_dir() -> Option<PathBuf> {
    std::env::var_os("HOME")
        .map(PathBuf::from)
        .filter(|p| p.as_os_str().len() > 1)
}

/// Disk space actually used on disk (blocks), recursive, never follows symlinks.
fn dir_size(p: &Path) -> u64 {
    let Ok(md) = fs::symlink_metadata(p) else { return 0 };
    let mut total = md.blocks() * 512;
    if md.is_dir() {
        if let Ok(rd) = fs::read_dir(p) {
            for e in rd.flatten() {
                total += dir_size(&e.path());
            }
        }
    }
    total
}

/// Removes everything *inside* `dir` (keeps `dir` itself).
fn clear_dir_contents(dir: &Path) {
    let Ok(rd) = fs::read_dir(dir) else { return };
    for e in rd.flatten() {
        let p = e.path();
        let Ok(md) = fs::symlink_metadata(&p) else { continue };
        if md.is_dir() {
            let _ = fs::remove_dir_all(&p);
        } else {
            let _ = fs::remove_file(&p);
        }
    }
}

// ───────────────────────────── disks backend ─────────────────────────────

struct Part {
    name: String,
    label: String,
    fstype: String,
    mount: Option<String>,
    size: u64,
    /// (used, total) of the mounted filesystem
    usage: Option<(u64, u64)>,
}

struct Disk {
    name: String,
    title: String,
    kind: &'static str,
    size: u64,
    parts: Vec<Part>,
}

fn unescape(s: &str) -> String {
    let b = s.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'\\' && i + 3 < b.len() && b[i + 1] == b'x' {
            let v = std::str::from_utf8(&b[i + 2..i + 4])
                .ok()
                .and_then(|h| u8::from_str_radix(h, 16).ok());
            if let Some(v) = v {
                out.push(v);
                i += 4;
                continue;
            }
        }
        out.push(b[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).to_string()
}

/// Parses one `lsblk -P` line:  KEY="value" KEY2="value 2" …
fn parse_pairs(line: &str) -> HashMap<String, String> {
    let mut map = HashMap::new();
    let mut rest = line.trim();
    while let Some(eq) = rest.find("=\"") {
        let key = rest[..eq].trim().to_string();
        let after = &rest[eq + 2..];
        let Some(end) = after.find('"') else { break };
        map.insert(key, unescape(&after[..end]));
        rest = after[end + 1..].trim_start();
    }
    map
}

fn fs_usage(mount: &str) -> Option<(u64, u64)> {
    use std::ffi::CString;
    let c = CString::new(mount).ok()?;
    let mut s: libc::statvfs = unsafe { std::mem::zeroed() };
    if unsafe { libc::statvfs(c.as_ptr(), &mut s) } != 0 {
        return None;
    }
    let frsize = s.f_frsize as u64;
    let total_blocks = s.f_blocks as u64;
    if total_blocks == 0 {
        return None;
    }
    let free = s.f_bfree as u64 * frsize;
    let avail = s.f_bavail as u64 * frsize;
    let used = (total_blocks * frsize).saturating_sub(free);
    Some((used, used + avail))
}

struct Dev {
    name: String,
    kind: String,
    size: u64,
    model: String,
    mount: String,
    fstype: String,
    label: String,
    rota: bool,
    rm: bool,
    tran: String,
    pkname: String,
}

fn load_disks() -> Vec<Disk> {
    let out = Command::new("lsblk")
        .env("LC_ALL", "C")
        .args(["-P", "-b", "-o", "NAME,TYPE,SIZE,MODEL,MOUNTPOINT,FSTYPE,LABEL,ROTA,RM,TRAN,PKNAME"])
        .output();
    let Ok(out) = out else { return Vec::new() };
    let text = String::from_utf8_lossy(&out.stdout).to_string();

    let devs: Vec<Dev> = text
        .lines()
        .map(parse_pairs)
        .filter(|m| !m.is_empty())
        .map(|m| {
            let g = |k: &str| m.get(k).cloned().unwrap_or_default();
            Dev {
                name: g("NAME"),
                kind: g("TYPE"),
                size: g("SIZE").parse().unwrap_or(0),
                model: g("MODEL").trim().to_string(),
                mount: g("MOUNTPOINT"),
                fstype: g("FSTYPE"),
                label: g("LABEL"),
                rota: g("ROTA") == "1",
                rm: g("RM") == "1",
                tran: g("TRAN"),
                pkname: g("PKNAME"),
            }
        })
        .collect();

    let by_name: HashMap<&str, &Dev> = devs.iter().map(|d| (d.name.as_str(), d)).collect();

    let root_of = |d: &Dev| -> String {
        let mut cur = d;
        for _ in 0..8 {
            if cur.pkname.is_empty() {
                break;
            }
            match by_name.get(cur.pkname.as_str()) {
                Some(p) => cur = p,
                None => break,
            }
        }
        cur.name.clone()
    };

    let make_part = |d: &Dev| -> Part {
        let mount = if d.mount.is_empty() { None } else { Some(d.mount.clone()) };
        let usage = mount
            .as_deref()
            .filter(|m| m.starts_with('/'))
            .and_then(fs_usage);
        Part {
            name: d.name.clone(),
            label: d.label.clone(),
            fstype: d.fstype.clone(),
            mount,
            size: d.size,
            usage,
        }
    };

    let mut disks = Vec::new();
    for d in devs.iter().filter(|d| d.kind == "disk") {
        if d.name.starts_with("zram") || d.name.starts_with("ram") {
            continue;
        }
        let kind = if d.tran == "nvme" || d.name.starts_with("nvme") {
            "NVMe"
        } else if d.tran == "usb" || d.rm {
            "USB"
        } else if d.rota {
            "HDD"
        } else {
            "SSD"
        };

        let mut parts: Vec<Part> = devs
            .iter()
            .filter(|p| !matches!(p.kind.as_str(), "disk" | "loop" | "rom") && root_of(p) == d.name)
            .map(&make_part)
            .collect();
        // a whole-disk filesystem with no partition table
        if parts.is_empty() && (!d.fstype.is_empty() || !d.mount.is_empty()) {
            parts.push(make_part(d));
        }

        let title = if d.model.is_empty() { d.name.clone() } else { d.model.clone() };
        disks.push(Disk { name: d.name.clone(), title, kind, size: d.size, parts });
    }
    disks
}

// ───────────────────────────── disks card ─────────────────────────────

fn usage_bar(fraction: f64) -> ProgressBar {
    let bar = ProgressBar::new();
    bar.add_css_class("storage-bar");
    bar.set_fraction(fraction.clamp(0.0, 1.0));
    if fraction >= 0.90 {
        bar.add_css_class("storage-bar-crit");
    } else if fraction >= 0.80 {
        bar.add_css_class("storage-bar-warn");
    }
    bar.set_hexpand(true);
    bar
}

fn build_part_row(p: &Part) -> GtkBox {
    let row = GtkBox::new(Orientation::Vertical, 4);
    row.set_margin_start(30);

    let top = GtkBox::new(Orientation::Horizontal, 8);
    let name = if p.label.is_empty() { p.name.clone() } else { p.label.clone() };
    let name_lbl = Label::new(Some(&name));
    name_lbl.add_css_class("row-label");
    name_lbl.set_halign(Align::Start);
    name_lbl.set_xalign(0.0);
    name_lbl.set_hexpand(true);
    name_lbl.set_ellipsize(gtk4::pango::EllipsizeMode::End);

    let mount_lbl = Label::new(Some(p.mount.as_deref().unwrap_or("Not mounted")));
    mount_lbl.add_css_class("row-caption");
    mount_lbl.set_halign(Align::End);
    mount_lbl.set_selectable(true);

    top.append(&name_lbl);
    top.append(&mount_lbl);
    row.append(&top);

    let fs_part = if p.fstype.is_empty() { String::new() } else { format!(" · {}", p.fstype) };
    match p.usage {
        Some((used, total)) if total > 0 => {
            let frac = used as f64 / total as f64;
            row.append(&usage_bar(frac));
            row.append(&caption(&format!(
                "{} used of {} · {} free · {:.0}%  ({}{})",
                human(used),
                human(total),
                human(total.saturating_sub(used)),
                frac * 100.0,
                p.name,
                fs_part
            )));
        }
        _ => {
            let note = if p.mount.as_deref() == Some("[SWAP]") { "Swap" } else { "No usage data" };
            row.append(&caption(&format!("{} · {}  ({}{})", human(p.size), note, p.name, fs_part)));
        }
    }
    row
}

fn build_disk_block(d: &Disk) -> GtkBox {
    let block = GtkBox::new(Orientation::Vertical, 8);

    let head = GtkBox::new(Orientation::Horizontal, 12);
    let icon = Image::from_file("/var/lib/cynager/icons/disk.svg");
    icon.set_pixel_size(28);
    icon.add_css_class("card-icons-smol");
    icon.set_valign(Align::Center);

    let texts = GtkBox::new(Orientation::Vertical, 2);
    texts.set_hexpand(true);
    let title = Label::new(Some(&d.title));
    title.add_css_class("row-label");
    title.set_halign(Align::Start);
    title.set_xalign(0.0);
    title.set_ellipsize(gtk4::pango::EllipsizeMode::End);
    texts.append(&title);
    texts.append(&caption(&format!("/dev/{} · {}", d.name, human(d.size))));

    let badge = Label::new(Some(d.kind));
    badge.add_css_class("storage-badge");
    badge.set_valign(Align::Center);

    head.append(&icon);
    head.append(&texts);
    head.append(&badge);
    block.append(&head);

    // whole-disk summary
    let used: u64 = d.parts.iter().filter_map(|p| p.usage.map(|u| u.0)).sum();
    if d.parts.iter().any(|p| p.usage.is_some()) && d.size > 0 {
        block.append(&usage_bar(used as f64 / d.size as f64));
        block.append(&caption(&format!("{} used of {}", human(used), human(d.size))));
    }

    if d.parts.is_empty() {
        let l = caption("No partitions");
        l.set_margin_start(30);
        block.append(&l);
    }
    for p in &d.parts {
        block.append(&build_part_row(p));
    }
    block
}

/// Returns the card and a function that reloads it.
fn build_disks_card() -> (Frame, Rc<dyn Fn()>) {
    let frame = Frame::new(None);
    let outer = GtkBox::new(Orientation::Vertical, 12);
    outer.set_margin_top(20);
    outer.set_margin_bottom(20);
    outer.set_margin_start(20);
    outer.set_margin_end(20);

    let header = GtkBox::new(Orientation::Horizontal, 12);

    let refresh_btn = Button::with_label("Refresh");
    refresh_btn.add_css_class("sub-btn");
    refresh_btn.add_css_class("sub-btn-small");
    refresh_btn.set_halign(Align::End);
    refresh_btn.set_hexpand(true);
    refresh_btn.set_cursor_from_name(Some("pointer"));

    header.append(&refresh_btn);

    let body = GtkBox::new(Orientation::Vertical, 14);

    outer.append(&header);
    outer.append(&body);
    frame.set_child(Some(&outer));

    let refresh: Rc<dyn Fn()> = {
        let body = body.clone();
        Rc::new(move || {
            let body = body.clone();
            glib::spawn_future_local(async move {
                let Ok(disks) = gio::spawn_blocking(load_disks).await else { return };
                clear_children(&body);
                if disks.is_empty() {
                    body.append(&caption("No disks found (is lsblk installed?)"));
                    return;
                }
                let n = disks.len();
                for (i, d) in disks.iter().enumerate() {
                    body.append(&build_disk_block(d));
                    if i + 1 != n {
                        body.append(&Separator::new(Orientation::Horizontal));
                    }
                }
            });
        })
    };

    {
        let refresh = refresh.clone();
        refresh_btn.connect_clicked(move |_| refresh());
    }
    {
        let refresh = refresh.clone();
        frame.connect_map(move |_| refresh());
    }

    (frame, refresh)
}

// ───────────────────────────── cleanup backend ─────────────────────────────

#[derive(Clone, Copy, PartialEq)]
enum Group {
    System,
    Apps,
}

#[derive(Clone)]
enum Action {
    /// delete the contents of these folders (always inside $HOME)
    Dirs(Vec<PathBuf>),
    /// old files owned by the user in /tmp and /var/tmp
    OldTmp,
    /// run as the current user
    Cmd(Vec<String>),
    /// run through pkexec (asks for the password)
    Priv(Vec<String>),
}

#[derive(Clone)]
struct Item {
    title: String,
    desc: String,
    /// None = size can't be known beforehand
    size: Option<u64>,
    action: Action,
    group: Group,
}

const TMP_MAX_AGE_DAYS: u64 = 7;
const MIN_APP_CACHE: u64 = 1_000_000;
const MAX_APP_ROWS: usize = 12;

fn uid() -> u32 {
    unsafe { libc::geteuid() }
}

fn tmp_cutoff() -> SystemTime {
    SystemTime::now()
        .checked_sub(Duration::from_secs(TMP_MAX_AGE_DAYS * 86_400))
        .unwrap_or(SystemTime::UNIX_EPOCH)
}

/// Walks a temp dir. Only plain files/symlinks that the user owns and that
/// haven't been touched for a week are counted/removed — sockets, pipes and
/// anything fresh (live sessions) are left alone.
fn tmp_walk(dir: &Path, cutoff: SystemTime, uid: u32, delete: bool) -> u64 {
    let mut total = 0;
    let Ok(rd) = fs::read_dir(dir) else { return 0 };
    for e in rd.flatten() {
        let p = e.path();
        let Ok(md) = fs::symlink_metadata(&p) else { continue };
        if md.uid() != uid {
            continue;
        }
        let ft = md.file_type();
        let old_m = md.modified().map(|m| m < cutoff).unwrap_or(false);
        if ft.is_dir() {
            total += tmp_walk(&p, cutoff, uid, delete);
            if delete && old_m {
                let _ = fs::remove_dir(&p); // only succeeds when empty
            }
        } else if ft.is_file() || ft.is_symlink() {
            let old_a = md.accessed().map(|a| a < cutoff).unwrap_or(true);
            if old_m && old_a {
                total += md.blocks() * 512;
                if delete {
                    let _ = fs::remove_file(&p);
                }
            }
        }
    }
    total
}

fn tmp_total(delete: bool) -> u64 {
    let c = tmp_cutoff();
    let u = uid();
    ["/tmp", "/var/tmp"]
        .iter()
        .map(|d| tmp_walk(Path::new(d), c, u, delete))
        .sum()
}

fn dirs_item(group: Group, title: &str, desc: &str, paths: Vec<PathBuf>) -> Option<Item> {
    let size: u64 = paths.iter().map(|p| dir_size(p)).sum();
    // an empty folder still costs 4 KB of its own — treat that as nothing
    if size <= 16_384 {
        return None;
    }
    Some(Item {
        title: title.to_string(),
        desc: desc.to_string(),
        size: Some(size),
        action: Action::Dirs(paths),
        group,
    })
}

fn out_of(cmd: &str, args: &[&str]) -> Option<String> {
    let o = Command::new(cmd).env("LC_ALL", "C").args(args).output().ok()?;
    Some(String::from_utf8_lossy(&o.stdout).to_string())
}

/// "…take up 1.2G in the file systems." → bytes
fn journal_size() -> Option<u64> {
    let out = out_of("journalctl", &["--disk-usage"])?;
    let tok = out.split("take up ").nth(1)?.split_whitespace().next()?;
    let unit = tok.chars().last()?;
    let mult: f64 = match unit {
        'K' => 1024.0,
        'M' => 1024.0 * 1024.0,
        'G' => 1024.0 * 1024.0 * 1024.0,
        'T' => 1024.0f64.powi(4),
        _ => 1.0,
    };
    let num: f64 = tok.trim_end_matches(|c: char| c.is_alphabetic()).parse().ok()?;
    Some((num * mult) as u64)
}

fn scan_items() -> Vec<Item> {
    let mut items: Vec<Item> = Vec::new();
    let home = home_dir();

    // ───── system & files ─────
    if let Some(h) = &home {
        if let Some(i) = dirs_item(
            Group::System,
            "Trash",
            "Files you deleted but didn't empty yet.",
            vec![h.join(".local/share/Trash/files"), h.join(".local/share/Trash/info")],
        ) {
            items.push(i);
        }
        if let Some(i) = dirs_item(
            Group::System,
            "Thumbnail cache",
            "Preview images. Regenerated when needed.",
            vec![h.join(".cache/thumbnails"), h.join(".thumbnails")],
        ) {
            items.push(i);
        }
    }

    let tmp = tmp_total(false);
    if tmp > 16_384 {
        items.push(Item {
            title: "Temporary files".into(),
            desc: format!("Your files in /tmp and /var/tmp untouched for {TMP_MAX_AGE_DAYS}+ days."),
            size: Some(tmp),
            action: Action::OldTmp,
            group: Group::System,
        });
    }

    // package manager
    if have("pacman") {
        let sz = dir_size(Path::new("/var/cache/pacman/pkg"));
        if sz > 16_384 {
            items.push(Item {
                title: "Package cache".into(),
                desc: "Downloaded package files kept by pacman. Needs your password.".into(),
                size: Some(sz),
                action: Action::Priv(vec!["sh".into(), "-c".into(), "yes | pacman -Scc".into()]),
                group: Group::System,
            });
        }
        let orphans = out_of("pacman", &["-Qdtq"])
            .map(|s| s.lines().filter(|l| !l.trim().is_empty()).count())
            .unwrap_or(0);
        if orphans > 0 {
            items.push(Item {
                title: "Unused packages".into(),
                desc: format!("{orphans} package(s) no longer needed by anything. Needs your password."),
                size: None,
                action: Action::Priv(vec![
                    "sh".into(),
                    "-c".into(),
                    "o=$(pacman -Qdtq); [ -z \"$o\" ] || pacman -Rns --noconfirm $o".into(),
                ]),
                group: Group::System,
            });
        }
    } else if have("apt-get") {
        let sz = dir_size(Path::new("/var/cache/apt/archives"));
        if sz > 16_384 {
            items.push(Item {
                title: "Package cache".into(),
                desc: "Downloaded .deb files kept by apt. Needs your password.".into(),
                size: Some(sz),
                action: Action::Priv(vec!["apt-get".into(), "clean".into()]),
                group: Group::System,
            });
        }
        let n = out_of("apt-get", &["-s", "autoremove"])
            .map(|s| s.lines().filter(|l| l.starts_with("Remv")).count())
            .unwrap_or(0);
        if n > 0 {
            items.push(Item {
                title: "Unused packages".into(),
                desc: format!("{n} package(s) no longer needed by anything. Needs your password."),
                size: None,
                action: Action::Priv(vec!["apt-get".into(), "autoremove".into(), "-y".into()]),
                group: Group::System,
            });
        }
    } else if have("dnf") {
        let sz = dir_size(Path::new("/var/cache/dnf")) + dir_size(Path::new("/var/cache/libdnf5"));
        if sz > 16_384 {
            items.push(Item {
                title: "Package cache".into(),
                desc: "Metadata and packages cached by dnf. Needs your password.".into(),
                size: Some(sz),
                action: Action::Priv(vec!["dnf".into(), "clean".into(), "all".into()]),
                group: Group::System,
            });
        }
    }

    if have("journalctl") {
        if let Some(sz) = journal_size().filter(|s| *s > 50_000_000) {
            items.push(Item {
                title: "System logs".into(),
                desc: "Old journal entries. Keeps the last 7 days. Needs your password.".into(),
                size: Some(sz),
                action: Action::Priv(vec!["journalctl".into(), "--vacuum-time=7d".into()]),
                group: Group::System,
            });
        }
    }

    if have("flatpak") {
        items.push(Item {
            title: "Unused Flatpak runtimes".into(),
            desc: "Runtimes left behind by removed Flatpak apps.".into(),
            size: None,
            action: Action::Cmd(vec![
                "flatpak".into(),
                "uninstall".into(),
                "--unused".into(),
                "--user".into(),
                "-y".into(),
            ]),
            group: Group::System,
        });
    }

    // ───── left behind by apps ─────
    if let Some(h) = &home {
        let mut apps: Vec<Item> = Vec::new();

        // ~/.cache/<app>
        if let Ok(rd) = fs::read_dir(h.join(".cache")) {
            for e in rd.flatten() {
                let p = e.path();
                let name = e.file_name().to_string_lossy().to_string();
                if name == "thumbnails" || !p.is_dir() {
                    continue;
                }
                let sz = dir_size(&p);
                if sz >= MIN_APP_CACHE {
                    apps.push(Item {
                        title: name,
                        desc: format!("~/.cache/{}", e.file_name().to_string_lossy()),
                        size: Some(sz),
                        action: Action::Dirs(vec![p]),
                        group: Group::Apps,
                    });
                }
            }
        }

        // Flatpak app caches
        if let Ok(rd) = fs::read_dir(h.join(".var/app")) {
            for e in rd.flatten() {
                let p = e.path().join("cache");
                if !p.is_dir() {
                    continue;
                }
                let sz = dir_size(&p);
                if sz >= MIN_APP_CACHE {
                    apps.push(Item {
                        title: e.file_name().to_string_lossy().to_string(),
                        desc: "Flatpak app cache".into(),
                        size: Some(sz),
                        action: Action::Dirs(vec![p]),
                        group: Group::Apps,
                    });
                }
            }
        }

        // developer download caches
        for (title, desc, rel) in [
            ("npm cache", "Downloaded npm packages (~/.npm/_cacache)", ".npm/_cacache"),
            ("Cargo download cache", "Downloaded crates (~/.cargo/registry/cache)", ".cargo/registry/cache"),
        ] {
            let p = h.join(rel);
            let sz = dir_size(&p);
            if p.is_dir() && sz >= MIN_APP_CACHE {
                apps.push(Item {
                    title: title.into(),
                    desc: desc.into(),
                    size: Some(sz),
                    action: Action::Dirs(vec![p]),
                    group: Group::Apps,
                });
            }
        }

        apps.sort_by(|a, b| b.size.cmp(&a.size));
        apps.truncate(MAX_APP_ROWS);
        items.extend(apps);
    }

    items
}

/// Executes one cleanup item. Returns the bytes freed (best effort).
fn run_action(item: &Item) -> Result<u64, String> {
    match &item.action {
        Action::Dirs(paths) => {
            let home = home_dir().ok_or("HOME is not set")?;
            let mut before = 0;
            for p in paths {
                if !p.starts_with(&home) || *p == home {
                    return Err(format!("refusing to touch {}", p.display()));
                }
                before += dir_size(p);
            }
            for p in paths {
                clear_dir_contents(p);
            }
            let after: u64 = paths.iter().map(|p| dir_size(p)).sum();
            Ok(before.saturating_sub(after))
        }
        Action::OldTmp => Ok(tmp_total(true)),
        Action::Cmd(argv) | Action::Priv(argv) => {
            let privileged = matches!(item.action, Action::Priv(_));
            let mut cmd = if privileged {
                if !have("pkexec") {
                    return Err("pkexec is not installed".into());
                }
                let mut c = Command::new("pkexec");
                c.args(argv);
                c
            } else {
                let mut c = Command::new(&argv[0]);
                c.args(&argv[1..]);
                c
            };
            let out = cmd.output().map_err(|e| format!("cannot run command: {e}"))?;
            if out.status.success() {
                Ok(item.size.unwrap_or(0))
            } else {
                let err = String::from_utf8_lossy(&out.stderr).trim().to_string();
                Err(if err.is_empty() { "cancelled or failed".into() } else { err })
            }
        }
    }
}

// ───────────────────────────── cleanup card ─────────────────────────────

type Rows = Rc<RefCell<Vec<(CheckButton, Item)>>>;

fn make_item_row(item: &Item, rows: &Rows, update: &Rc<dyn Fn()>, list: &GtkBox) {
    let row = GtkBox::new(Orientation::Horizontal, 12);
    row.set_margin_top(4);
    row.set_margin_bottom(4);

    let check = CheckButton::new();
    check.set_active(true);
    check.set_valign(Align::Center);
    check.set_cursor_from_name(Some("pointer"));

    let texts = GtkBox::new(Orientation::Vertical, 2);
    texts.set_hexpand(true);
    let t = Label::new(Some(&item.title));
    t.add_css_class("row-label");
    t.set_halign(Align::Start);
    t.set_xalign(0.0);
    t.set_ellipsize(gtk4::pango::EllipsizeMode::End);
    texts.append(&t);
    texts.append(&caption(&item.desc));

    let size = Label::new(Some(&item.size.map(human).unwrap_or_else(|| "—".into())));
    size.add_css_class("row-label");
    size.set_valign(Align::Center);

    row.append(&check);
    row.append(&texts);
    row.append(&size);
    list.append(&row);

    {
        let update = update.clone();
        check.connect_toggled(move |_| update());
    }
    rows.borrow_mut().push((check, item.clone()));
}

fn group_heading(text: &str) -> Label {
    let l = Label::new(Some(text));
    l.add_css_class("frame-title");
    l.set_halign(Align::Start);
    l.set_margin_top(8);
    l
}

fn build_cleanup_card(toast: &Toast, on_done: Rc<dyn Fn()>) -> Frame {
    let (frame, inner) = section(
        "Cleanup",
        "Remove files you don't need and what apps leave behind. Caches are rebuilt \
         automatically the next time an app runs, so close apps before cleaning theirs.",
    );

    let summary = Label::new(Some("Scanning…"));
    summary.add_css_class("row-caption");
    summary.set_halign(Align::Start);
    summary.set_xalign(0.0);

    let list = GtkBox::new(Orientation::Vertical, 2);

    let actions = GtkBox::new(Orientation::Horizontal, 8);
    actions.set_halign(Align::End);
    actions.set_margin_top(8);

    let rescan = Button::with_label("Rescan");
    rescan.add_css_class("sub-btn");
    rescan.set_cursor_from_name(Some("pointer"));

    let clean_btn = Button::with_label("Clean selected");
    clean_btn.add_css_class("destructive-action");
    clean_btn.set_cursor_from_name(Some("pointer"));
    clean_btn.set_sensitive(false);

    actions.append(&rescan);
    actions.append(&clean_btn);

    inner.append(&summary);
    inner.append(&list);
    inner.append(&actions);

    let rows: Rows = Rc::new(RefCell::new(Vec::new()));
    let busy = Rc::new(Cell::new(false));
    let armed = Rc::new(Cell::new(false));

    // selected count / total
    let update: Rc<dyn Fn()> = {
        let rows = rows.clone();
        let summary = summary.clone();
        let clean_btn = clean_btn.clone();
        let busy = busy.clone();
        Rc::new(move || {
            let r = rows.borrow();
            let (n, bytes) = r
                .iter()
                .filter(|(c, _)| c.is_active())
                .fold((0usize, 0u64), |(n, b), (_, it)| (n + 1, b + it.size.unwrap_or(0)));
            if r.is_empty() {
                summary.set_text("Everything is already clean.");
            } else if n == 0 {
                summary.set_text("Nothing selected");
            } else {
                summary.set_text(&format!("{n} selected · about {} to free", human(bytes)));
            }
            clean_btn.set_sensitive(n > 0 && !busy.get());
        })
    };

    let refresh: Rc<dyn Fn()> = {
        let rows = rows.clone();
        let list = list.clone();
        let summary = summary.clone();
        let update = update.clone();
        let clean_btn = clean_btn.clone();
        let busy = busy.clone();
        Rc::new(move || {
            if busy.get() {
                return;
            }
            busy.set(true);
            summary.set_text("Scanning…");
            clean_btn.set_sensitive(false);
            clear_children(&list);
            rows.borrow_mut().clear();

            let rows = rows.clone();
            let list = list.clone();
            let update = update.clone();
            let busy = busy.clone();
            glib::spawn_future_local(async move {
                let items = gio::spawn_blocking(scan_items).await.unwrap_or_default();

                for (group, heading) in [(Group::System, "System and files"), (Group::Apps, "Left behind by apps")] {
                    let part: Vec<&Item> = items.iter().filter(|i| i.group == group).collect();
                    if part.is_empty() {
                        continue;
                    }
                    list.append(&group_heading(heading));
                    for it in part {
                        make_item_row(it, &rows, &update, &list);
                    }
                }
                busy.set(false);
                update();
            });
        })
    };

    {
        let refresh = refresh.clone();
        rescan.connect_clicked(move |_| refresh());
    }
    {
        let refresh = refresh.clone();
        frame.connect_map(move |_| refresh());
    }

    // clean — click twice to confirm
    {
        let rows = rows.clone();
        let toast = toast.clone();
        let busy = busy.clone();
        let armed = armed.clone();
        let refresh = refresh.clone();
        clean_btn.connect_clicked(move |btn| {
            let chosen: Vec<Item> = rows
                .borrow()
                .iter()
                .filter(|(c, _)| c.is_active())
                .map(|(_, it)| it.clone())
                .collect();
            if chosen.is_empty() {
                toast.show("Nothing selected");
                return;
            }

            if !armed.get() {
                armed.set(true);
                let total: u64 = chosen.iter().map(|i| i.size.unwrap_or(0)).sum();
                btn.set_label(&format!("Click again to delete ~{}", human(total)));
                let btn = btn.clone();
                let armed = armed.clone();
                glib::timeout_add_local_once(Duration::from_secs(4), move || {
                    if armed.get() {
                        armed.set(false);
                        btn.set_label("Clean selected");
                    }
                });
                return;
            }
            armed.set(false);

            busy.set(true);
            btn.set_sensitive(false);
            btn.set_label("Cleaning…");

            let btn = btn.clone();
            let toast = toast.clone();
            let busy = busy.clone();
            let refresh = refresh.clone();
            let on_done = on_done.clone();
            glib::spawn_future_local(async move {
                let res = gio::spawn_blocking(move || {
                    let mut freed = 0u64;
                    let mut errs: Vec<String> = Vec::new();
                    for it in chosen {
                        match run_action(&it) {
                            Ok(n) => freed += n,
                            Err(e) => errs.push(format!("{}: {e}", it.title)),
                        }
                    }
                    (freed, errs)
                })
                .await;

                btn.set_label("Clean selected");
                busy.set(false);
                match res {
                    Ok((freed, errs)) if errs.is_empty() => {
                        toast.show(&format!("Cleanup done — freed about {}", human(freed)))
                    }
                    Ok((_, _)) => toast.show(&format!(
                        "Failed to remove Some Files"
                    )),
                    Err(_) => toast.show("Cleanup failed"),
                }
                refresh();
                on_done();
            });
        });
    }

    frame
}

// ───────────────────────────── page ─────────────────────────────

pub fn build_storage_page() -> Overlay {
    let content = GtkBox::new(Orientation::Vertical, 16);

    // header card
    let card = GtkBox::new(Orientation::Vertical, 10);
    card.add_css_class("shortcut-card");
    card.add_css_class("detail-card");
    card.set_margin_top(10);
    card.set_hexpand(true);

    let infoicon = Image::from_file("/var/lib/cynager/icons/disk.svg");
    infoicon.set_pixel_size(54);
    infoicon.set_halign(Align::Center);
    infoicon.set_css_classes(&["card-icons"]);

    let title = Label::new(Some("Storage"));
    title.add_css_class("shortcut-title");
    title.set_halign(Align::Center);

    let subtitle = Label::new(Some("See where your space goes and free it up."));
    subtitle.add_css_class("shortcut-desc");
    subtitle.set_halign(Align::Center);

    card.append(&infoicon);
    card.append(&title);
    card.append(&subtitle);
    content.append(&card);

    let toast = Toast::new();

    // card 1 — disks
    let disktithead = GtkBox::new(Orientation::Vertical, 2);
    disktithead.set_hexpand(true);
    disktithead.set_margin_top(20);
    disktithead.set_margin_start(20);
    let title = Label::new(Some("Disks"));
    title.add_css_class("frame-title");
    title.set_halign(Align::Start);
    let sub = Label::new(Some("See mounted disks and available storage."));
    sub.add_css_class("frame-subtitle");
    sub.set_halign(Align::Start);
    disktithead.append(&title);
    disktithead.append(&sub);

    content.append(&disktithead);
    let (disks_card, reload_disks) = build_disks_card();
    content.append(&disks_card);

    // card 2 — cleanup (reloads the disk usage afterwards)
    content.append(&build_cleanup_card(&toast, reload_disks));

    let overlay = Overlay::new();
    let scroller: ScrolledWindow = page_scroller(&content);
    overlay.set_child(Some(&scroller));
    overlay.add_overlay(&toast.revealer);
    overlay
}