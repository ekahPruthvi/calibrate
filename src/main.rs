use gtk4::{
    prelude::*, Switch, Frame, Application, ApplicationWindow, Box as GtkBox, Button, Image, Label, Orientation, Stack, GestureDrag,
    Fixed, MessageDialog , gdk::Display , CssProvider, glib, ResponseType, Widget, DrawingArea, Dialog, FileChooserDialog, FileChooserAction
};
use std::{cell::RefCell, fs, path::PathBuf, process::Command, rc::Rc};
use std::collections::HashMap;
use std::env;
use std::io::{Write, BufReader, BufRead};
use std::fs::OpenOptions;
use std::path::Path;

struct MonitorInfo {
    name: String,
    width: i32,
    height: i32,
    frame: Frame,
}

const SCALE: f64 = 0.1;
const SNAP_SIZE: i32 = 50; 

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

        for line in lines {
            if line.contains(" at ") {
                let parts: Vec<_> = line.trim().split(" at ").collect();
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
        }

        let frame = Frame::builder()
            .width_request((width as f64 * SCALE) as i32)
            .height_request((height as f64 * SCALE) as i32)
            .build();

        frame.set_child(Some(&Label::new(Some(name))));

        fixed.put(&frame, pos_x as f64 * SCALE, pos_y as f64 * SCALE);

        enable_dragging(&frame, fixed);

        monitors.insert(
            name.to_string(),
            MonitorInfo { name: name.to_string(), width, height, frame: frame.clone() },
        );
    }

    monitors
}

fn enable_dragging(frame: &Frame, fixed: &Fixed) {
    let start_offset = Rc::new(RefCell::new((0.0, 0.0)));
    let grid_lines = Rc::new(RefCell::new(Vec::new()));

    let drag = GestureDrag::new();
    drag.set_button(0); // allow any button

    drag.connect_drag_begin({
        let frame = frame.clone();
        let fixed = fixed.clone();
        let start_offset = start_offset.clone();
        let grid_lines = grid_lines.clone();

        move |_gesture, start_x, start_y| {
            // Set dragging cursor
            frame.set_cursor_from_name(Some("grab"));

            let frame_widget: &Widget = frame.as_ref();
            let fixed_widget: &Widget = fixed.as_ref();
            if let Some((fx, fy)) = frame_widget.translate_coordinates(fixed_widget, 0.0, 0.0) {
                start_offset.replace((fx - start_x, fy - start_y));
            }

            // Draw snap grid
            let parent_alloc = fixed.allocation();
            let mut lines = Vec::new();

            for x in (0..parent_alloc.width()).step_by((SNAP_SIZE as f64 * SCALE) as usize) {
                let l = DrawingArea::builder()
                    .width_request(1)
                    .height_request(parent_alloc.height())
                    .css_classes(vec!["grid-line"])
                    .build();
                fixed.put(&l, x as f64, 0.0);
                l.show();
                lines.push(l);
            }

            for y in (0..parent_alloc.height()).step_by((SNAP_SIZE as f64 * SCALE) as usize) {
                let l = DrawingArea::builder()
                    .width_request(parent_alloc.width())
                    .height_request(1)
                    .css_classes(vec!["grid-line"])
                    .build();
                fixed.put(&l, 0.0, y as f64);
                l.show();
                lines.push(l);
            }

            grid_lines.replace(lines);
        }
    });

    drag.connect_drag_update({
        let frame = frame.clone();
        let fixed = fixed.clone();
        let start_offset = start_offset.clone();

        move |_gesture, x, y| {
            let (offset_x, offset_y) = *start_offset.borrow();
            let new_x = x + offset_x;
            let new_y = y + offset_y;

            let parent_alloc = fixed.allocation();
            let frame_alloc = frame.allocation();

            let clamped_x = new_x.clamp(0.0, (parent_alloc.width() - frame_alloc.width()) as f64);
            let clamped_y = new_y.clamp(0.0, (parent_alloc.height() - frame_alloc.height()) as f64);

            fixed.move_(&frame, clamped_x, clamped_y);
        }
    });

    drag.connect_drag_end({
        let frame = frame.clone();
        let fixed = fixed.clone();
        let start_offset = start_offset.clone();
        let grid_lines = grid_lines.clone();

        move |_gesture, release_x, release_y| {
            frame.set_cursor_from_name(None); // Restore cursor

            let (offset_x, offset_y) = *start_offset.borrow();
            let target_x = release_x + offset_x;
            let target_y = release_y + offset_y;

            let parent_alloc = fixed.allocation();
            let frame_alloc = frame.allocation();

            let clamped_x = target_x.clamp(0.0, (parent_alloc.width() - frame_alloc.width()) as f64);
            let clamped_y = target_y.clamp(0.0, (parent_alloc.height() - frame_alloc.height()) as f64);

            let snapped_x = ((clamped_x / (SCALE * SNAP_SIZE as f64)).round() * (SCALE * SNAP_SIZE as f64)).round();
            let snapped_y = ((clamped_y / (SCALE * SNAP_SIZE as f64)).round() * (SCALE * SNAP_SIZE as f64)).round();

            fixed.move_(&frame, snapped_x, snapped_y);

            // Remove grid lines
            for l in grid_lines.borrow().iter() {
                fixed.remove(l);
            }
            grid_lines.borrow_mut().clear();
        }
    });

    frame.add_controller(drag);
}


fn save_monitor_layout(monitors: &HashMap<String, MonitorInfo>, parent_widget: &impl IsA<gtk4::Widget>) {
    let mut config = String::new();

    for monitor in monitors.values() {
        let alloc = monitor.frame.allocation();
        let pos_x = (alloc.x() as f64 / SCALE).round() as i32;
        let pos_y = (alloc.y() as f64 / SCALE).round() as i32;
        config.push_str(&format!(
            "monitor = {}, {}x{}, {}x{}, 1\n",
            monitor.name, monitor.width, monitor.height, pos_x, pos_y
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

            // Optional: Live reload hyprland
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
        // Allow the state change
        gtk4::glib::Propagation::Proceed
    });
}

fn is_notifications_sound() -> bool {
    let output = Command::new("sh")
        .arg("-c")
        .arg("cynage -n")
        .output();

    if let Ok(output) = output {
        let prefer_output = String::from_utf8_lossy(&output.stdout).trim().to_string();
        return prefer_output != "toggle-on";
    }
    true  
}

fn setup_sound_switch(switch: &Switch) {
    let is_on = is_notifications_sound();
    switch.set_active(is_on);

    switch.connect_state_set(|_, state| {
        let cmd = if state {
            "cynagectl -n true"
        } else {
            "cynagectl -n false"
        };

        // Run the command
        let _ = Command::new("sh").arg("-c").arg(cmd).spawn();
        // Allow the state change
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

pub fn list_wifi_networks_clickable(container: &GtkBox) {
    let output = Command::new("nmcli")
        .args(["-t", "-f", "IN-USE,SSID,SIGNAL", "dev", "wifi"])
        .output()
        .expect("Failed to run nmcli");

    let stdout = String::from_utf8_lossy(&output.stdout);

    while let Some(child) = container.first_child() {
        container.remove(&child);
    }

    // Store buttons so we can later update their CSS classes
    let buttons: Rc<RefCell<Vec<Button>>> = Rc::new(RefCell::new(Vec::new()));

    for line in stdout.lines() {
        let parts: Vec<&str> = line.split(':').collect();
        if parts.len() != 3 {
            continue;
        }

        let in_use = parts[0].trim();
        let ssid = parts[1].trim();
        let signal: u32 = parts[2].trim().parse().unwrap_or(0);

        if ssid.is_empty() {
            continue;
        }

        let bars = match signal {
            75..=100 => "████",
            50..=74 => "███",
            25..=49 => "██",
            1..=24 => "█",
            _ => "",
        };

        let label_text = Label::new(Some(&ssid));
        label_text.set_hexpand(true);
        label_text.set_halign(gtk4::Align::Start);
        let label_bars = Label::new(Some(&bars));
        label_bars.set_hexpand(true);
        label_bars.set_halign(gtk4::Align::End);
        let label_box = GtkBox::new(Orientation::Horizontal, 0);
        label_box.set_hexpand(true);
        label_box.append(&label_text);
        label_box.append(&label_bars);
        
        let btn = Button::builder().child(&label_box).build();
        btn.set_hexpand(true);
        btn.set_halign(gtk4::Align::Fill);

        if in_use == "*" {
            btn.add_css_class("connected");
            label_text.set_css_classes(&["network_label"]);
            label_bars.set_css_classes(&["network_label"]);
        }

        let ssid_cloned = ssid.to_string();
        let btn_cloned = btn.clone();
        let buttons_ref = Rc::clone(&buttons);

        btn.connect_clicked(move |_| {
            let result = Command::new("nmcli")
                .args(["dev", "wifi", "connect", &ssid_cloned, "--ask"])
                .output()
                .expect("Failed to run nmcli connect");

            let (title, msg) = if result.status.success() {
                ("Connected", format!("Successfully connected to \"{}\".", ssid_cloned))
            } else {
                (
                    "Connection Failed",
                    format!(
                        "Failed to connect to \"{}\":\n{}",
                        ssid_cloned,
                        String::from_utf8_lossy(&result.stderr)
                    ),
                )
            };

            // Find the top-level window
            let parent_win = btn_cloned
                .root()
                .and_then(|w| w.downcast::<ApplicationWindow>().ok())
                .unwrap();

            let dialog = Dialog::builder()
                .transient_for(&parent_win)
                .modal(true)
                .title(title)
                .build();

            dialog.content_area().append(&Label::new(Some(&msg)));
            dialog.add_button("OK", ResponseType::Ok);
            dialog.connect_response(move |d, _| {
                d.close();
            });
            dialog.show();

            // If success, update CSS classes
            if result.status.success() {
                for b in buttons_ref.borrow().iter() {
                    b.remove_css_class("connected");
                    label_text.remove_css_class("network_label");
                    label_text.remove_css_class("network_label");
                }
                btn_cloned.add_css_class("connected");
            }
        });

        container.append(&btn);
        buttons.borrow_mut().push(btn);
    }
}

fn gett_wifi_status() -> bool {
    let output = Command::new("nmcli")
        .args(["radio", "wifi"])
        .output()
        .expect("Failed to get Wi-Fi status");
    String::from_utf8_lossy(&output.stdout).trim() == "enabled"
}

fn sett_wifi_status(enabled: bool) {
    let _ = Command::new("nmcli")
        .args(["radio", "wifi", if enabled { "on" } else { "off" }])
        .output();
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
            background-color: rgb(5, 148, 122);
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

        .home_page {
            margin: 30px;
            padding: 10px;
        }

        #wall-dialog {
            background-color: #1e1e1e;
            border: 2px solid #888;
            border-radius: 15px;
            padding: 0px;
        }

        #wall-dialog button {
            background-color: #333;
            color: #fff;
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
    let wallpaper_box = GtkBox::builder().orientation(Orientation::Vertical).spacing(0).build();

    let output = Command::new("/home/ekah/.config/hypr/scripts/swwwallpaper.sh")
        .output()
        .expect("Failed to run swwwallpaper.sh");

    let stdout = String::from_utf8_lossy(&output.stdout);
    let line = stdout.trim();

    let parts: Vec<&str> = line.split(':').collect();
    let display_name = parts[0].trim().to_string();

    let rest = parts[1];
    let resolution_part = rest.split(',').next().unwrap().trim().to_string();

    let image_path_part = line.split("image:").nth(1).unwrap().trim();
    let image_filename = image_path_part
        .rsplit('/')
        .next()
        .unwrap()
        .replace(".blur", "");

    let display_label = gtk4::Label::new(Some(&format!("Display: {}", display_name)));
    display_label.set_justify(gtk4::Justification::Right);
    display_label.set_halign(gtk4::Align::End);
    display_label.set_hexpand(true);
    let resolution_label = gtk4::Label::new(Some(&format!("Resolution: {}", resolution_part)));
    resolution_label.set_justify(gtk4::Justification::Right);
    resolution_label.set_halign(gtk4::Align::End);
    resolution_label.set_hexpand(true);

    let image_path = format!("{}/.config/swww/cynage/{}", home_dir, image_filename);

    let file = gtk4::gio::File::for_path(image_path);
    let texture = gtk4::gdk::Texture::from_file(&file).unwrap();
    let current_pic = gtk4::Picture::for_paintable(&texture);
    let current_pic_ref = Rc::new(RefCell::new(current_pic.clone()));

    let current_wall = GtkBox::new(Orientation::Horizontal, 2);

    let wall_info = GtkBox::new(Orientation::Vertical, 10);
    wall_info.set_hexpand(true);
    wall_info.set_halign(gtk4::Align::Fill);

    let wall_buttons = GtkBox::new(Orientation::Horizontal, 5);
    wall_buttons.set_hexpand(true);
    wall_buttons.set_vexpand(true);
    wall_buttons.set_valign(gtk4::Align::Baseline);
    wall_buttons.set_halign(gtk4::Align::End);
    let add_wall = Button::builder().child(&Label::new(Some("Add wallpapers"))).build();
    let remove_wall = Button::builder().child(&Label::new(Some("Remove wallpaper"))).build();
    let vdummy_forwallinfo = GtkBox::new(Orientation::Vertical, 15);
    vdummy_forwallinfo.set_vexpand(true);

    wall_buttons.append(&add_wall);
    wall_buttons.append(&remove_wall);

    wall_info.append(&display_label);
    wall_info.append(&resolution_label);
    wall_info.append(&vdummy_forwallinfo);
    wall_info.append(&wall_buttons);

    current_wall.append(&current_pic);
    current_wall.append(&wall_info);
    current_wall.set_widget_name("current_wall");


    let scrolled_window = gtk4::ScrolledWindow::builder()
        .min_content_height(150)
        .hscrollbar_policy(gtk4::PolicyType::Automatic)
        .vscrollbar_policy(gtk4::PolicyType::Never)
        .hexpand(true)
        .vexpand(false)
        .build();
    scrolled_window.set_css_classes(&["wall_s"]);

    let image_grid = GtkBox::builder()
        .orientation(gtk4::Orientation::Horizontal)
        .spacing(10)
        .build();

    scrolled_window.set_child(Some(&image_grid));

    wallpaper_box.append(&current_wall);
    wallpaper_box.append(&scrolled_window);

    // Load images dynamically
    let add_walls_to_grid = |boxxy: &GtkBox, notiv_boxxy: &GtkBox, picc_ref: &Rc<RefCell<gtk4::Picture>>| {
        while let Some(child) = boxxy.first_child() {
            boxxy.remove(&child);
        }
        let home_dir = std::env::var("HOME").unwrap();
        let wallpaper_dir = PathBuf::from(format!("{}/.config/swww/cynage", home_dir));
        if let Ok(entries) = fs::read_dir(wallpaper_dir.clone()) {
            let notiv_clone_outer_for_wall = notiv_boxxy.clone();
            for entry in entries.flatten() {
                let path = entry.path();
                if path.is_file() {
                    let filename = path.file_name().unwrap().to_string_lossy().to_string();
                    let img_path = path.clone();
                    let btn = Button::builder().build();
                    btn.set_css_classes(&["walls"]);
                    let pixbuf = gtk4::gdk_pixbuf::Pixbuf::from_file_at_scale(img_path, 260, 260, true).ok();
                    if let Some(pix) = pixbuf {
                        let image = Image::from_pixbuf(Some(&pix));
                        image.set_pixel_size(260);
                        image.add_css_class("thumbnail");
                        btn.set_child(Some(&image));
                    }

                    // Clicking image button to execute script
                    let home_dir_cloned = home_dir.clone();
                    let filename_clone = filename.clone();
                    let current_pic_clone = picc_ref.clone();
                    let notiv_clone_for_wall = notiv_clone_outer_for_wall.clone(); 
                    btn.connect_clicked(move |_| {
                        let target_path = format!("{}/.config/swww/cynage/{}", home_dir_cloned, filename_clone);
                        let script_path = format!("{}/.config/hypr/scripts/swwwallpaper.sh", home_dir_cloned);
                        let _ = Command::new(script_path).arg("-s").arg(&target_path).spawn();
                        let file = gtk4::gio::File::for_path(&target_path);
                        if let Ok(texture) = gtk4::gdk::Texture::from_file(&file) {
                            let new_pic = gtk4::Picture::for_paintable(&texture);
                            new_pic.set_hexpand(true);
                            new_pic.set_vexpand(true);
                            new_pic.set_halign(gtk4::Align::Fill);
                            new_pic.set_valign(gtk4::Align::Fill);
                            current_pic_clone.borrow_mut().set_paintable(Some(&texture));
                        }
                        let now = is_image_dark(&target_path);
                        let prefer_output = is_system_theme_light();
                        if now && prefer_output {
                            show_notification(&notiv_clone_for_wall, "wallpaper changed, dark wallpaper detected");
                            let _ = Command::new("cynagectl").arg("-s").arg("dark").spawn();
                        } else if now && !prefer_output {
                            show_notification(&notiv_clone_for_wall, "wallpaper changed");
                        } else if !now && !prefer_output {
                            show_notification(&notiv_clone_for_wall, "wallpaper changed, Light wallpaper detected");
                            let _ = Command::new("cynagectl").arg("-s").arg("light").spawn();
                        } else {
                            show_notification(&notiv_clone_for_wall, "wallpaper changed");
                        }
                    });
                    boxxy.append(&btn);
                }
            }
        }
    };

    add_walls_to_grid(&image_grid, &notif_box, &current_pic_ref);
    let image_grid_clone = image_grid.clone();
    let notif_box_clone = notif_box.clone();
    let window_clone = window.clone();
    add_wall.connect_clicked(move |_| {
        let notif_box_clone = notif_box_clone.clone();
        let curren_pic_ref_clone = current_pic_ref.clone();
        let dialog = FileChooserDialog::new(
            Some("Choose a wallpaper"),
            Some(&window_clone),
            FileChooserAction::Open,
            &[("_Cancel", ResponseType::Cancel), ("_Add", ResponseType::Accept)],
        );
        dialog.set_widget_name("wall-dialog");

        let image_grid_inner = image_grid_clone.clone();
        dialog.connect_response(move |dialog, response| {
            if response == ResponseType::Accept {
                if let Some(file_path) = dialog.file().and_then(|f| f.path()) {
                    let _ = Command::new("cynagectl")
                        .args(["-w", "add", file_path.to_str().unwrap_or_default()])
                        .spawn();
                }
            }
            dialog.close();
            add_walls_to_grid(&image_grid_inner, &notif_box_clone, &curren_pic_ref_clone);
        });

        dialog.show();
    });
    
    let window_clone2 = window.clone();
    let image_grid_clone = image_grid.clone();
    let notif_box_clone2 = notif_box.clone();
    let current_pic_ref2 = Rc::new(RefCell::new(current_pic.clone()));
    remove_wall.connect_clicked(move |_| {
        let notif_box_clone2 = notif_box_clone2.clone();
        let curren_pic_ref_clone2 = current_pic_ref2.clone();
        let dialog = FileChooserDialog::new(
            Some("Choose wallpaper to remove"),
            Some(&window_clone2),
            FileChooserAction::Open,
            &[("_Cancel", ResponseType::Cancel), ("_Remove", ResponseType::Accept)],
        );

        dialog.set_widget_name("wall-dialog");

        let image_grid_inner = image_grid_clone.clone();
        if let Some(home_dir) = std::env::var_os("HOME") {
            let start_path = Path::new(&home_dir).join(".config/swww/cynage/");
            let _ = dialog.set_current_folder(Some(&gtk4::gio::File::for_path(start_path)));
        }

        dialog.connect_response(move |dialog, response| {
            if response == ResponseType::Accept {
                if let Some(file_path) = dialog.file().and_then(|f| f.path()) {
                    if let Some(file_stem) = file_path.file_stem().and_then(|s| s.to_str()) {
                        let _ = Command::new("cynagectl")
                            .args(["-w", "remove", file_stem])
                            .spawn();
                    }
                }
            }
            dialog.close();
            add_walls_to_grid(&image_grid_inner, &notif_box_clone2, &curren_pic_ref_clone2);
        });

        dialog.show();
    });


    stack.add_titled(&wallpaper_box, Some("wallpaper"), "Wallpaper");

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

    monitor_box.append(&back_button);
    monitor_box.append(&scrolled);

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
        if prefer_output != "'prefer-dark'" {
            theme_switch.set_state(true);
        }
    }


    switch_grid.attach(&theme_switch_label, 0, 0, 1, 1);
    switch_grid.attach(&theme_switch, 1, 0, 1, 1);

    // notification sound
    let notiv_sound_label = Label::new(Some("Notifications sound toggle switch"));
    notiv_sound_label.set_halign(gtk4::Align::Start);
    let notiv_sound_switch = gtk4::Switch::builder().build();
    notiv_sound_switch.set_halign(gtk4::Align::Start);
    setup_sound_switch(&notiv_sound_switch);

    let output = Command::new("sh")
        .arg("-c")
        .arg("cynagectl -n")
        .output();
    if let Ok(output) = output {
        let prefer_output = String::from_utf8_lossy(&output.stdout).trim().to_string();
        if prefer_output != "toggle-on" {
            notiv_sound_switch.set_state(true);
        }
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
    network_home.set_hexpand(true);
    network_home.set_vexpand(true);

    let nm_list_scroller = gtk4::ScrolledWindow::builder()
        .min_content_height(150)
        .hscrollbar_policy(gtk4::PolicyType::Never)
        .vscrollbar_policy(gtk4::PolicyType::Automatic)
        .hexpand(true)
        .vexpand(true)
        .build();

    let network_list = GtkBox::new(Orientation::Vertical, 0);
    network_list.set_hexpand(true);
    network_list.set_vexpand(true);
    list_wifi_networks_clickable(&network_list);
    nm_list_scroller.set_child(Some(&network_list));
    nm_list_scroller.set_css_classes(&["display_win"]);

    let nm_toggle = Switch::new();
    nm_toggle.set_halign(gtk4::Align::End);
    nm_toggle.set_hexpand(true);
    nm_toggle.set_active(gett_wifi_status());

    nm_toggle.connect_state_set(move |_, state| {
        sett_wifi_status(state);
        glib::Propagation::Proceed
    });

    let nm_ctrl = GtkBox::new(Orientation::Horizontal, 5);
    nm_ctrl.set_hexpand(true);
    nm_ctrl.set_halign(gtk4::Align::Fill);
    let nm_edit_button = Button::with_label("Edit Saved Connections");
    nm_edit_button.set_halign(gtk4::Align::Start);
    nm_edit_button.set_hexpand(true);
    let page_title_clone_edit = page_title.clone();
    let back_button_clone = back_button.clone();  
    let stack_weak = net_stack.downgrade(); 
    
    nm_edit_button.connect_clicked(move |_| {
        if let Some(stack) = stack_weak.upgrade() {    
            stack.set_visible_child_name("edit");
            typing_effect(&page_title_clone_edit, "network settings >> edit saved connections", 10);
            back_button_clone.set_visible(true);
        }
    });
    

    nm_ctrl.append(&nm_edit_button);
    nm_ctrl.append(&nm_toggle);

    network_home.append(&nm_ctrl);
    network_home.append(&nm_list_scroller);

    // edit page
    let network_edit = GtkBox::new(Orientation::Vertical, 5);
    network_edit.append(&Label::new(Some("edit page")));

    // main stack
    net_stack.add_titled(&network_home, Some("network List"), "Network lsit");
    net_stack.add_titled(&network_edit, Some("edit"), "Edit Saved");

    stack.add_titled(&net_stack, Some("network"), "network settings");

    // window ----------------------------------------------------------------------------------------------------------------------------------------- //
    let back_button_clone = back_button.clone();
    let stack_clone = stack.clone();
    let net_stack_clone = net_stack.clone();
    back_button.connect_clicked(move |_| {
        if let Some(visible_name) = stack_clone.visible_child_name() {
            match visible_name.as_str() {
                "network" => {
                    net_stack_clone.set_visible_child_name("network List");
                    typing_effect(&page_title_clone, "network", 10);
                    back_button_clone.set_visible(false);
                }
                "cynide" => {
                    shell_stack_clone_back.set_visible_child_name("shell_settings");
                    typing_effect(&page_title_clone, "Shell Configs", 10);
                    back_button_clone.set_visible(false);
                }
                _ => {
                    // default fallback, or hide back button
                    back_button_clone.set_visible(false);
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
