use gtk4::{
    prelude::*, Switch, Frame, Application, ApplicationWindow, Box as GtkBox, Button, Image, Label, Orientation, Stack, GestureDrag,
    Fixed, MessageDialog , gdk::Display , CssProvider, glib, ResponseType, Widget, DrawingArea, Dialog, FileChooserDialog, FileChooserAction, gdk, ScrolledWindow
};
use std::{cell::RefCell, fs, path::PathBuf, process::Command, rc::Rc};
use std::collections::HashMap;
use std::{borrow, env};
use std::io::{Write, BufReader, BufRead};
use std::fs::OpenOptions;
use std::path::Path;
use networkmanager::{NetworkManager, devices::Device};
use networkmanager::devices::Wireless;
use dbus::blocking::Connection as DbusConnection;
use vte4::Terminal;
use vte4::TerminalExtManual;
use vte4::prelude::*;
use vte4::PtyFlags;

struct MonitorInfo {
    name: String,
    width: i32,
    height: i32,
    rotation: Rc<RefCell<u32>>,
    frame: Frame,
}

const SCALE: f64 = 0.1;
const SNAP_SIZE: i32 = 50; 

fn add_class_recursive (widget: &gtk4::Widget, class_name: &str) {
    widget.add_css_class(class_name);

    let mut child = widget.first_child();
    while let Some(current) = child {
        add_class_recursive(&current, class_name);
        child = current.next_sibling();
    }

}

fn typing_effect(label: &Label, text: &str, delay_ms: u64) {
    let label = label.clone();
    let chars: Vec<char> = text.chars().collect();
    let index = Rc::new(RefCell::new(0));
    let chars_rc = Rc::new(chars);

    glib::timeout_add_local(std::time::Duration::from_millis(delay_ms), move || {
        let i = *index.borrow();
        if i < chars_rc.len() {
            let current_text = chars_rc.iter().take(i + 1).collect::<String>();
            label.set_text(&current_text);
            *index.borrow_mut() += 1;
            glib::ControlFlow::Continue
        } else {
            glib::ControlFlow::Break
        }
    });
}

fn rotate_info(info: &Label, name: &str, rot: u32){
    let deg = match rot {
        1 => "90°",
        2 => "180°",
        3 => "270°",
        _ => "0°",
    };
    let label_info = format!("{}\nRotation:{}", name, deg);
    info.set_text(&label_info);
}

fn load_monitoors(fixed: &Fixed) -> HashMap<String, MonitorInfo> {
    let output = Command::new("hyprctl")
        .arg("monitors")
        .arg("all")
        .output()
        .expect("failed to execute hyprctl");
    let stdout = String::from_utf8_lossy(&output.stdout);

    let mut monitors = HashMap::new();

    for block in stdout.split("Monitor ").skip(1) {
        let mut name = "";
        let mut width = 0;
        let mut height = 0;
        let mut pos_x = 0;
        let mut pos_y = 0;

        let lines: Vec<_> = block.lines().collect();
        if let Some(first_line) = lines.get(0) {
            name = first_line.split_whitespace().next().unwrap_or("");
        }

        let mut transform = 0;
        for line in lines {
            let trimmed = line.trim();
            if trimmed.contains(" at ") {
                let parts: Vec<_> = trimmed.split(" at ").collect();
                if parts.len() == 2 {
                    let res = parts[0].split('@').next().unwrap_or("").trim();
                    let pos = parts[1].trim();

                    let res_parts: Vec<_> = res.split('x').collect();
                    if res_parts.len() == 2 {
                        width = res_parts[0].parse().unwrap_or(0);
                        height = res_parts[1].parse().unwrap_or(0);
                    }

                    let pos_parts: Vec<_> = pos.split('x').collect();
                    if pos_parts.len() == 2 {
                        pos_x = pos_parts[0].parse().unwrap_or(0);
                        pos_y = pos_parts[1].parse().unwrap_or(0);
                    }
                }
            }

            if trimmed.starts_with("transform:") {
                transform = trimmed
                    .trim_start_matches("transform:")
                    .trim()
                    .parse::<u32>()
                    .unwrap_or(0);
            }
        }

        let rot = Rc::new(RefCell::new(transform));
        let info = Label::new(None);
        rotate_info(&info, name, *rot.borrow());

        let frame = Frame::builder()
            .width_request((width as f64 * SCALE) as i32)
            .height_request((height as f64 * SCALE) as i32)
            .can_focus(true)
            .focusable(true)
            .build();

        let click = gtk4::GestureClick::new();
        let frame_clone = frame.clone();
        click.connect_pressed(move |_, _, _, _| {
            frame_clone.grab_focus();
        });
        frame.add_controller(click);

        frame.set_child(Some(&info));

        fixed.put(&frame, pos_x as f64 * SCALE, pos_y as f64 * SCALE);

        enable_key_movement(&frame, fixed, &rot, name, &info);

        monitors.insert(
            name.to_string(),
            MonitorInfo { name: name.to_string(), width, height, rotation: rot.clone(), frame: frame.clone() },
        );
    }

    monitors
}

fn enable_key_movement(frame: &Frame, fixed: &Fixed, rotation: &Rc<RefCell<u32>>, name: &str, label: &Label) {
    frame.set_focusable(true);
    frame.set_can_focus(true);

    let fixed = fixed.clone();
    let frame_clone = frame.clone();

    let wer_x = Rc::new(RefCell::new(0.0));
    let wer_y = Rc::new(RefCell::new(0.0));
    let rot = rotation.clone();

    let key_ctrl = gtk4::EventControllerKey::new();

    {
        let wer_x = wer_x.clone();
        let wer_y = wer_y.clone();
        let frame_clone = frame_clone.clone();
        let fixed = fixed.clone();
        let rot = rot.clone();

        key_ctrl.connect_key_pressed({
            let name = name.to_string(); // capture only once as String
            let label = label.clone();

            move |_, keyval, _, state| {
                let parent_alloc = fixed.allocation();
                let frame_width = frame_clone.width();
                let frame_height = frame_clone.height();

                let mut new_x = *wer_x.borrow();
                let mut new_y = *wer_y.borrow();

                match keyval {
                    gdk::Key::Up => {
                        new_y -= if state.contains(gdk::ModifierType::SHIFT_MASK) { 20.0 } else { 1.0 };
                        let clamped_y = (new_y as f64).clamp(0.0, (parent_alloc.height() - frame_height) as f64);
                        *wer_y.borrow_mut() = clamped_y;
                        fixed.move_(&frame_clone, *wer_x.borrow(), clamped_y);
                    }
                    gdk::Key::Down => {
                        new_y += if state.contains(gdk::ModifierType::SHIFT_MASK) { 20.0 } else { 1.0 };
                        let clamped_y = new_y.clamp(0.0, (parent_alloc.height() - frame_height) as f64);
                        *wer_y.borrow_mut() = clamped_y;
                        fixed.move_(&frame_clone, *wer_x.borrow(), clamped_y);
                    }
                    gdk::Key::Left => {
                        new_x -= if state.contains(gdk::ModifierType::SHIFT_MASK) { 20.0 } else { 1.0 };
                        let clamped_x = new_x.clamp(0.0, (parent_alloc.width() - frame_width) as f64);
                        *wer_x.borrow_mut() = clamped_x;
                        fixed.move_(&frame_clone, clamped_x, *wer_y.borrow());
                    }
                    gdk::Key::Right => {
                        new_x += if state.contains(gdk::ModifierType::SHIFT_MASK) { 20.0 } else { 1.0 };
                        let clamped_x = new_x.clamp(0.0, (parent_alloc.width() - frame_width) as f64);
                        *wer_x.borrow_mut() = clamped_x;
                        fixed.move_(&frame_clone, clamped_x, *wer_y.borrow());
                    }
                    gdk::Key::Control_L => {
                        let mut r = rot.borrow_mut();
                        *r = (*r + 1) % 4;
                        println!("Rotation set to: {}", *r);
                        rotate_info(&label, &name, *r);
                    }
                    _ => return glib::Propagation::Proceed,
                }
                glib::Propagation::Stop
            }
        });
    }

    frame.add_controller(key_ctrl);
}

fn save_monitor_layout(monitors: &HashMap<String, MonitorInfo>, parent_widget: &impl IsA<gtk4::Widget>) {
    let mut config = String::new();

    for monitor in monitors.values() {
        let alloc = monitor.frame.allocation();
        let pos_x = (alloc.x() as f64 / SCALE).round() as i32;
        let pos_y = (alloc.y() as f64 / SCALE).round() as i32;
        let rot = *monitor.rotation.borrow();
        config.push_str(&format!(
            "monitor = {}, {}x{}, {}x{}, 1, transform, {}\n",
            monitor.name, monitor.width, monitor.height, pos_x, pos_y, rot
        ));
    }

    // Confirmation dialog
    let dialog = MessageDialog::builder()
        .text("Apply this monitor layout?")
        .secondary_text(&config)
        .modal(true)
        .transient_for(&parent_widget.root().unwrap().downcast::<ApplicationWindow>().unwrap())
        .build();

    dialog.add_buttons(&[("Cancel", ResponseType::Cancel), ("Apply", ResponseType::Accept)]);

    dialog.connect_response(move |dialog, response| {
        if response == ResponseType::Accept {
            // Write config
            let home_dir = env::var("HOME").unwrap();
            let config_path = format!("{}/.config/hypr/monitors.conf", home_dir);

            let mut file = fs::File::create(&config_path).expect("Failed to write file");
            file.write_all(config.as_bytes()).expect("Failed to write");

            println!("Monitor layout saved to {}", config_path);

            let _ = Command::new("hyprctl").arg("reload").output();
        }
        dialog.close();
    });

    dialog.show();
}

fn is_system_theme_light() -> bool {
    let output = Command::new("sh")
        .arg("-c")
        .arg("gsettings get org.gnome.desktop.interface color-scheme")
        .output();

    if let Ok(output) = output {
        let prefer_output = String::from_utf8_lossy(&output.stdout).trim().to_string();
        return prefer_output != "'prefer-dark'";
    }
    true  
}

fn setup_switch(theme_switch: &Switch) {
    let is_light = is_system_theme_light();
    theme_switch.set_active(is_light);

    theme_switch.connect_state_set(|_, state| {
        let cmd = if state {
            "cynagectl -s light"
        } else {
            "cynagectl -s dark"
        };

        // Run the command
        let _ = Command::new("sh").arg("-c").arg(cmd).spawn();
        gtk4::glib::Propagation::Proceed
    });
}

fn is_notifications_sound() -> bool {
    let output = Command::new("cynagectl")
        .arg("-n")
        .output();
    if let Ok(output) = output {
        let prefer_output = String::from_utf8_lossy(&output.stdout)
            .trim()
            .to_string();
        return prefer_output == "toggle-on";
    }
    false
}

fn setup_sound_switch(switch: &Switch) {
    let is_on = is_notifications_sound();
    eprint!("{}", is_on);
    switch.set_active(is_on);

    switch.connect_state_set(|_, state| {
        let cmd = if state {
            "cynagectl -n true"
        } else {
            "cynagectl -n false"
        };
        let _ = Command::new("sh")
            .arg("-c")
            .arg(cmd)
            .spawn();

        gtk4::glib::Propagation::Proceed
    });
}

fn show_notification(notif_box: &GtkBox, text: &str) {
    let mut child = notif_box.first_child();
    while let Some(c) = child {
        child = c.next_sibling();  // get next before removing
        notif_box.remove(&c);
    }

    let notif_label = Label::new(Some(text));

    notif_box.append(&notif_label);
    notif_box.set_widget_name("notif_box");
    notif_box.show();

    let notif_box_clone = notif_box.clone();
    glib::timeout_add_local(std::time::Duration::from_secs(4), move || {
        notif_box_clone.hide();
        glib::ControlFlow::Break
    });
}

fn is_image_dark(image_path: &str) -> bool {
    let pixbuf = match gtk4::gdk::gdk_pixbuf::Pixbuf::from_file(image_path) {
        Ok(p) => p,
        Err(_) => return true, // assume dark on error
    };

    let width = pixbuf.width();
    let height = pixbuf.height();
     let pixels = unsafe { pixbuf.pixels() };
    let rowstride = pixbuf.rowstride();
    let n_channels = pixbuf.n_channels();

    let mut total_luminance = 0.0;
    let mut count = 0;

    // Sample every 10th pixel (for speed)
    let sample_step = 10;

    for y in (0..height).step_by(sample_step) {
        for x in (0..width).step_by(sample_step) {
            let offset = (y * rowstride + x * n_channels) as usize;
            let r = pixels[offset] as f64;
            let g = pixels[offset + 1] as f64;
            let b = pixels[offset + 2] as f64;

            let luminance = 0.2126 * r + 0.7152 * g + 0.0722 * b;
            total_luminance += luminance;
            count += 1;
        }
    }

    let avg_luminance = total_luminance / count as f64;

    println!("Average luminance: {}", avg_luminance);
    avg_luminance <= 128.0  // true = dark, false = light
}

fn extract_current_selection(config_path: &str) -> Option<String> {
    if let Ok(file) = fs::File::open(config_path) {
        for line in BufReader::new(file).lines().flatten() {
            if line.starts_with("exec-once") {
                // Extract filename
                if let Some(start) = line.find("/sound/startup/") {
                    let part = &line[start + "/sound/startup/".len()..];
                    if let Some(end) = part.find('"') {
                        return Some(part[..end].to_string());
                    }
                    return Some(part.to_string());
                }
            }
        }
    }
    None
}

fn getty_wifi_status() -> bool {
    let conn = dbus::blocking::Connection::new_system().unwrap();
    let nm = NetworkManager::new(&conn);
    nm.wireless_enabled().unwrap_or(false)
}

fn setty_wifi_enabled(enabled: bool) {
    let state = if enabled { "on" } else { "off" };
    let _ = Command::new("nmcli").args(&["radio", "wifi", state]).status();
}

fn clean_ssid(s: &str) -> String {
    // Remove leading/trailing whitespace and quotes
    let s = s.trim();
    s.trim_matches('"').to_string()
}

fn get_active_ssid() -> Option<String> {
    // Use nmcli to get the current active SSID, clean, and return it
    if let Ok(out) = Command::new("nmcli")
        .args(&["-t", "-f", "active,ssid", "dev", "wifi"])
        .output()
    {
        if let Ok(text) = String::from_utf8(out.stdout) {
            for line in text.lines() {
                if line.starts_with("yes:") {
                    let ssid = &line[4..];
                    let cleaned = clean_ssid(ssid);
                    if !cleaned.is_empty() && cleaned != "--" {
                        return Some(cleaned);
                    }
                }
            }
        }
    }
    None
}

fn refresh_wifi_listty(conn: &DbusConnection, network_list: &GtkBox) {
    // Remove all children
    while let Some(child) = network_list.first_child() {
        network_list.remove(&child);
    }

    // Collect the current connected SSID
    let active_ssid = get_active_ssid();

    let nm = NetworkManager::new(conn);
    let devices = match nm.get_devices() {
        Ok(devs) => devs,
        Err(_) => return,
    };

    let button_vec = Rc::new(RefCell::new(Vec::new()));

    for dev in devices {
        if let Device::WiFi(wifi_dev) = dev {
            let aps = match wifi_dev.get_access_points() { Ok(aps) => aps, Err(_) => continue };
            for ap in aps {
                let ssid = ap.ssid().unwrap_or_else(|_| "(unknown SSID)".to_string());
                let clean_button_ssid = clean_ssid(&ssid);
                let strength = ap.strength().unwrap_or(0);

                let button = Button::with_label(&format!("{} ({}%)", ssid, strength));
                button.set_css_classes(&["network_label"]);

                // Highlight the already-connected SSID
                if let Some(ref active) = active_ssid {
                    if &clean_button_ssid == active {
                        button.add_css_class("connected");
                    }
                }

                button_vec.borrow_mut().push(button.clone());
                let ssid_clone = ssid.clone();
                let button_vec_clone = button_vec.clone();
                let button_clone = button.clone();

                button.connect_clicked(move |_| {
                    let output = Command::new("nmcli")
                        .args(&["device", "wifi", "connect", &ssid_clone])
                        .output();

                    let mut connected = false;
                    if let Ok(_out) = output {
                        let check = Command::new("nmcli")
                            .args(&["-t", "-f", "active,ssid", "dev", "wifi"])
                            .output();

                        if let Ok(check) = check {
                            if let Ok(outstr) = String::from_utf8(check.stdout) {
                                for line in outstr.lines() {
                                    if line.starts_with("yes:") && clean_ssid(&line[4..]) == clean_ssid(&ssid_clone) {
                                        connected = true;
                                        break;
                                    }
                                }
                            }
                        }
                    }

                    // Remove 'connected' from all, add only to this one if successful
                    for btn in button_vec_clone.borrow().iter() {
                        btn.remove_css_class("connected");
                    }

                    if connected {
                        eprint!("connect");
                        button_clone.add_css_class("connected");
                    } else {
                        eprint!("failed");
                        button_clone.remove_css_class("connected");
                    }
                });

                network_list.append(&button);
            }
        }
    }
}



fn load_css() {
    let csss = r#"
        *{
            font-family: "BigBlueTerm437 Nerd Font";
            color: rgb(5, 117, 97);
        }

        window {
            background-color: rgba(30, 30, 30, 0.76);
            border-radius: 15px;
            box-shadow: 0 10px 30px rgba(0, 0, 0, 0.5);
            border: 1px solid rgba(255, 255, 255, 0.1);
            color: white;
            background-image: radial-gradient(rgba(92, 92, 92, 0.09) 2px, transparent 0);
            background-size: 30px 30px;
            background-position: -5px -5px;
        }

        .header {
            padding: 10px;
            margin: 0px;
            background-color: rgba(34, 34, 34, 0);
            min-height: 0px;
            box-shadow: none;
        }

        .footer {
            padding: 10px;
            margin: 0px;
            background-color: rgba(34, 34, 34, 0);
            min-height: 0px;
            box-shadow: none;
        }

        #notif_box {
            margin: 0px;
            padding: 0px;
        }


        .network_label {
            color: rgb(2, 71, 59);
            letter-spacing: 2px;
            padding: 25px;
            font-weight: 700;
        }

        button {
            all: unset;
            background-color: rgba(255, 255, 255, 0);
            border-radius: 0px;
            border: 1px solid rgb(5, 148, 122);
            padding: 10px;
            padding-right: 30px;
            padding-left: 30px;
            margin: 0px;
        }

        button:hover {
            background-color: #05947A;
            color: rgb(0, 0, 0);
        }

        button.walls {
            all: unset;
            padding: 1px;
            background-color: rgba(0, 0, 0, 0.18);
            box-shadow: 0 0 0 0px rgba(0, 0, 0, 0);
            transition: background-color 1s, box-shadow  ease-in-out 1s ;

        }   

        button.walls:hover {
            background-color: rgba(59, 59, 59, 0.31);
            box-shadow: 0 0 20px 1px rgba(0, 0, 0, 0.63);

        }

        button.connected {
            background-color: rgba(0, 0, 0, 0.2);
        }

        box {
            padding: 20px;
        }

        .wall_s {
            padding: 0px;
            border: 1px solid #05947A;
        }

        .wall_s scrollbar {
            background-color: transparent;
            border: none;
            padding: 4px;
        }

        .wall_s scrollbar slider {
            background-color: rgb(5, 148, 122);
            border-radius: 0px;
            min-height: 10px;
            border: none;
            transition: background-color 0.3s ease;
        }

        .wall_s scrollbar slider:hover {
            background-color: rgb(5, 197, 162);
        }

        .wall_s scrollbar trough {
            background-color: transparent;
            border-radius: 10px;
        }

        .display_win scrollbar {
            background-color: transparent;
            border: none;
            padding: 4px;
        }

        .display_win scrollbar slider {
            background-color: rgb(5, 148, 122);
            border-radius: 0px;
            min-height: 10px;
            border: none;
            transition: background-color 0.3s ease;
        }

        .display_win scrollbar slider:hover {
            background-color: rgb(5, 197, 162);
        }

        .display_win scrollbar trough {
            background-color: transparent;
            border-radius: 10px;
        }

        #current_wall {
            background-color: rgba(139, 139, 139, 0.09);
            padding: 5px;
            border: 1px solid rgba(5, 148, 122, 0.63);
            margin: 0;
        }

        .current_img {
            all: unset;
            padding: 0px;
            margin: 0;
        }

        switch {
            background-color: rgba(255, 255, 255, 0.1);
            border-radius: 0px;
            border: 1px solid rgba(5, 148, 122, 0.63);
            min-width: 80px;
            min-height: 30px;
            padding: 3px;
            transition: background-color 0.3s ease;
        }

        switch:checked {
            background-color: rgba(5, 148, 122, 0.63);
            border: 1px solid rgba(5, 148, 122, 0.63);
        }

        switch slider {
            background-color: rgba(5, 148, 122, 0.63);
            border-radius: 0px;
            min-width: 24px;
            min-height: 24px;
            transition: transform 0.3s ease, background-color 0.3s ease;
        }

        switch:checked slider {
            background-color: rgba(255, 255, 255, 0.1);
        }

        .grid-line {
            background-color: rgba(0,0,0,0.2);
        }

        .sound_btn {
            background-color: rgba(41, 41, 41, 0.25);
            border-radius: 0px;
            padding: 12px;
            font-size: 16px;
            border: 1px solid rgba(5, 148, 122, 0.63);
        }

        .sound_btn:hover {
            background-color: rgba(5, 148, 122, 0.13);
        }

        .sound_btn_selected {
            border: 2px solid rgba(5, 148, 122, 0.63);
            background-color: rgba(5, 197, 162, 0.63);
            color: rgba(3, 90, 74, 0.63);
        }

        label.eye {
            letter-spacing: 1px;
            line-height: 0.5;
            border: 1px solid rgba(5, 148, 122, 0.63);
            text-shadow:
                0 0 2px #aaffff,
                0 0 4px #55ffff,
                0 0 6px #22dddd,
                0 0 10px #11aaaa,
                0 0 20px #118888;
            padding: 0;
        } 

        label.eye2 {
            font-size: 8px;
            border: 1px solid rgba(5, 148, 122, 0.63);
            padding: 10px;
        }

        label.display {
            font-size: 2px;
            letter-spacing: 0px;
            line-height: 1;
            padding: 0;
            margin-top: 10px;
        } 
        
        label.switches {
            font-size: 8px;
            letter-spacing: 0px;
            line-height: 0.7;
            padding: 0;
            margin-top: 10px;
        }

        label.startup {
            font-size: 6px;
            letter-spacing: 0px;
            line-height: 0.9;
            padding: 0;
            margin-top: 10px;
        }

        label.calibrate {
            font-size: 18px;
            padding: 5px;
            margin: 5px;
        } 

        frame:focus {
            border: 2px solid rgb(255, 230, 0);
        }

        .home_page {
            margin: 30px;
            padding: 10px;
        }

        .wall-dialog filechooser {
            all: unset;
            background-color:rgba(32, 32, 32, 0.62);
            border: 2px solid #888;
            border-radius: 15px;
            padding: 0px;
            margin: 0px;
        }

        .wall-dialog {
            all:unset;
            padding: 5px;
            margin: 1px;
        }

        .wall-dialog pathbarbox {
            padding: 0px;
            margin: 0px;
        }

        .wall-dialog box {
            all:unset;
            padding: 5px;
            margin: 0px;
        }

    "#;

    let provider = CssProvider::new();
    provider.load_from_data(csss);
    gtk4::style_context_add_provider_for_display(
        &Display::default().unwrap(),
        &provider,
        gtk4::STYLE_PROVIDER_PRIORITY_APPLICATION,
    );
}

fn build_ui(app: &Application) {
    let window = ApplicationWindow::builder()
        .application(app)
        .title("Calibrate")
        .default_width(1500)
        .default_height(700)
        .resizable(false)
        .build();

    let main_box = GtkBox::builder().orientation(Orientation::Vertical).spacing(0).build();

    let home_dir = std::env::var("HOME").unwrap();

    let footer = GtkBox::new(Orientation::Horizontal, 0);
    footer.set_hexpand(true);
    footer.set_halign(gtk4::Align::Fill);
    footer.set_css_classes(&["footer"]);

    let page_title = Label::new(Some("HOME"));

    footer.append(&page_title);


    let notif_box = GtkBox::builder()
        .orientation(Orientation::Horizontal)
        .spacing(2)
        .build();
    notif_box.set_hexpand(true);
    notif_box.set_halign(gtk4::Align::End);
    notif_box.hide();

    footer.append(&notif_box);

    // header ----------------------------------------------------------------------------------------------------------------------------------------- //
    let header_box = GtkBox::new(Orientation::Horizontal, 0);
    header_box.set_hexpand(true);
    header_box.set_halign(gtk4::Align::Fill);
    header_box.set_css_classes(&["header"]);
    
    let scu_info = Label::new(Some("A Stertorus Cerebral Unit Software"));
    scu_info.set_justify(gtk4::Justification::Left);
    scu_info.set_halign(gtk4::Align::Start);
    scu_info.set_widget_name("scu_title");
    header_box.append(&scu_info);

    let hello = Label::new(Some(""));
    hello.set_halign(gtk4::Align::Center);
    hello.set_hexpand(true);
    typing_effect(&hello, "Welcome USER.5", 150);
    header_box.append(&hello);

    let title = Label::new(Some("Calibrate - COS"));
    title.set_justify(gtk4::Justification::Left);
    title.set_halign(gtk4::Align::End);
    title.set_hexpand(true);
    header_box.append(&title);

    // Stack_tab_box  ---------------------------------------------------------------------------------------------------------------------------------- //
    let stack = Stack::builder()
        .transition_type(gtk4::StackTransitionType::None)
        .build();

    let stack_box = GtkBox::new(Orientation::Horizontal, 5);
    stack_box.set_hexpand(true);
    stack_box.set_vexpand(true);

    let tabs_box = GtkBox::builder()
        .orientation(Orientation::Vertical)
        .spacing(0)
        .margin_top(20)
        .margin_start(20)
        .build();

    let home_box = gtk4::Grid::builder()
        .row_spacing(2)
        .column_spacing(2)
        .margin_top(20)
        .margin_bottom(20)
        .margin_start(20)
        .margin_end(20)
        .halign(gtk4::Align::Center)
        .build();

    home_box.set_halign(gtk4::Align::Center);
    home_box.set_valign(gtk4::Align::Center);
    home_box.set_hexpand(true);
    home_box.set_vexpand(true);
    home_box.set_css_classes(&["home_page"]);

   


    let back_button = Button::with_label("<< Back");
    back_button.set_visible(false);
    tabs_box.append(&back_button);

    let scu_logo = Image::from_icon_name("scu");
    scu_logo.set_pixel_size(246);
    tabs_box.append(&scu_logo);

    let stack_weak = stack.downgrade();
    let tabs_box_clone = tabs_box.clone();
    let egg_counter = Rc::new(RefCell::new(0));

    let add_tab_button = |name: &str, page_id: &str| {
        let button = Button::builder().build();
        let image = Label::new(Some(&name));
        image.set_justify(gtk4::Justification::Left);
        image.set_halign(gtk4::Align::Start);
        button.set_child(Some(&image));

        let page_id_string = page_id.to_lowercase();
        let stack_weak = stack_weak.clone();
        let page_title_clone = page_title.clone();
        let name_clone = name.to_lowercase();
        let egg_counter_clone = egg_counter.clone();
        let home_blox_outer_clone = home_box.clone();

        let eye_penglin = Label::new(Some("@@@@@%%%%%%%%%####********+++++++++++++++++++******########%%%%%%%%%%%%%%%%%%%%%%%%%##%######
@@@@@@@%%%####************++++++++++++++++++++++******########%%%%%%%%%%%%%%%%%%%%%%%#%######
@@@@%%%%%%%%####*********++++++++++++++++++++++++*******########%%%#%%%%%%%%%%%%%%%%%%#######
@@@@%%%%%%#####***********+*+++++++++++++++++++++*++******#*########%%%%%%%%%%%%%%%%%%#######
@@@@%%%%%%######************+**+**++++++++++*+**++++++*****############%%%%%%%%%%%%%%########
@@%%%%######******************+++++++++++++++++++++++++++******#########%%#%%%%%##%%%########
@@%%%########*******************++++++++++++++++++++++++++++++*+****########%%%%%##%%########
@%%%%%####*************************++++++++++++++++=++++++++++++++**#*########%%%%%#%%%#%%%%%
%%%%%######***************************+++++++++++++==+++++++=++==+=+++**########%%%%%%%%%%%%%
%%%%%#######**#**#********************+++++++++++++++++=++===+==+++=+++****#######%%%%%%%%%%%
%%%%#########*##########*******************************+++++++===-=+++=++****#######%%%%%%%%%
%%%%#############################****************+*++*******+=-=========+++****#######%%%%%%%
%%%%##########################*****************+++*++++*+++++*#*=:-=--===++++*****#####%%%%%%
%%%%##################%%########*****************+***++++++*+******=--======++*****######%%%%
%%%%#############%%%%%#########*****************++++++=+++++++++++**#+=-=====++*******####%%%
%%%%########%%%%%%%%%%%%%%########*****************+++++++====+++++++***+-==++++*******###%%%
%%%%%%%%%%%%%%%%%%%%%%%%###########************+++++=++=+===+++==++++++***+==+++*******###%%%
%%%%%%%%%%%%%%%%%%%%%%%###############**********+++====++++======+==+++++***++++********##%%%
%%%%%%%%%%%%%%%%%%%%%%%#######################**+*+++==+==++++++====+++++++***+++********##%%
%%%%%%%%%%%%%%%%%%%%%%%%%%%%%%%%%#%%#%%%%%%%%%%%%%%%%%%#####****++++++++++++***+++******###%%
%%%%%%%%%%%%%%%%%%%%%%%%%%%%%%%%%@%@@@@@@@@@@@@@@@@@@@@@%%%%%#######**++++*++++**++**######%%
%%%%%%%%%%%%%%%%%%%%%%%%%%%%%%@@@@@@@@@@@@@@@@@@@@@@%%@@%%%%%%%%%%%%%%##**++++++*****#####%%%
%%%%%%%%%%%%%%%%%%%%%%%%%%@@@@@@@@@@@@@@@@@@@@@@@#*%==%%%%####%%%%%%%%%%%##*++++++***#####%%%
%%%%%%%%%%%%%%%%%%%%%%%@@@@@@@@@@@@@@@@@@@@@@@%%#%%%=+%%%%*==++*##%%%%%%%%%%%#*++++***###%%%%
%%%%%%%%%%%%%%%%%%%%%@@@@@@@@%%%@@@@@@@@@@@@@@@%%%%%=*%%%%#+---==+*#%%%%%%%%%%%%#*++**###%%%%
%%%%%%%%%%%%%%%%%@@@@@@@@@%%%%%%@@@@@@@@@@%@@@@@@@%%%%%%%%%*=-----=+**##%%%%@@%%%%######%%%%%
%%%%%%%%%%%%%%%%%@@@@@@@@%%%#%%%@@@@@@@@%%@@@@@%%%%#%@@%%%%*=--::--==++*#%%%%%%@@@%%%%#%%%%%%
%%%%%%%%%%%%%@@@@@@@@@@@@%%%###%%@@@@@@@@@@@@@%@@@@@@@%%%%%*=-::::---==+*##%%%%%@%%%%%%%%%%%%
%%%%%%%%@@@@@@@@@@@@@@@%%%%#####%%@@@@@@@@@@%@@@@@@@@%%%%%#+=-::::---==+**##%%%%%%##%%%%%%%%%
%%%%%%%%%%%@@@@@@@@@@@@@%%%%%%##%%@@@@@@@@@@@@@@@@%%%%%%%%*=--:::----==++*##%#######%%%%%#%%%
%%%%%%%%%%%%%%@@@@@@@@@@%%%%%%%%%%%@@@@@@%%%%%@@%%%%%%%%#+=-:::-=++*****+**#***#####%%%%####%
%%%%%%%%%%%%%%%%%%%%%%%%%%%%%%%%%%%%@@@@@@@@%%%%%%%%%%#+=-+++*******+++**#***##**##%%########
#%%%%%%%%%%%%%%%%%%%%%%%%%%%%%%%%%################******+*++***+++++*###*******##############
###%%%%%%%%%%%%%%%%%%%%%%%%%%%%%%%%%%%%########*#**#*************###**********###############
#######%%%%%%%%%%%%%%%%%%%%%%%%%%%%%%####%###########*##**########***********###############%
#######################%%%%%%%%%%%%%%%%%%%##%%%%%###########****************##*******########
##############*#************###############%%%######**********************************######%
#***************************************************++++*+++++++**+*+*****************######%
***************+*+++++++++++++++++++++++++++++++++++++++++++++++++******++***********#######%
***********+++*+++++++++++++++++++++++++++++++++++++++++++++********++++++++********#######%%
******++++++++++++++++++++++++++++++++++++++++++++++**+*********+++++++++++++******######%%%%
*****+++++++++++++++++++++++++++++++++++++++**++*+******++++++++++++++++++++++****######%%%%%
***+++++++++++++=+++++===+++++++++++++++++++++++++++++++++++++++++++++++++++++****######%%%%%
****++++++++++++++++++==+++++++=++=+++++++++++++++++++=+++++++++++++++++++++++***######%%%%%%
*++++++++++++++++++=+==++=========++++==+=++++++++======++=+++===++++++++++++****#####%%%%%%%
***++++++++++++++++++==+=====================+================+========+=+++*****#####%%%%%%#
*****++++++++++++++=++=======================================+========++++++*****####%%%%%###
******++++++++++++++=================================================+===++++***####%%%%####*
******+++++++++++++++===============================================+==+++++****###%%%#####**
******++++++++++++++=================================================+=+++++***###%%%########
*****+++++++++++++======+==============================-===============+++++**###%%%#########
***+*+++++++++++++==+===================================-========-=====+++++**##%%%##########
***+++++++++++++++===================================-=-=-===----=====+=+++**##%%%###########
****++++++++++++++=====+===========================-=-=-==---==--===++++=+**##%%%%###########
**++++++++*+++======+=++===============================----=-=--=====++++**##%%%%############"));
        eye_penglin.set_css_classes(&["eye2"]);

        button.connect_clicked(move |_| {
            if let Some(stack) = stack_weak.upgrade() {
                if page_id_string == "wallpaper" {
                    let mut counter = egg_counter_clone.borrow_mut();
                    *counter += 1;
                    if *counter == 5 {
                        eprintln!("\n\n\n\n\n\n\n\nit's 5 ---------------------------------");
                    } else if *counter == 7 {
                        eprintln!("7 7 2005 huh?");
                        home_blox_outer_clone.attach(&eye_penglin, 0, 0, 2, 2);
                    }
                }
                stack.set_visible_child_name(&page_id_string);
                typing_effect(&page_title_clone, &name_clone, 10);
            }
        });

        tabs_box_clone.append(&button);
    };

    add_tab_button("Home", "home");
    add_tab_button("Wallpapers", "wallpaper");
    add_tab_button("Shell configs", "cynide");
    add_tab_button("Network", "network");
    add_tab_button("Bluetooth", "bluetooth");
    add_tab_button("Show Keybindings", "keybindings");
    add_tab_button("Notification History", "notifications");
    add_tab_button("About", "about");

     
    let eye = Label::new(Some("
⣿⡟⢠⣿⣯⠦⢀⡀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⢀⡀⠀⠀⠀⠀⠀⠈⠂⠀⠀⠀⠀⠀⠀⠀⠑⠐⠀⠀⠀⠀⠀⠀⠸⡀ \n
⣿⢇⡿⣭⡦⠗⠁⠄⠂⠀⠀⠀⠀⠀⡠⣰⢀⠀⠀⠀⢰⠋⡆⢀⢠⠀⠀⠀⠀⠀⠐⢆⠀⢂⠀⠀⠀⠀⠀⠀⠀⠀⠀⠂⠀⠁⠀\n
⣟⠘⣼⣎⠕⠊⠁⠀⠀⠀⢢⠆⡀⠬⡑⢿⣻⡆⠀⡀⡄⠄⣧⢸⡈⢀⠀⡆⢠⠀⠀⠀⠁⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀\n
⡇⠸⡩⡠⡔⢱⢀⠰⣄⠔⠁⣻⣢⢙⣿⣼⣿⣷⠴⠿⣿⡗⣟⣿⡿⣷⣾⣤⣼⣄⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀\n
⠁⠈⠔⢱⢌⢿⢢⠑⠻⣗⠎⣀⣿⣟⢛⣍⣯⣿⣧⣤⣿⣧⣿⣿⣵⣾⣿⣎⡹⠿⣿⣶⣄⡀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀\n
⠀⠀⠀⠀⢾⢱⣷⣷⡢⢾⣷⢯⣽⣽⣿⣿⠿⣿⣛⡿⠯⠿⠿⠿⡿⠿⣿⣿⣿⣿⣿⣿⣽⣟⣦⡀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀\n
⠀⠀⢀⠑⢬⣧⣻⣽⣽⣿⣿⣿⣿⢟⣻⠟⠋⠁⠀⠀⠀⠀⠀⠀⠀⠀⠈⠉⠛⠿⣽⢿⡙⢿⣿⣿⣇⡀⠀⠀⠀⠀⠀⠀⠀⠀⠀\n
⠀⠀⠈⠳⢜⢿⣿⣿⢿⣿⣿⡿⣩⠋⠄⠀⠀⠀⠀⠀⣀⣠⣤⣤⣤⣤⣄⡀⠀⠀⠈⠻⣮⡟⠙⠹⣿⣷⡀⠀⠀⠀⠀⠀⠀⠀⠀\n
⠀⠀⢀⢀⡀⠉⢟⡻⢛⣿⠿⡷⠁⠀⠀⠀⠀⢀⣴⣿⣿⣿⣿⣿⣿⣿⣿⣿⣦⡀⠀⠀⠹⣿⣷⣦⣱⣿⣿⣄⠀⠀⠀⠀⠀⠀⠀\n
⠀⠀⠀⠉⠚⣋⠶⣋⡵⢏⣰⠁⠀⠀⠀⠀⢠⣾⣿⣿⣿⣿⣿⣿⣿⣿⣿⣿⣿⣷⠀⠀⠀⠈⢿⣿⣿⣿⣿⣿⣦⡀⠀⠀⠀⠀⠀\n
⠀⠀⠀⢬⣷⣶⣽⣿⣦⡉⢡⠀⠀⠀⠀⠀⣾⣷⣿⣿⣿⣿⣿⣿⣿⣿⣿⣿⣿⣿⠀⠀⠀⠀⠀⠹⣿⣿⣿⣿⣿⣿⣷⢄⡀⠀⠀\n
⠀⠀⠀⡨⠟⠉⠉⣉⠻⣿⡌⢆⠀⠀⠀⠀⢻⣿⣿⣿⣿⣿⣿⣿⣿⣿⣿⣿⣿⡇⠀⠀⠀⠀⠀⢔⣿⣽⣿⣿⣿⣿⣿⣤⠑⢶⡄\n
⠀⠀⠐⠁⠀⢠⡪⠒⣚⣻⣶⣄⠳⣠⠀⠀⠈⢻⣿⣿⣿⣿⣿⣿⣿⣿⣿⣿⡿⠁⠀⠀⠀⠀⠐⣾⣿⣿⣿⣿⣿⣿⣿⣿⣱⣼⣯\n
⠀⠀⠀⢀⣔⢡⡴⢛⣳⡼⠿⢿⣧⣬⣑⠤⣀⡀⠉⠻⢿⣿⣿⣿⣿⣿⠟⠋⠀⠀⠀⣀⣀⣤⣾⣿⣿⣿⡿⣿⣿⠿⠿⢿⢿⡿⠠\n
⠀⠀⠀⠉⠊⡝⠨⠋⠀⢀⡤⣾⣟⡻⣿⢷⣶⣬⣭⣐⣤⣄⢀⣈⣀⠀⡠⢄⡦⣤⡛⠩⣿⢛⣻⢿⢛⡼⠾⠝⡅⠭⠪⠴⠋⠀⠀\n
⠀⠀⠀⠀⠀⠀⠀⢠⠖⢛⢜⡩⠔⠋⣉⢔⠟⢪⡿⣫⠛⢿⣿⣿⡧⠉⣿⠎⠺⣾⠁⠃⣻⠑⠠⠂⠑⢒⢁⠤⠐⡄⠉⠀⠀⠀⠀\n
⠀⠀⠀⠀⠀⠀⠐⠁⠀⠉⠁⠀⠀⣪⠼⠃⢠⠿⠈⢼⢀⣾⠯⢿⠂⢑⢸⠢⠂⠃⠀⠀⠐⡘⠄⢠⠔⠓⢙⡥⠋⠀⠀⠀⠀⠀⠀\n
⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠈⠀⠀⠀⠉⠀⠀⠇⠀⠃⠀⠘⢀⢉⠁⢀⢀⠀⡀⠀⠀⢔⠺⢽⠪⠈⠀⠀⠀⠀⠀⠀⠀⠀⠀\n
⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠉⠀⠀⡀⢀⡅⠤⠀⠈⢤⠐⠆⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀\n
⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⡐⠈⠰⠓⢀⠄⠇⠺⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀\n
⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⢀⠀⠀⠀⠀⠁⠀⠀⠀⠴⠠⠀⠂⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀"));
    eye.set_css_classes(&["eye"]);
    eye.is_selectable();

    let calibrate = Label::new(Some("Calibrate for cynageOS"));
    calibrate.set_css_classes(&["calibrate"]);

    home_box.attach(&eye, 0, 0, 2, 2);
    home_box.attach(&calibrate, 0, 2, 2, 2);

    stack.add_titled(&home_box, Some("home"), "Home");

    stack_box.append(&tabs_box);
    stack_box.append(&stack);

    // Wallpaper page ---------------------------------------------------------------------------------------------------------------------------------- //
    // let wallpaper_box = GtkBox::builder().orientation(Orientation::Vertical).spacing(0).build();

    // let output = Command::new("/home/ekah/.config/hypr/scripts/swwwallpaper.sh")
    //     .output()
    //     .expect("Failed to run swwwallpaper.sh");

    // let stdout = String::from_utf8_lossy(&output.stdout);
    // let line = stdout.trim();

    // let parts: Vec<&str> = line.split(':').collect();
    // let display_name = parts[0].trim().to_string();

    // let rest = parts[1];
    // let resolution_part = rest.split(',').next().unwrap().trim().to_string();

    // let image_path_part = line.split("image:").nth(1).unwrap().trim();
    // let image_filename = image_path_part
    //     .rsplit('/')
    //     .next()
    //     .unwrap()
    //     .replace(".blur", "");

    // let display_label = gtk4::Label::new(Some(&format!("Display: {}", display_name)));
    // display_label.set_justify(gtk4::Justification::Right);
    // display_label.set_halign(gtk4::Align::End);
    // display_label.set_hexpand(true);
    // let resolution_label = gtk4::Label::new(Some(&format!("Resolution: {}", resolution_part)));
    // resolution_label.set_justify(gtk4::Justification::Right);
    // resolution_label.set_halign(gtk4::Align::End);
    // resolution_label.set_hexpand(true);

    // let image_path = format!("{}/.config/swww/cynage/{}", home_dir, image_filename);

    // let file = gtk4::gio::File::for_path(image_path);
    // let texture = gtk4::gdk::Texture::from_file(&file).unwrap();
    // let current_pic = gtk4::Picture::for_paintable(&texture);
    // let current_pic_ref = Rc::new(RefCell::new(current_pic.clone()));

    // let current_wall = GtkBox::new(Orientation::Horizontal, 2);

    // let wall_info = GtkBox::new(Orientation::Vertical, 10);
    // wall_info.set_hexpand(true);
    // wall_info.set_halign(gtk4::Align::Fill);

    // let wall_buttons = GtkBox::new(Orientation::Horizontal, 5);
    // wall_buttons.set_hexpand(true);
    // wall_buttons.set_vexpand(true);
    // wall_buttons.set_valign(gtk4::Align::Baseline);
    // wall_buttons.set_halign(gtk4::Align::End);
    // let add_wall = Button::builder().child(&Label::new(Some("Add wallpapers"))).build();
    // let remove_wall = Button::builder().child(&Label::new(Some("Remove wallpaper"))).build();
    // let vdummy_forwallinfo = GtkBox::new(Orientation::Vertical, 15);
    // vdummy_forwallinfo.set_vexpand(true);

    // wall_buttons.append(&add_wall);
    // wall_buttons.append(&remove_wall);

    // wall_info.append(&display_label);
    // wall_info.append(&resolution_label);
    // wall_info.append(&vdummy_forwallinfo);
    // wall_info.append(&wall_buttons);

    // current_wall.append(&current_pic);
    // current_wall.append(&wall_info);
    // current_wall.set_widget_name("current_wall");


    // let scrolled_window = gtk4::ScrolledWindow::builder()
    //     .min_content_height(150)
    //     .hscrollbar_policy(gtk4::PolicyType::Automatic)
    //     .vscrollbar_policy(gtk4::PolicyType::Never)
    //     .hexpand(true)
    //     .vexpand(false)
    //     .build();
    // scrolled_window.set_css_classes(&["wall_s"]);

    // let image_grid = GtkBox::builder()
    //     .orientation(gtk4::Orientation::Horizontal)
    //     .spacing(10)
    //     .build();

    // scrolled_window.set_child(Some(&image_grid));

    // wallpaper_box.append(&current_wall);
    // wallpaper_box.append(&scrolled_window);

    // // Load images dynamically
    // let add_walls_to_grid = |boxxy: &GtkBox, notiv_boxxy: &GtkBox, picc_ref: &Rc<RefCell<gtk4::Picture>>| {
    //     while let Some(child) = boxxy.first_child() {
    //         boxxy.remove(&child);
    //     }
    //     let home_dir = std::env::var("HOME").unwrap();
    //     let wallpaper_dir = PathBuf::from(format!("{}/.config/swww/cynage", home_dir));
    //     if let Ok(entries) = fs::read_dir(wallpaper_dir.clone()) {
    //         let notiv_clone_outer_for_wall = notiv_boxxy.clone();
    //         for entry in entries.flatten() {
    //             let path = entry.path();
    //             if path.is_file() {
    //                 let filename = path.file_name().unwrap().to_string_lossy().to_string();
    //                 let img_path = path.clone();
    //                 let btn = Button::builder().build();
    //                 btn.set_css_classes(&["walls"]);
    //                 let pixbuf = gtk4::gdk_pixbuf::Pixbuf::from_file_at_scale(img_path, 260, 260, true).ok();
    //                 if let Some(pix) = pixbuf {
    //                     let image = Image::from_pixbuf(Some(&pix));
    //                     image.set_pixel_size(260);
    //                     image.add_css_class("thumbnail");
    //                     btn.set_child(Some(&image));
    //                 }

    //                 // Clicking image button to execute script
    //                 let home_dir_cloned = home_dir.clone();
    //                 let filename_clone = filename.clone();
    //                 let current_pic_clone = picc_ref.clone();
    //                 let notiv_clone_for_wall = notiv_clone_outer_for_wall.clone(); 
    //                 btn.connect_clicked(move |_| {
    //                     let target_path = format!("{}/.config/swww/cynage/{}", home_dir_cloned, filename_clone);
    //                     let script_path = format!("{}/.config/hypr/scripts/swwwallpaper.sh", home_dir_cloned);
    //                     let _ = Command::new(script_path).arg("-s").arg(&target_path).spawn();
    //                     let file = gtk4::gio::File::for_path(&target_path);
    //                     if let Ok(texture) = gtk4::gdk::Texture::from_file(&file) {
    //                         let new_pic = gtk4::Picture::for_paintable(&texture);
    //                         new_pic.set_hexpand(true);
    //                         new_pic.set_vexpand(true);
    //                         new_pic.set_halign(gtk4::Align::Fill);
    //                         new_pic.set_valign(gtk4::Align::Fill);
    //                         current_pic_clone.borrow_mut().set_paintable(Some(&texture));
    //                     }
    //                     let now = is_image_dark(&target_path);
    //                     let prefer_output = is_system_theme_light();
    //                     if now && prefer_output {
    //                         show_notification(&notiv_clone_for_wall, "wallpaper changed, dark wallpaper detected");
    //                         let _ = Command::new("cynagectl").arg("-s").arg("dark").spawn();
    //                     } else if now && !prefer_output {
    //                         show_notification(&notiv_clone_for_wall, "wallpaper changed");
    //                     } else if !now && !prefer_output {
    //                         show_notification(&notiv_clone_for_wall, "wallpaper changed, Light wallpaper detected");
    //                         let _ = Command::new("cynagectl").arg("-s").arg("light").spawn();
    //                     } else {
    //                         show_notification(&notiv_clone_for_wall, "wallpaper changed");
    //                     }
    //                 });
    //                 boxxy.append(&btn);
    //             }
    //         }
    //     }
    // };

    // add_walls_to_grid(&image_grid, &notif_box, &current_pic_ref);
    // let image_grid_clone = image_grid.clone();
    // let notif_box_clone = notif_box.clone();
    // let window_clone = window.clone();
    // add_wall.connect_clicked(move |_| {
    //     let notif_box_clone = notif_box_clone.clone();
    //     let curren_pic_ref_clone = current_pic_ref.clone();
    //     let dialog = FileChooserDialog::new(
    //         Some("Select wallpaper to add"),
    //         Some(&window_clone),
    //         FileChooserAction::Open,
    //         &[("_Cancel", ResponseType::Cancel), ("_Add", ResponseType::Accept)],
    //     );
    //     dialog.set_css_classes(&["wall-dialog"]);
    //     dialog.set_size_request(400, 800);
    //     add_class_recursive(&dialog.upcast_ref(), "wall-dialog");

    //     let image_grid_inner = image_grid_clone.clone();
    //     dialog.connect_response(move |dialog, response| {
    //         if response == ResponseType::Accept {
    //             if let Some(file_path) = dialog.file().and_then(|f| f.path()) {
    //                 let _ = Command::new("cynagectl")
    //                     .args(["-w", "add", file_path.to_str().unwrap_or_default()])
    //                     .spawn();
    //             }
    //         }
    //         dialog.close();
    //         add_walls_to_grid(&image_grid_inner, &notif_box_clone, &curren_pic_ref_clone);
    //     });

    //     dialog.show();
    // });
    
    // let window_clone2 = window.clone();
    // let image_grid_clone = image_grid.clone();
    // let notif_box_clone2 = notif_box.clone();
    // let current_pic_ref2 = Rc::new(RefCell::new(current_pic.clone()));
    // remove_wall.connect_clicked(move |_| {
    //     let notif_box_clone2 = notif_box_clone2.clone();
    //     let curren_pic_ref_clone2 = current_pic_ref2.clone();
    //     let dialog = FileChooserDialog::new(
    //         Some("Select wallpaper to remove"),
    //         Some(&window_clone2),
    //         FileChooserAction::Open,
    //         &[("_Cancel", ResponseType::Cancel), ("_Remove", ResponseType::Accept)],
    //     );
    //     dialog.set_css_classes(&["wall-dialog"]);
    //     dialog.set_size_request(400, 800);
    //     add_class_recursive(&dialog.upcast_ref(), "wall-dialog");

    //     let image_grid_inner = image_grid_clone.clone();
    //     if let Some(home_dir) = std::env::var_os("HOME") {
    //         let start_path = Path::new(&home_dir).join(".config/swww/cynage/");
    //         let _ = dialog.set_current_folder(Some(&gtk4::gio::File::for_path(start_path)));
    //     }

    //     dialog.connect_response(move |dialog, response| {
    //         if response == ResponseType::Accept {
    //             if let Some(file_path) = dialog.file().and_then(|f| f.path()) {
    //                 if let Some(file_stem) = file_path.file_stem().and_then(|s| s.to_str()) {
    //                     let _ = Command::new("cynagectl")
    //                         .args(["-w", "remove", file_stem])
    //                         .spawn();
    //                 }
    //             }
    //         }
    //         dialog.close();
    //         add_walls_to_grid(&image_grid_inner, &notif_box_clone2, &curren_pic_ref_clone2);
    //     });

    //     dialog.show();
    // });


    // stack.add_titled(&wallpaper_box, Some("wallpaper"), "Wallpaper");

    // shell settings ---------------------------------------------------------------------------------------------------------------------------------- //
    
    let shell_settings_scroller: gtk4::ScrolledWindow = gtk4::ScrolledWindow::new();
    shell_settings_scroller.set_policy(gtk4::PolicyType::Automatic, gtk4::PolicyType::Never);

    let shell_stack = Stack::builder()
        .transition_type(gtk4::StackTransitionType::None)
        .build();

    let shell_settings_box = GtkBox::new(Orientation::Horizontal, 50);
    let shell_settings_box_clone = shell_settings_box.clone();


    let shell_stack_weak = shell_stack.downgrade();
    let add_shell_button = |icon_ascii: &str, page_id: &str, page_name: &str| {
        let btn_box = GtkBox::new(Orientation::Vertical, 2);
        let image = Label::new(Some(&icon_ascii));
        image.set_css_classes(&[page_id]);
        image.set_vexpand(true);
        image.set_valign(gtk4::Align::Baseline);
        let label = Label::new(Some(&page_id));

        btn_box.append(&label);
        btn_box.append(&image);

        let button = Button::builder().child(&btn_box).build();

        let page_id_string = page_id.to_lowercase();
        let page_title_clone = page_title.clone();
        let page_name_clone = page_name.to_lowercase();
        let back_button_clone = back_button.clone();   
        let stack_weak = shell_stack_weak.clone(); 

        button.connect_clicked(move |_| {
            if let Some(stack) = stack_weak.upgrade() {    
                stack.set_visible_child_name(&page_id_string);
                typing_effect(&page_title_clone, &page_name_clone, 10);
                back_button_clone.set_visible(true);
            }
        });

        shell_settings_box_clone.append(&button);
    };
    

    add_shell_button("
                                       ▓▓▓▓▓▓                                          
                                    ▓▓▓▓░░░░░▓▓▓▓▓                                      
                                ▓▓▓▓░░░░░░░░░░░░▒▒████                                  
                            ▓▓▓▓░░░░░░░░░░░░░░░░░▒▒▒▒▒████                              
                        ████░░░░░░░░░░░░░░░░░░░░░░░░░░░░▒▒████                            
                    ████░░░░░░░░░░░░░░░░░░░░░░░░░░░░▒▒▒▒░░░░██▓▓                          
                ▓▓▓▓░░░░░░░░░░░░░░░░░░░░░░░░░░░░▒▒░░░░░░░░░░░░██▓▓                        
            ██▓▓░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░▓▓▓▓░░░░░░░░░░██                      
        ████░░░░░░░░░░░░░░░░░░░░░░░░░░░░▒▒▒▒▒▒████▓▓▓▓██░░░░░░░░██                      
      ██░░░░░░░░░░░░░░░░░░░░░░░░░░░░▒▒▒▒░░████▓▓▓▓▓▓▓▓██░░░░██░░██                      
    ██░░▒▒▒▒░░░░░░░░░░░░░░░░░░░░▒▒▒▒░░████▓▓▓▓▓▓▓▓▓▓▓▓██░░████░░██                      
    ██░░░░░░▒▒▒▒░░░░░░░░░░░░▒▒▒▒░░████▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓██░░██░░░░██                      
    ██░░░░░░░░░░░░░░░░░░░░░░░░██▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓██░░░░▒▒░░██                      
    ██░░░░░░░░░░░░░░▒▒▒▒░░████▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓██░░▒▒░░░░██                      
    ██░░░░░░░░░░░░░░░░░░██▓▓▓▓▓▓░░░░▓▓░░▓▓▓▓▓▓▓▓▓▓▓▓▓▓██░░░░██░░██                      
    ██░░░░░░░░░░░░░░░░░░██▓▓▓▓░░▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓██░░████░░██                      
    ██░░░░░░░░░░░░░░░░░░██▓▓▓▓░░▓▓▓▓▓▓░░▓▓▓▓▓▓▓▓▓▓▓▓▓▓██░░████░░██                      
    ██░░░░░░░░░░░░░░░░░░██▓▓▓▓▒▒░░░░▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓██░░██░░░░██                      
    ██░░░░░░░░░░░░░░░░░░██▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓██░░░░░░░░██                      
    ██░░░░░░░░░░░░░░░░░░██▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓██░░░░░░░░██                      
    ██░░░░░░░░░░░░░░░░░░██▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓████░░░░░░████                        
    ██░░░░░░░░░░░░░░░░░░██▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓████░░░░░░████▒▒██                        
    ██░░░░░░░░░░░░░░░░░░██▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓████░░░░░░▓▓▓▓▒▒▒▒▒▒██████                    
    ██░░░░░░░░░░░░░░░░░░██▓▓▓▓▓▓▓▓▓▓▓▓████░░░░░░▓▓▓▓▒▒▒▒▒▒████░░░░░░▓▓                  
    ██░░░░░░░░░░░░░░░░░░██▓▓▓▓▓▓▓▓██▓▓░░░░░░▓▓▓▓▒▒▒▒▒▒████░░░░░░▓▓▓▓██                  
    ████░░░░░░░░░░░░░░░░██▓▓▓▓████░░░░░░████▒▒▒▒▒▒████░░░░░░████░░░░░░██                
      ██████░░░░░░░░░░░░░░████░░░░░░████▒▒▒▒▒▒████░░░░░░████░░░░████░░░░██              
    ██▒▒▒▒▒▒████░░░░░░░░░░░░░░░░████▒▒▒▒▒▒██▓▓░░░░░░████░░░░████░░░░▓▓░░░░██            
    ██████▒▒▒▒▒▒▓▓██░░░░░░░░▓▓██▒▒▒▒▒▒████░░░░░░▓▓██░░░░▓▓▓▓░░░░░░░░░░██░░░░▓▓          
    ██░░░░████▒▒▒▒▒▒████████▒▒▒▒▒▒████░░░░░░██▓▓░░░░████░░░░░░░░░░░░░░░░██░░░░██        
    ██░░░░░░░░████▒▒▒▒▒▒▒▒▒▒▒▒████░░░░░░████░░░░████░░░░░░░░░░░░░░░░░░░░░░██░░░░██      
    ██░░░░░░░░░░░░████████████░░░░░░████░░░░██▓▓░░░░░░░░░░░░░░░░░░░░░░░░░░░░██░░░░██    
  ██░░░░░░░░░░░░░░░░░░░░░░░░░░░░████░░░░████░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░██░░░░██  
  ██░░░░░░░░░░░░░░░░░░░░░░░░████░░░░████░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░████░░░░██████
  ██░░░░░░░░░░░░░░░░░░░░░░▓▓░░██░░░░██░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░▓▓▓▓░░░░▓▓▓▓░░░░██
  ████░░░░░░░░░░░░░░░░░░▓▓░░░░░░██░░░░██░░░░░░░░░░░░░░░░░░░░░░░░░░████░░░░████░░░░░░░░██
      ████░░░░░░░░░░░░░░▓▓░░░░░░░░██░░░░██░░░░░░░░░░░░░░░░░░░░████░░░░████░░░░░░░░░░░░██
          ████░░░░░░░░░░▓▓░░░░░░░░░░██░░░░██░░░░░░░░░░░░░░████░░░░████░░░░░░░░░░░░████  
              ████░░░░░░▓▓░░░░░░░░░░░░██░░░░██░░░░░░░░████░░░░████░░░░░░░░░░░░████      
                  ▓▓██░░▓▓░░░░░░░░░░░░░░██░░░░▓▓░░▓▓▓▓░░░░▓▓██░░░░░░░░░░░░████          
                      ██▓▓░░░░░░░░░░░░░░▒▒▓▓░░░░██░░▒▒▓▓▓▓░░░░░░░░░░░░▓▓██              
                        ░░▓▓▓▓░░░░░░░░░░░░▒▒▒▒░░▒▒▓▓▓▓▒▒░░░░░░░░░░▓▓▒▒               
                              ████░░░░░░░░░░▒▒████░░░░░░░░░░░░████                      
                                  ████░░░░░░░░░░░░░░░░░░░░████                          
                                      ████░░░░░░░░░░░░████                              
                                          ▓▓▓▓░░░░▓▓██                                  
                                              ▓▓▓▓                                    
", "display", "Shell configs >> Display settings");

    add_shell_button("⠀⠀⢀⣤⣶⣿⣿⣿⣿⣿⣿⣿⣿⣿⣿⣿⣿⣿⣿⣶⣤⡀⠀⠀
⠀⣴⣿⣿⡿⠟⠛⠛⠻⢿⣿⣿⣿⣿⣿⣿⣿⣿⣿⣿⣿⣿⣦⠀
⣸⣿⣿⠏⠀⠀⠀⠀⠀⠀⠹⣿⣿⣿⣿⣿⣿⣿⣿⣿⣿⣿⣿⣇
⣿⣿⣿⠀⠀⠀⠀⠀⠀⠀⠀⣿⣿⣿⣿⣿⣿⣿⣿⣿⣿⣿⣿⣿
⢹⣿⣿⣆⠀⠀⠀⠀⠀⠀⣰⣿⣿⣿⣿⣿⣿⣿⣿⣿⣿⣿⣿⡏
⠀⠻⣿⣿⣷⣦⣤⣤⣴⣾⣿⣿⣿⣿⣿⣿⣿⣿⣿⣿⣿⣿⠟⠀
⠀⠀⠈⠛⠿⣿⣿⣿⣿⣿⣿⣿⣿⣿⣿⣿⣿⣿⣿⠿⠛⠁⠀⠀" , "switches", "Shell configs >> Miscellaneous Settings");

    add_shell_button("BBBBBBBBBBBBBBBBBBBBBBBBBBB
BMB---------------------B B
BBB---------------------BBB
BBB---------------------BBB
BBB---------------------BBB
BBB---------------------BBB
BBB---------------------BBB
BBBBBBBBBBBBBBBBBBBBBBBBBBB
BBBBB++++++++++++++++BBBBBB
BBBBB++BBBBB+++++++++BBBBBB
BBBBB++BBBBB+++++++++BBBBBB
BBBBB++BBBBB+++++++++BBBBBB
BBBBB++++++++++++++++BBBBBB", "startup", "Shell configs >> Startup sound settings");


    let shell_stack_clone_back: Stack = shell_stack.clone();
    let page_title_clone = page_title.clone();

    shell_settings_scroller.set_child(Some(&shell_settings_box));
    shell_stack.add_titled(&shell_settings_scroller, Some("shell_settings"), "cynide shell settings");

    // monitor
    let monitor_box = GtkBox::new(Orientation::Vertical, 10);
    let fixed = Fixed::new();
    fixed.set_size_request(4000, 3000); // Large canvas for multiple monitors

    let scrolled: gtk4::ScrolledWindow = gtk4::ScrolledWindow::new();
    scrolled.set_policy(gtk4::PolicyType::Automatic, gtk4::PolicyType::Automatic);
    scrolled.set_hexpand(true);
    scrolled.set_vexpand(true);
    scrolled.set_css_classes(&["display_win"]);

    scrolled.set_child(Some(&fixed));
    let play_frame_monitors = GtkBox::new(Orientation::Horizontal, 5);
    play_frame_monitors.set_vexpand(true);
    let play_frame_key_info = Label::new(Some("Select a display\nUse arrow keys to move\nControl_L to rotate display"));
    play_frame_key_info.set_vexpand(true);
    play_frame_key_info.set_valign(gtk4::Align::Baseline);

    play_frame_monitors.append(&scrolled);
    play_frame_monitors.append(&play_frame_key_info);
    monitor_box.append(&play_frame_monitors);

    // Load monitors
    let monitors = Rc::new(RefCell::new(load_monitoors(&fixed)));

    // Save button
    let save_button = Button::with_label("Save Layout");
    monitor_box.append(&save_button);

    // Save button logic
    let monitors_clone = monitors.clone();
    let save_button_clone = save_button.clone();
    save_button.connect_clicked(move |_| {
        save_monitor_layout(&monitors_clone.borrow(), &save_button_clone);
    });
    

    // switches
    let switch_box = GtkBox::new(Orientation::Vertical, 5);
    
    let switch_grid = gtk4::Grid::builder()
        .column_homogeneous(true)
        .row_homogeneous(true)
        .column_spacing(10)
        .row_spacing(10)
        .build();

    switch_box.append(&switch_grid);

    // themeswitch 
    let theme_switch = gtk4::Switch::builder().build();
    theme_switch.set_halign(gtk4::Align::Start);
    setup_switch(&theme_switch);
    let theme_switch_label = Label::new(Some("Dark / Light Theme switch"));
    theme_switch_label.set_halign(gtk4::Align::Start);

    let output = Command::new("sh")
        .arg("-c")
        .arg("gsettings get org.gnome.desktop.interface color-scheme")
        .output();
    if let Ok(output) = output {
        let prefer_output = String::from_utf8_lossy(&output.stdout).trim().to_string();
        theme_switch.set_state(prefer_output == "'prefer-light'");
    }


    switch_grid.attach(&theme_switch_label, 0, 0, 1, 1);
    switch_grid.attach(&theme_switch, 1, 0, 1, 1);

    // notification sound
    let notiv_sound_label = Label::new(Some("Notifications sound toggle switch"));
    notiv_sound_label.set_halign(gtk4::Align::Start);
    let notiv_sound_switch = gtk4::Switch::builder().build();
    notiv_sound_switch.set_halign(gtk4::Align::Start);
    setup_sound_switch(&notiv_sound_switch);

    let output = Command::new("cynagectl")
        .arg("-n")
        .output();
    if let Ok(output) = output {
        let prefer_output = String::from_utf8_lossy(&output.stdout).trim().to_string();
        notiv_sound_switch.set_state(prefer_output == "toggle-on");
    }
    
    switch_grid.attach(&notiv_sound_label, 0, 1, 1, 1);
    switch_grid.attach(&notiv_sound_switch, 1, 1, 1, 1);

    // start up sound

    let startup_box = GtkBox::new(Orientation::Vertical, 10);
    let select_startup_label = Label::new(Some("Select startup sound"));
    select_startup_label.set_halign(gtk4::Align::Start);
    let sound_button_box = GtkBox::new(Orientation::Horizontal, 5);

    startup_box.append(&select_startup_label);
    startup_box.append(&sound_button_box);

    let config_path = format!("{}/.config/hypr/startup.conf", home_dir);
    let sound_dir = PathBuf::from(format!("{}/.config/hypr/sound/startup", home_dir));

    let selected_button: Rc<RefCell<Option<Button>>> = Rc::new(RefCell::new(None));

    if let Ok(entries) = fs::read_dir(&sound_dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_file() {
                let filename = path.file_name().unwrap().to_string_lossy().to_string();

                let btn = Button::with_label(&filename);
                btn.set_css_classes(&["sound_btn"]);
                btn.set_size_request(100, 100);
                btn.set_valign(gtk4::Align::Center);
                btn.set_halign(gtk4::Align::Center);

                // Initial highlight if currently selected
                if let Some(current_file) = extract_current_selection(&config_path) {
                    if current_file == filename {
                        btn.add_css_class("sound_btn_selected");
                        *selected_button.borrow_mut() = Some(btn.clone());
                    }
                }

                let selected_button_clone = selected_button.clone();
                let config_path_clone = config_path.clone();
                let filename_clone = filename.clone();
                let notiv_box_clone_sound = notif_box.clone();

                btn.connect_clicked(move |btn_ref| {
                    // Deselect previous button
                    if let Some(prev_btn) = selected_button_clone.borrow_mut().take() {
                        prev_btn.remove_css_class("sound_btn_selected");
                    }

                    // Select current
                    btn_ref.add_css_class("sound_btn_selected");
                    *selected_button_clone.borrow_mut() = Some(btn_ref.clone());

                    // Write config
                    let conf_content = format!(
                        "exec-once = mpv --no-video --volume=100 \"$HOME/.config/hypr/sound/startup/{}\"\n",
                        filename_clone
                    );
                    if let Ok(mut file) = OpenOptions::new()
                        .write(true)
                        .truncate(true)
                        .create(true)
                        .open(&config_path_clone)
                    {
                        let _ = file.write_all(conf_content.as_bytes());
                    }

                    show_notification(&notiv_box_clone_sound, "Startup Sound Modified");

                });

                sound_button_box.append(&btn);
            }
        }
    }

    shell_stack.add_titled(&monitor_box, Some("display"), "Display_settings");
    shell_stack.add_titled(&switch_box, Some("switches"), "Misc");
    shell_stack.add_titled(&startup_box, Some("startup"), "startup_sound");

    stack.add_titled(&shell_stack, Some("cynide"), "Cynide Settings");

    // network settings ------------------------------------------------------------------------------------------------------------------------------- //
    let net_stack = Stack::new();
    
    let network_home = GtkBox::new(Orientation::Vertical, 0);
    network_home.set_vexpand(true);
    network_home.set_hexpand(true);
    network_home.set_valign(gtk4::Align::Fill);
    network_home.set_halign(gtk4::Align::Fill);
    let nm_ctrl = GtkBox::new(Orientation::Horizontal, 7);
    nm_ctrl.set_hexpand(true);

    let nm_toggle = Switch::new();
    nm_toggle.set_active(getty_wifi_status());
    nm_toggle.set_halign(gtk4::Align::End);

    nm_toggle.connect_state_set(|_, state| {
        setty_wifi_enabled(state);
        glib::Propagation::Proceed
    });

    let nm_edit_button = Button::with_label("Edit Saved Connections");
    nm_edit_button.set_halign(gtk4::Align::Start);
    nm_edit_button.set_hexpand(true);

    nm_ctrl.append(&nm_edit_button);
    nm_ctrl.append(&nm_toggle);

    let nm_list_scroller = ScrolledWindow::builder()
        .min_content_height(150)
        .hscrollbar_policy(gtk4::PolicyType::Never)
        .vscrollbar_policy(gtk4::PolicyType::Automatic)
        .hexpand(true)
        .vexpand(true)
        .halign(gtk4::Align::Fill)
        .valign(gtk4::Align::Fill)
        .build();
    let network_list = GtkBox::new(Orientation::Vertical, 10);
    nm_list_scroller.set_child(Some(&network_list));
    nm_list_scroller.set_css_classes(&["display_win"]);

    network_home.append(&nm_ctrl);
    network_home.append(&nm_list_scroller);

    let saved_scroller = ScrolledWindow::builder()
        .min_content_height(140)
        .hscrollbar_policy(gtk4::PolicyType::Never)
        .vscrollbar_policy(gtk4::PolicyType::Automatic)
        .hexpand(true)
        .vexpand(true)
        .build();
    let saved_list = GtkBox::new(Orientation::Vertical, 0);
    saved_scroller.set_child(Some(&saved_list));

    network_home.append(&saved_scroller);


    let vte_box = GtkBox::new(Orientation::Vertical, 0);
    vte_box.set_hexpand(true);
    vte_box.set_vexpand(true);
    vte_box.set_halign(gtk4::Align::Center);
    vte_box.set_valign(gtk4::Align::Center);
    vte_box.set_css_classes(&["wall_s"]);

    let vte_term = Terminal::default();
    let fg = gtk4::gdk::RGBA::new(0.0, 1.0, 0.66, 1.0);
    let bg = gtk4::gdk::RGBA::new(0.0, 0.0, 0.0, 1.0);

    let palette_owned: Vec<gtk4::gdk::RGBA> = vec![
        bg.clone(),   // Black
        bg.clone(), // Red (here it's green)
        fg.clone(),   // Green
        fg.clone(),   // Yellow
        fg.clone(),   // Blue
        fg.clone(),   // Magenta
        fg.clone(),   // Cyan
        fg.clone(),   // White
    ];
    let palette: Vec<&gtk4::gdk::RGBA> = palette_owned.iter().collect();
    vte_term.set_colors(Some(&fg), Some(&bg), &palette);

    // vte_term.spawn_async(
    //     PtyFlags::DEFAULT,
    //     None,      
    //     &["nmtui", "edit"],          
    //     &[],            
    //     gtk4::glib::SpawnFlags::DEFAULT,
    //     || {},               
    //     -1,                      
    //     None::<&gtk4::gio::Cancellable>,  
    //     move |res: Result<gtk4::glib::Pid, gtk4::glib::Error>| {
    //         if let Err(e) = res {
    //             eprintln!("Failed to spawn terminal process: {}", e);
    //         }
    //     },
    // );

    vte_box.append(&vte_term);
    net_stack.add_titled(&network_home, Some("home"), "Network Home");
    net_stack.add_titled(&vte_box, Some("edit"), "Edit Saved");

    let back_button_edit = back_button.clone();

    let stack_weak = net_stack.downgrade();
    let page_title_clone_edit = page_title.clone();

    
    nm_edit_button.connect_clicked(move |_| {
        if let Some(stack) = stack_weak.upgrade() {    
            stack.set_visible_child_name("edit");
            typing_effect(&page_title_clone_edit, "network settings >> edit saved connections", 10);
            back_button_edit.set_visible(true);
        }
    });

    // Timer for 1s refresh
    let network_list_clone = network_list.clone();
    glib::timeout_add_seconds_local(1, move || {
        let conn = dbus::blocking::Connection::new_system().unwrap();
        refresh_wifi_listty(&conn, &network_list_clone);
        glib::ControlFlow::Continue
    });


    stack.add_titled(&net_stack, Some("network"), "Network Settings");

    // window ----------------------------------------------------------------------------------------------------------------------------------------- //
    let back_button_clone = back_button.clone();
    let stack_clone = stack.clone();
    let net_stack_clone = net_stack.clone();
    back_button.connect_clicked(move |_| {
        if let Some(visible_name) = stack_clone.visible_child_name() {
            match visible_name.as_str() {
                "network" => {
                    net_stack_clone.set_visible_child_name("home");
                    typing_effect(&page_title_clone, "network", 10);
                    back_button_clone.set_visible(false);
                }
                "cynide" => {
                    shell_stack_clone_back.set_visible_child_name("shell_settings");
                    typing_effect(&page_title_clone, "Shell Configs", 10);
                    back_button_clone.set_visible(false);
                }
                _ => {
                    eprint!("not in the required page");
                }
            }
        }
    });

    main_box.append(&header_box);
    main_box.append(&stack_box);
    main_box.append(&footer);
    window.set_child(Some(&main_box));
    window.present();

}

fn main() {
    let app = Application::builder()
        .application_id("ekah.scu.calibrate")
        .build();
    app.connect_activate(|app| {
        load_css();
        build_ui(app);
    });
    app.run();
}
