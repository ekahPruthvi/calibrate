use gtk4::prelude::*;
use gtk4::{
    ApplicationWindow, Box as GtkBox, Label,
    Orientation, ScrolledWindow, Stack, Frame, Align, DrawingArea, gdk_pixbuf::Pixbuf,
    Button, Dialog, DropTarget, StackSwitcher, DropDown, gio, gdk, glib,
};
use std::fs;
use std::{rc::Rc, path::PathBuf};

use niri_ipc::{Request, Response, socket::Socket};
use infoprober::{parse, Entry, Value};
use xcursor::{CursorTheme, parser::parse_xcursor};

use gtk4:: gdk_pixbuf::{InterpType};
use std::cell::RefCell;

use crate::home::{rounded_rect, page_scroller};

const CFGPATH: &str = "/var/lib/cynager/info.probe";
const NIRI_CFGPATH: &str = "/var/lib/cynager/niri/config.kdl";

fn get_monitors() -> Vec<(String, String)> {
    let mut socket = Socket::connect().expect("[calibrate] cannot connect to niri socket");

    let reply = socket.send(Request::Outputs).expect("[calibrate] request to niri error");

    let mut out :Vec<(String, String)>= vec![];
    match reply {
        Ok(Response::Outputs(outputs)) => {
            for (name, outt) in outputs {
                out.push((name, outt.make));
            }
        }

        Ok(response) => {
            eprintln!("[calibrate] Unexpected response: {response:?}");
        }

        Err(err) => {
            eprintln!("[calibrate] Niri IPC error: {err:?}");
        }
    }

    return out
}

fn build_round_wallpaper(path: &str, radius: f64) -> DrawingArea {
    let area = DrawingArea::new();
    area.set_hexpand(true);
    area.set_vexpand(true);
    area.set_content_width(150);
    area.set_content_height(84);

    let pixbuf = Pixbuf::from_file(path).ok();
    let cache: RefCell<Option<((i32, i32), Pixbuf)>> = RefCell::new(None);

    area.set_draw_func(move |_, cr, w, h| {
        let (wf, hf) = (w as f64, h as f64);
        let r = radius.min(wf / 2.0).min(hf / 2.0);

        rounded_rect(cr, 0.0, 0.0, wf, hf, r);
        cr.clip();

        if let Some(pb) = &pixbuf {
            let (pw, ph) = (pb.width() as f64, pb.height() as f64);
            let s = (wf / pw).max(hf / ph);
            let (sw, sh) = ((pw * s).ceil() as i32, (ph * s).ceil() as i32);

            let mut c = cache.borrow_mut();
            if c.as_ref().map(|(k, _)| *k) != Some((sw, sh)) {
                *c = pb
                    .scale_simple(sw, sh, InterpType::Bilinear)
                    .map(|p| ((sw, sh), p));
            }
            if let Some((_, scaled)) = c.as_ref() {
                let dx = (wf - sw as f64) / 2.0;
                let dy = (hf - sh as f64) / 2.0;
                cr.set_source_pixbuf(scaled, dx, dy);
                cr.paint().unwrap();
            }
        }

        let shine = gtk4::cairo::LinearGradient::new(0.0, 0.0, 0.0, hf * 0.6);
        shine.add_color_stop_rgba(0.0, 1.0, 1.0, 1.0, 0.25);
        shine.add_color_stop_rgba(1.0, 1.0, 1.0, 1.0, 0.0);
        cr.set_source(&shine).unwrap();
        cr.paint().unwrap();

        cr.reset_clip();
        rounded_rect(cr, 0.5, 0.5, wf - 1.0, hf - 1.0, (r - 0.5).max(0.0));
        cr.set_source_rgba(1.0, 1.0, 1.0, 0.25);
        cr.set_line_width(1.0);
        cr.stroke().unwrap();
    });

    area
}

fn refresh_wall_gallery(wallbox: &GtkBox, monwalls: &Stack, gallery_holder: &Rc<RefCell<ScrolledWindow>>) {
    let old_gallery = gallery_holder.borrow().clone();
    wallbox.remove(&old_gallery);

    let new_gallery = build_wall_gallery(monwalls, wallbox, gallery_holder);
    wallbox.append(&new_gallery);

    *gallery_holder.borrow_mut() = new_gallery;
}

fn build_wall_gallery(
    monwalls: &Stack,
    wallbox: &GtkBox,
    gallery_holder: &Rc<RefCell<ScrolledWindow>>,
) -> ScrolledWindow {
    let scroller = ScrolledWindow::builder()
        .hscrollbar_policy(gtk4::PolicyType::Automatic)
        .vscrollbar_policy(gtk4::PolicyType::Never)
        .hexpand(true)
        .height_request(120)
        .margin_top(10)
        .build();

    let row = GtkBox::new(Orientation::Horizontal, 10);
    row.set_margin_top(10);
    row.set_margin_bottom(10);
    row.set_margin_start(10);
    row.set_margin_end(10);

    let home = std::env::var("HOME").unwrap_or_default();
    let walls_dir = format!("{home}/.config/walls");

    let mut paths: Vec<std::path::PathBuf> = fs::read_dir(&walls_dir)
        .into_iter()
        .flatten()
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .filter(|p| p.is_file())
        .collect();
    paths.sort();

    for path in paths {
        let Some(path_str) = path.to_str() else { continue };

        let thumb = build_round_wallpaper(path_str, 10.0);
        thumb.set_content_width(200);
        thumb.set_content_height(120);
        thumb.set_hexpand(false);
        thumb.set_vexpand(false);

        let btn = Button::new();
        btn.set_child(Some(&thumb));
        btn.add_css_class("wall-thumb");
        btn.set_tooltip_text(Some(path_str));
        btn.set_margin_bottom(10);
        btn.set_margin_end(5);
        btn.set_margin_start(5);
        btn.set_margin_top(5);

        let monwalls = monwalls.clone();
        let path_owned = path_str.to_string();
        btn.connect_clicked({
            let monwalls = monwalls.clone();
            let path_owned = path_owned.clone();
            move |_| {
            if let Some(visible_widget) = monwalls.visible_child() {
                if let Some(monitor) = monwalls.visible_child_name() {
                    switch_wall(monitor.as_str(), &path_owned);
                } else {
                    eprintln!("[calibrate] no monitor tab selected, not switching wallpaper");
                }
                if let Ok(visible_box) = visible_widget.downcast::<GtkBox>() {
                    while let Some(child) = visible_box.first_child() {
                        visible_box.remove(&child);
                    }
                    visible_box.set_css_classes(&["wallpaperPrev"]);
                    visible_box.set_hexpand(false);
                    visible_box.set_vexpand(false);
                    visible_box.set_halign(Align::Start);
                    visible_box.set_height_request(200);
                    visible_box.set_width_request(300);
                    visible_box.set_margin_bottom(20);
                    visible_box.set_margin_end(10);
                    visible_box.set_margin_start(10);
                    visible_box.set_margin_top(10);
                    let wallrnimg = build_round_wallpaper(&path_owned, 20.0);
                    wallrnimg.set_tooltip_text(Some(&path_owned));
                    visible_box.append(&wallrnimg);
                }
            }
        }});

        let popover = gtk4::Popover::new();
        popover.set_parent(&btn);
        popover.set_css_classes(&["popoverd"]);
        popover.set_has_arrow(false);
        popover.set_autohide(true);

        let popover_box = GtkBox::new(Orientation::Vertical, 0);
        let delete_item = Button::new();
        delete_item.set_label("Delete Wallpaper");
        delete_item.add_css_class("flat");
        delete_item.add_css_class("destructive-action");
        popover_box.append(&delete_item);
        popover.set_child(Some(&popover_box));

        {
            let popover = popover.clone();
            let path_owned = path_owned.clone();
            let wallbox = wallbox.clone();
            let monwalls = monwalls.clone();
            let gallery_holder = gallery_holder.clone();
            delete_item.connect_clicked(move |_| {
                popover.popdown();
                if let Err(e) = fs::remove_file(&path_owned) {
                    eprintln!("[calibrate] failed to delete wallpaper {}: {e}", path_owned);
                } else {
                    println!("[calibrate] deleted wallpaper {}", path_owned);
                }
                refresh_wall_gallery(&wallbox, &monwalls, &gallery_holder);
            });
        }

        let right_click = gtk4::GestureClick::new();
        right_click.set_button(gdk::BUTTON_SECONDARY);
        {
            let popover = popover.clone();
            right_click.connect_pressed(move |_, _, x, y| {
                popover.set_pointing_to(Some(&gdk::Rectangle::new(
                    x as i32,
                    y as i32,
                    1,
                    1,
                )));
                popover.popup();
            });
        }
        btn.add_controller(right_click);

        row.append(&btn);
    }

    scroller.set_child(Some(&row));
    scroller
}

fn switch_wall(monitor: &str, path: &str) {
    let home = std::env::var("HOME").unwrap_or_default();
    let rel = path.strip_prefix(&home).map(|p| p.trim_start_matches('/')).unwrap_or(path);

    let src = match fs::read_to_string(CFGPATH) {
        Ok(s) => s,
        Err(e) => return eprintln!("[calibrate] failed to read cfg: {e}"),
    };
    let mut doc = match parse(&src) {
        Ok(d) => d,
        Err(e) => return eprintln!("[calibrate] failed to parse cfg: {e}"),
    };

    let set = doc.section_or_insert("set");
    match set.get_mut("walls") {
        Some(Value::Map(walls)) => {
            walls.retain(|e| e.value.as_str() != Some(monitor));
            walls.push(Entry::new(rel.to_string(), monitor.to_string()));
        }
        Some(Value::Str(_)) => return eprintln!("[calibrate] `walls` in cfg is not a map"),
        None => set.push("walls", vec![Entry::new(rel.to_string(), monitor.to_string())]),
    }

    if let Err(e) = doc.validate() {
        return eprintln!("[calibrate] not writing cfg: {e}");
    }
    if let Err(e) = fs::write(CFGPATH, doc.render()) {
        return eprintln!("[calibrate] failed to write cfg: {e}");
    }

    println!("[calibrate] {monitor} -> {rel}");
}

fn get_cursor_themes() -> Vec<String> {
    let mut themes = std::collections::BTreeSet::new();
    let home = std::env::var("HOME").unwrap_or_default();

    let dirs = [
        format!("{home}/.icons"),
        format!("{home}/.local/share/icons"),
        "/usr/share/icons".to_string(),
        "/usr/local/share/icons".to_string(),
    ];

    for dir in dirs {
        let Ok(entries) = fs::read_dir(&dir) else { continue };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() && path.join("cursors").is_dir() {
                if let Some(name) = path.file_name().and_then(|n| n.to_str()) {
                    themes.insert(name.to_string());
                }
            }
        }
    }

    if themes.is_empty() {
        themes.insert("Adwaita".to_string());
    }

    themes.into_iter().collect()
}

fn get_current_cursor_theme() -> String {
    std::process::Command::new("gsettings")
        .args(["get", "org.gnome.desktop.interface", "cursor-theme"])
        .output()
        .ok()
        .and_then(|o| String::from_utf8(o.stdout).ok())
        .map(|s| s.trim().trim_matches('\'').to_string())
        .unwrap_or_else(|| "Adwaita".to_string())
}

fn set_cursor_theme(name: &str) {
    if let Err(e) = std::process::Command::new("gsettings")
        .args(["set", "org.gnome.desktop.interface", "cursor-theme", name])
        .spawn()
    {
        eprintln!("[calibrate] failed to set cursor theme: {e}");
    }
}

fn get_current_cursor_size() -> u32 {
    std::process::Command::new("gsettings")
        .args(["get", "org.gnome.desktop.interface", "cursor-size"])
        .output()
        .ok()
        .and_then(|o| String::from_utf8(o.stdout).ok())
        .and_then(|s| s.trim().parse::<u32>().ok())
        .unwrap_or(24)
}

fn set_cursor_size(size: u32) {
    if let Err(e) = std::process::Command::new("gsettings")
        .args(["set", "org.gnome.desktop.interface", "cursor-size", &size.to_string()])
        .spawn()
    {
        eprintln!("[calibrate] failed to set cursor size: {e}");
    }
}

fn save_niri_cursor_config(theme: &str, size: u32) {
    let src = fs::read_to_string(NIRI_CFGPATH).unwrap_or_default();

    let new_block = format!(
        "cursor {{\n    hide-when-typing\n    xcursor-theme \"{theme}\"\n    xcursor-size {size}\n}}"
    );

    let updated = if let Some(start) = src.find("cursor {") {
        match src[start..].find('}') {
            Some(rel_end) => {
                let end = start + rel_end + 1;
                format!("{}{}{}", &src[..start], new_block, &src[end..])
            }
            None => {
                eprintln!("[calibrate] malformed `cursor` block in niri config, appending instead");
                format!("{src}\n\n{new_block}\n")
            }
        }
    } else {
        let sep = if src.trim().is_empty() { "" } else { "\n\n" };
        format!("{src}{sep}{new_block}\n")
    };

    if let Some(parent) = std::path::Path::new(NIRI_CFGPATH).parent() {
        if let Err(e) = fs::create_dir_all(parent) {
            eprintln!("[calibrate] failed to create niri config dir: {e}");
            return;
        }
    }

    match fs::write(NIRI_CFGPATH, updated) {
        Ok(()) => println!("[calibrate] saved cursor config -> theme={theme} size={size}"),
        Err(e) => eprintln!("[calibrate] failed to write niri config: {e}"),
    }
}

fn load_cursor_pixbuf(theme_name: &str, size: u32) -> Option<Pixbuf> {
    let theme = CursorTheme::load(theme_name);
    let icon_path = theme
        .load_icon("left_ptr")
        .or_else(|| theme.load_icon("default"))?;
 
    let data = fs::read(&icon_path).ok()?;
    let images = parse_xcursor(&data)?;
 
    let image = images
        .into_iter()
        .max_by_key(|img| (img.size as i64 - size as i64).abs())?;
 
    let width = image.width as i32;
    let height = image.height as i32;
    if width <= 0 || height <= 0 {
        return None;
    }
 
    let rowstride = width * 4;
    let bytes = glib::Bytes::from_owned(image.pixels_rgba);
    let pixbuf = Pixbuf::from_bytes(
        &bytes,
        gtk4::gdk_pixbuf::Colorspace::Rgb,
        true,
        8,
        width,
        height,
        rowstride,
    );
 
    let target = size as i32;
    if width == target && height == target {
        Some(pixbuf)
    } else {
        pixbuf.scale_simple(target, target, InterpType::Bilinear)
    }
}

fn apply_cursor_preview(image: &gtk4::Image, theme: &str, size: u32) {
    image.set_pixel_size(size as i32);
    match load_cursor_pixbuf(theme, size) {
        Some(pixbuf) => image.set_from_pixbuf(Some(&pixbuf)),
        None => image.set_icon_name(Some("input-mouse-symbolic")),
    }
}

fn build_setting_row(label_text: &str) -> (GtkBox, GtkBox) {
    let row = GtkBox::new(Orientation::Horizontal, 20);
    row.set_hexpand(true);
    row.set_margin_start(20);
    row.set_margin_end(20);

    let label = Label::new(Some(label_text));
    label.add_css_class("frame-subtitle");
    label.set_halign(Align::Start);
    label.set_hexpand(true);

    let control_box = GtkBox::new(Orientation::Horizontal, 0);
    control_box.set_halign(Align::End);
    control_box.set_valign(Align::Center);

    row.append(&label);
    row.append(&control_box);

    (row, control_box)
}

fn build_ghost_for_theme(light: bool) -> GtkBox {
    let theme_class = if light { "ghost-light" } else { "ghost-dark" };

    let ghost = GtkBox::builder()
        .orientation(Orientation::Vertical)
        .spacing(2)
        .css_classes([theme_class])
        .build();
 
    let appwindow = GtkBox::builder()
        .orientation(Orientation::Horizontal)
        .spacing(0)
        .css_classes(["ghost-win", theme_class])
        .width_request(100)
        .height_request(200)
        .overflow(gtk4::Overflow::Hidden)
        .build();
 
    let appside = GtkBox::builder()
        .orientation(Orientation::Vertical)
        .spacing(4)
        .css_classes(["ghost-sidebar", theme_class])
        .halign(Align::Fill)
        .valign(Align::Fill)
        .width_request(28)
        .build();
 
    for _ in 0..4 {
        let item = GtkBox::builder()
            .css_classes(["ghost-sidebar-item", theme_class])
            .height_request(6)
            .margin_start(4)
            .margin_end(4)
            .build();
        appside.append(&item);
    }
 
    let appmain = GtkBox::builder()
        .orientation(Orientation::Vertical)
        .spacing(3)
        .hexpand(true)
        .css_classes(["ghost-main", theme_class])
        .build();
 
    let apptitlebar = GtkBox::builder()
        .orientation(Orientation::Horizontal)
        .spacing(3)
        .halign(Align::Start)
        .css_classes(["ghost-titlebar", theme_class])
        .height_request(10)
        .margin_start(4)
        .margin_top(3)
        .build();
 
    for cls in ["ghost-btn-close", "ghost-btn-min", "ghost-btn-max"] {
        let btn = GtkBox::builder()
            .css_classes([cls, theme_class])
            .width_request(4)
            .height_request(4)
            .valign(Align::Center)
            .build();
        apptitlebar.append(&btn);
    }
    appmain.append(&apptitlebar);
 
    for i in 0..5 {
        let line = GtkBox::builder()
            .css_classes(["ghost-content-line", theme_class])
            .height_request(4)
            .margin_start(6)
            .margin_end(if i % 2 == 0 { 10 } else { 20 })
            .margin_top(2)
            .build();
        appmain.append(&line);
    }
    

    appwindow.append(&appmain);
    appwindow.append(&appside);
    ghost.append(&appwindow);
 
    ghost
}

pub fn build_appearance_page(window: &ApplicationWindow) -> ScrolledWindow {
    let content = GtkBox::new(Orientation::Vertical, 16);
    
    let card = GtkBox::new(Orientation::Vertical, 10);
    card.add_css_class("shortcut-card");
    card.add_css_class("detail-card");
    card.set_margin_top(10);
    card.set_hexpand(true);


    let infoicon = gtk4::Image::from_file("/var/lib/cynager/icons/appearance.svg");
    infoicon.set_pixel_size(54);
    infoicon.set_halign(Align::Center);
    infoicon.set_css_classes(&["card-icons"]);

    let title = Label::new(Some("Appearance"));
    title.add_css_class("shortcut-title");
    title.set_halign(Align::Center);

    let subtitle = Label::new(Some("Change the overall look and theme of the OS."));
    subtitle.add_css_class("shortcut-desc");
    subtitle.set_halign(Align::Center);

    card.append(&infoicon);
    card.append(&title);
    card.append(&subtitle);

    content.append(&card);

    let wallframe = Frame::new(None);

    let wallbox = GtkBox::new(Orientation::Vertical, 10);
    let walltitle = Label::new(Some("Wallpaper"));
    walltitle.add_css_class("frame-title");
    walltitle.set_margin_start(20);
    walltitle.set_margin_top(20);
    walltitle.set_margin_end(20);
    walltitle.set_halign(Align::Start);

    let wallhead = GtkBox::new(Orientation::Horizontal, 5);
    wallhead.set_margin_start(20);
    wallhead.set_margin_end(20);
    wallhead.set_hexpand(true);

    let mons = get_monitors();
    
    let monwalls = Stack::new();
    monwalls.set_margin_start(20);
    monwalls.set_transition_type(gtk4::StackTransitionType::SlideLeftRight);
    monwalls.set_vexpand(true);

    let src = fs::read_to_string(CFGPATH).expect("[calibrate] failed to read cfg PATH");
    let mut doc = parse(&src).expect("[calibrate] failed to parse cfg");
    let mut changed = false;

    for (monitor, title) in mons {
        let monbox = GtkBox::new(Orientation::Vertical, 10);
        monbox.set_hexpand(true);
        monbox.set_vexpand(true);

        let found: Option<String> = doc
            .get("set", "walls")
            .and_then(|v| v.as_map())
            .and_then(|walls| walls.iter().find(|e| e.value.as_str() == Some(monitor.as_str())))
            .map(|e| e.key.to_string());

        let wallpaper: String = match found {
            Some(w) => w,
            None => {
                let set = doc.section_or_insert("set");
                match set.get_mut("walls") {
                    Some(Value::Map(walls)) => {
                        walls.push(Entry::new("none", monitor.clone()));
                        changed = true;
                    }
                    Some(Value::Str(_)) => eprintln!(
                        "[calibrate] `walls` in cfg must be a map, like:\n
                            walls :{{\n
                                    .config/walls/weonlygotearth.png :eDP-1\n
                            }}"
                    ),
                    None => {
                        set.push("walls", vec![Entry::new("none", monitor.clone())]);
                        changed = true;
                    }
                }
                "none".to_string()
            }
        };

        let wallrnpreview = GtkBox::builder()
            .css_classes(["wallpaperPrev"])
            .hexpand(false)
            .vexpand(false)
            .halign(Align::Start)
            .height_request(200)
            .width_request(300)
            .margin_bottom(20)
            .margin_end(10)
            .margin_start(10)
            .margin_top(10)
            .build();

        if wallpaper != "none" {
            let path = format!("{}/{}", std::env::var("HOME").unwrap_or_default(), wallpaper);
            let wallrnimg = build_round_wallpaper(&path, 20.0);
            wallrnimg.set_tooltip_text(Some(&wallpaper));
            wallrnpreview.append(&wallrnimg);
        }

        monbox.append(&wallrnpreview);

        monwalls.add_titled(&monbox, Some(&monitor), &title);
    }

    if changed {
        match doc.validate() {
            Ok(()) => {
                if let Err(e) = fs::write(CFGPATH, doc.render()) {
                    eprintln!("[calibrate] failed to write cfg: {e}");
                }
            }
            Err(e) => eprintln!("[calibrate] not writing cfg: {e}"),
        }
    }

    let montab = StackSwitcher::builder()
        .stack(&monwalls)
        .valign(Align::Start)
        .halign(Align::Start)
        .build();

    let addbtn = Button::new();

    let plus = Label::new(Some("Add Wallpaper"));
    addbtn.set_child(Some(&plus));
    addbtn.add_css_class("sub-btn");
    addbtn.set_halign(Align::End);
    addbtn.set_valign(Align::Center);
    addbtn.set_hexpand(true);

    let parent = window.clone();
    let gallery_holder: Rc<RefCell<ScrolledWindow>> = Rc::new(RefCell::new(ScrolledWindow::new()));
    let initial_gallery = build_wall_gallery(&monwalls, &wallbox, &gallery_holder);
    *gallery_holder.borrow_mut() = initial_gallery;

    let wallbox_for_add = wallbox.clone();
    let monwalls_for_add = monwalls.clone();
    let gallery_holder_for_add = gallery_holder.clone();

    addbtn.connect_clicked(move |_| {
        let wallbox = wallbox_for_add.clone();
        let monwalls = monwalls_for_add.clone();
        let gallery_holder = gallery_holder_for_add.clone();

        fn walls_dir() -> PathBuf {
            let mut dir = glib::home_dir();
            dir.push(".config");
            dir.push("walls");
            dir
        }

        let dialog = Dialog::builder()
            .transient_for(&parent)
            .modal(true)
            .title("Add new wallpaper")
            .build();

        let content = dialog.content_area();

        let outer = GtkBox::new(Orientation::Vertical, 8);
        outer.set_margin_top(16);
        outer.set_margin_bottom(16);
        outer.set_margin_start(16);
        outer.set_margin_end(16);

        let title = Label::new(Some("Add Wallpaper"));
        title.add_css_class("title");
        outer.append(&title);
        
        let drop_box = GtkBox::new(Orientation::Vertical, 12);
        drop_box.set_halign(Align::Fill);
        drop_box.set_valign(Align::Fill);
        drop_box.set_hexpand(true);
        drop_box.set_vexpand(true);
        drop_box.set_size_request(360, 220);
        drop_box.add_css_class("drop-zone");

        let icon = gtk4::Image::from_icon_name("image-x-generic-symbolic");
        icon.set_pixel_size(48);
        icon.set_vexpand(true);
        icon.set_valign(Align::End);

        let hint = Label::new(Some("Drag & drop images here"));
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

            target.connect_drop(move |_, value, _, _| {
                let Ok(file) = value.get::<gtk4::gio::File>() else {
                    return false;
                };
                let Some(path) = file.path() else {
                    return false;
                };

                let is_image: bool = path
                    .extension()
                    .and_then(|e| e.to_str())
                    .map(|e| matches!(e.to_lowercase().as_str(), "png" | "jpg" | "jpeg" | "webp" | "bmp" | "gif"))
                    .unwrap_or(false);

                if !is_image {
                    return false;
                }

                let dest_dir = walls_dir();
                if let Err(e) = fs::create_dir_all(&dest_dir) {
                    eprintln!("failed to create walls dir: {e}");
                    return false;
                }

                let Some(filename) = path.file_name() else {
                    return false;
                };
                let dest = dest_dir.join(filename);

                match fs::copy(&path, &dest) {
                    Ok(_) => {
                        drop_box.remove_css_class("drag-active");
                        drop_box.add_css_class("success");

                        drop_box.remove(&icon);
                        drop_box.remove(&hint);

                        let tick = Label::new(Some("✓"));
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
                        gtk4::glib::timeout_add_local_once(
                            std::time::Duration::from_millis(650),
                            move || {
                                dialog.response(gtk4::ResponseType::Accept);
                                dialog.close();
                            },
                        );
                        true
                    }
                    Err(e) => {
                        eprintln!("failed to copy wallpaper: {e}");
                        false
                    }
                }
            });
        }

        drop_box.add_controller(target);
        outer.append(&drop_box);

        content.append(&outer);

        dialog.connect_response(move |dialog, response| {
            if response == gtk4::ResponseType::Accept {
                refresh_wall_gallery(&wallbox, &monwalls, &gallery_holder);
            }
            dialog.close();
        });

        dialog.present();
    });

    wallhead.append(&montab);
    wallhead.append(&addbtn);

    wallbox.append(&walltitle);
    wallbox.append(&wallhead);
    wallbox.append(&monwalls);
    wallbox.append(&*gallery_holder.borrow());

    wallframe.set_child(Some(&wallbox));

    let themeframe = Frame::new(None);

    let themebox = GtkBox::new(Orientation::Vertical, 5);
    let themetitle = Label::new(Some("Theme Mode Toggle"));
    themetitle.add_css_class("frame-title");
    themetitle.set_margin_start(20);
    themetitle.set_margin_top(20);
    themetitle.set_margin_end(10);
    themetitle.set_halign(Align::Start);

    let themesubtitle = Label::new(Some("Change the theme Dark/Light mode for GTK based applications."));
    themesubtitle.add_css_class("frame-subtitle");
    themesubtitle.set_margin_start(20);
    themesubtitle.set_margin_end(20);
    themesubtitle.set_halign(Align::Start);

    let themetogglecontainter = GtkBox::new(Orientation::Horizontal, 20);
    themetogglecontainter.set_margin_start(20);
    themetogglecontainter.set_margin_end(20);
    themetogglecontainter.set_margin_bottom(20);
    themetogglecontainter.set_margin_top(20);

    let lightthemeghostbtn = build_ghost_for_theme(true);
    lightthemeghostbtn.set_css_classes(&["ghostBtn"]);
    let darkthemeghostbtn = build_ghost_for_theme(false);
    darkthemeghostbtn.set_css_classes(&["ghostBtn"]);

    lightthemeghostbtn.set_cursor_from_name(Some("pointer"));
    darkthemeghostbtn.set_cursor_from_name(Some("pointer"));

    let current_scheme = std::process::Command::new("gsettings")
        .args(["get", "org.gnome.desktop.interface", "color-scheme"])
        .output()
        .ok()
        .and_then(|o| String::from_utf8(o.stdout).ok())
        .unwrap_or_default();

    if current_scheme.contains("prefer-dark") {
        darkthemeghostbtn.add_css_class("ghostselected");
    } else {
        lightthemeghostbtn.add_css_class("ghostselected");
    }

    fn set_color_scheme(dark: bool) {
        let value = if dark { "prefer-dark" } else { "prefer-light" };
        if let Err(e) = std::process::Command::new("gsettings")
            .args(["set", "org.gnome.desktop.interface", "color-scheme", value])
            .spawn()
        {
            eprintln!("[calibrate] failed to run gsettings: {e}");
        }
    }

    let light_click = gtk4::GestureClick::new();
    {
        let light = lightthemeghostbtn.clone();
        let dark = darkthemeghostbtn.clone();
        light_click.connect_released(move |_, _, _, _| {
            light.add_css_class("ghostselected");
            dark.remove_css_class("ghostselected");
            set_color_scheme(false);
        });
    }
    lightthemeghostbtn.add_controller(light_click);

    let dark_click = gtk4::GestureClick::new();
    {
        let light = lightthemeghostbtn.clone();
        let dark = darkthemeghostbtn.clone();
        dark_click.connect_released(move |_, _, _, _| {
            dark.add_css_class("ghostselected");
            light.remove_css_class("ghostselected");
            set_color_scheme(true);
        });
    }
    darkthemeghostbtn.add_controller(dark_click);

    themetogglecontainter.append(&lightthemeghostbtn);
    themetogglecontainter.append(&darkthemeghostbtn);
    
    themebox.append(&themetitle);
    themebox.append(&themesubtitle);
    themebox.append(&themetogglecontainter);

    themeframe.set_child(Some(&themebox));

    let cursorframe = Frame::new(None);

    let cursorbox = GtkBox::new(Orientation::Vertical, 5);
    let cursortitle = Label::new(Some("Cursor"));
    cursortitle.add_css_class("frame-title");
    cursortitle.set_margin_start(20);
    cursortitle.set_margin_top(20);
    cursortitle.set_margin_end(10);
    cursortitle.set_halign(Align::Start);

    let cursorsubtitle = Label::new(Some("Change the cursor theme and size for GTK based applications."));
    cursorsubtitle.add_css_class("frame-subtitle");
    cursorsubtitle.set_margin_start(20);
    cursorsubtitle.set_margin_end(20);
    cursorsubtitle.set_halign(Align::Start);
    cursorsubtitle.set_margin_bottom(10);

    cursorbox.append(&cursortitle);
    cursorbox.append(&cursorsubtitle);

    let cursorrow = GtkBox::new(Orientation::Horizontal, 20);
    cursorrow.set_hexpand(true);
    cursorrow.set_margin_start(20);
    cursorrow.set_margin_end(20);
    cursorrow.set_margin_bottom(20);

    let cursor_themes = get_cursor_themes();
    let current_cursor_theme = get_current_cursor_theme();

    let sizes: [u32; 6] = [16, 24, 32, 48, 64, 96];
    let current_cursor_size = get_current_cursor_size();

    let cursor_display = GtkBox::new(Orientation::Vertical, 10);
    // cursor_display.add_css_class("wallpaperPrev");
    cursor_display.set_halign(Align::Start);
    cursor_display.set_valign(Align::Center);
    cursor_display.set_size_request(160, 160);
    cursor_display.set_hexpand(false);
    cursor_display.set_vexpand(false);

    let cursor_icon = gtk4::Image::new();
    cursor_icon.set_halign(Align::Center);
    cursor_icon.set_valign(Align::Center);
    cursor_icon.set_vexpand(true);
    apply_cursor_preview(&cursor_icon, &current_cursor_theme, current_cursor_size);

    cursor_display.append(&cursor_icon);

    let cursor_controls = GtkBox::new(Orientation::Vertical, 12);
    cursor_controls.set_hexpand(true);
    cursor_controls.set_valign(Align::Center);

    let theme_strs: Vec<&str> = cursor_themes.iter().map(String::as_str).collect();
    let theme_dropdown = DropDown::from_strings(&theme_strs);
    theme_dropdown.set_valign(Align::Center);

    let theme_idx = cursor_themes
        .iter()
        .position(|t| t == &current_cursor_theme)
        .unwrap_or(0);
    theme_dropdown.set_selected(theme_idx as u32);

    let size_strs_owned: Vec<String> = sizes.iter().map(|s| s.to_string()).collect();
    let size_strs: Vec<&str> = size_strs_owned.iter().map(String::as_str).collect();
    let size_dropdown = DropDown::from_strings(&size_strs);
    size_dropdown.set_valign(Align::Center);

    let size_idx = sizes.iter().position(|s| *s == current_cursor_size).unwrap_or(1);
    size_dropdown.set_selected(size_idx as u32);

    theme_dropdown.connect_selected_notify({
        let cursor_themes = cursor_themes.clone();
        let cursor_icon = cursor_icon.clone();
        let size_dropdown = size_dropdown.clone();
        move |dd| {
            let idx = dd.selected() as usize;
            if let Some(name) = cursor_themes.get(idx) {
                let size_idx = size_dropdown.selected() as usize;
                let size = sizes.get(size_idx).copied().unwrap_or(24);
                apply_cursor_preview(&cursor_icon, name, size);
            }
        }
    });

    let (cursor_theme_row, cursor_theme_control) = build_setting_row("Cursor Theme");
    cursor_theme_control.append(&theme_dropdown);

    size_dropdown.connect_selected_notify({
        let cursor_themes = cursor_themes.clone();
        let cursor_icon = cursor_icon.clone();
        let theme_dropdown = theme_dropdown.clone();
        move |dd| {
            let idx = dd.selected() as usize;
            if let Some(size) = sizes.get(idx) {
                let theme_idx = theme_dropdown.selected() as usize;
                if let Some(name) = cursor_themes.get(theme_idx) {
                    apply_cursor_preview(&cursor_icon, name, *size);
                }
            }
        }
    });

    let (cursor_size_row, cursor_size_control) = build_setting_row("Cursor Size");
    cursor_size_control.append(&size_dropdown);

    let cursor_save_btn = Button::new();
    let cursor_save_label = Label::new(Some("Save"));
    cursor_save_btn.set_child(Some(&cursor_save_label));
    cursor_save_btn.add_css_class("sub-btn");
    cursor_save_btn.set_halign(Align::End);
    cursor_save_btn.set_cursor_from_name(Some("pointer"));

    cursor_save_btn.connect_clicked({
        let theme_dropdown = theme_dropdown.clone();
        let size_dropdown = size_dropdown.clone();
        let cursor_themes = cursor_themes.clone();
        move |_| {
            let theme_idx = theme_dropdown.selected() as usize;
            let size_idx = size_dropdown.selected() as usize;

            let Some(theme) = cursor_themes.get(theme_idx) else { return };
            let Some(size) = sizes.get(size_idx) else { return };

            set_cursor_theme(theme);
            set_cursor_size(*size);
            save_niri_cursor_config(theme, *size);
        }
    });

    cursor_controls.append(&cursor_theme_row);
    cursor_controls.append(&cursor_size_row);
    cursor_controls.append(&cursor_save_btn);

    cursorrow.append(&cursor_display);
    cursorrow.append(&cursor_controls);

    cursorbox.append(&cursorrow);

    cursorframe.set_child(Some(&cursorbox));

    content.append(&wallframe);
    content.append(&themeframe);
    content.append(&cursorframe);
    page_scroller(&content)
}