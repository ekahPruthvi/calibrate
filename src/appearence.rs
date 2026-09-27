use gtk4::prelude::*;
use gtk4::{
    Application, ApplicationWindow, Box as GtkBox, Label, ListBox, ListBoxRow,
    Orientation, ScrolledWindow, SearchEntry, Stack, Separator, Switch, Scale, SpinButton,
    Adjustment, ComboBoxText, Frame, Grid, Align, CssProvider, DrawingArea, gdk_pixbuf::Pixbuf,
    Button, Dialog, DropTarget, StackSwitcher, gio, gdk, glib,
};
use std::fs;
use std::{rc::Rc, path::PathBuf};

use niri_ipc::{Request, Response, socket::Socket};
use infoprober::{parse, Entry, Value};

use gtk4:: gdk_pixbuf::{InterpType};
use std::cell::RefCell;

use crate::home::{rounded_rect, page_scroller};

const CFGPATH: &str = "/var/lib/cynager/info.probe";

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

fn build_ghost_for_theme(light: bool) -> GtkBox {
    let ghost = GtkBox::builder()
        .orientation(Orientation::Vertical)
        .spacing(2)
        .build();

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

    let themetogglecontainter = GtkBox::new(Orientation::Horizontal, 10);

    let lightthemeghostbtn = build_ghost_for_theme(true);

    themetogglecontainter.append(&lightthemeghostbtn);
    
    themebox.append(&themetitle);
    themebox.append(&themesubtitle);
    themebox.append(&themetogglecontainter);

    themeframe.set_child(Some(&themebox));

    content.append(&wallframe);
    content.append(&themeframe);
    page_scroller(&content)
}