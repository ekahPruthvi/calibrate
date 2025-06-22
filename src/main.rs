use gtk4::{
    prelude::*, Switch, Frame, Application, ApplicationWindow, Box as GtkBox, Button, HeaderBar, Image, Label, Orientation, Stack, GestureDrag, 
    Fixed, MessageDialog , gdk::Display , CssProvider, glib, ResponseType, Widget, DrawingArea
};
use std::{cell::RefCell, fs, path::PathBuf, process::Command, rc::Rc};
use std::collections::HashMap;
use std::env;
use std::io::Write;

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

fn show_notification(notif_area: &GtkBox, text: &str) {
    let notif_box = GtkBox::builder()
        .orientation(Orientation::Horizontal)
        .spacing(10)
        .build();

    let notif_label = Label::new(Some(text));
    let close_btn = Button::builder().label("X").build();

    notif_box.append(&notif_label);
    notif_box.append(&close_btn);
    notif_box.set_widget_name("notif_box");

    notif_area.append(&notif_box);
    notif_box.show();

    let notif_box_clone = notif_box.clone();
    close_btn.connect_clicked(move |_| {
        notif_box_clone.hide();
    });

    let notif_box_clone2 = notif_box.clone();
    glib::timeout_add_local(std::time::Duration::from_secs(2), move || {
        notif_box_clone2.hide();
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

fn load_css() {
    let csss = r#"
        window {
            background-color: rgba(30, 30, 30, 0.58);
            border-radius: 13px;
            box-shadow: 0 10px 30px rgba(0, 0, 0, 0.5);
            border: 1px solid rgba(255, 255, 255, 0.1);
            color: white;
        }

        headerbar {
            all: unset;
            padding: 5px;
            background-color: rgba(34, 34, 34, 0);
            border: none;
            box-shadow: none;
        }

        .label {
            color: white;
            font-size: 16px;
        }

        button {
            all: unset;
            background-color: rgba(255, 255, 255, 0.1);
            border-radius: 10px;
            border: 1px solid rgba(255, 255, 255, 0.2);
            padding: 10px;
        }

        button:hover {
            background-color: rgba(255, 255, 255, 0.2);
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

        box {
            padding: 20px;
        }

        button.home_btn {
            background-color: rgba(0, 0, 0, 0.09);
            border-radius: 5px;
            border: 0.1px solid rgba(255, 255, 255, 0.29);
            color: rgba(255, 255, 255, 0.21);
            padding: 30px;
            box-shadow: inset 0 0 1px 1px rgba(32, 32, 32, 0.76);
        }

        button.home_btn:hover {
            color: rgba(255, 255, 255, 0.53);
            box-shadow: 0 0 20px 1px rgba(0, 0, 0, 0.93);
        }

        #hello {
            font-weight: 700;
            font-size: 80px; 
        }

        .wall_s {
            padding: 10px;
        }

        .wall_s scrollbar {
            background-color: transparent;
            border: none;
            padding: 4px;
        }

        .wall_s scrollbar slider {
            background-color: rgba(255, 255, 255, 0.7);
            border-radius: 10px;
            min-width: 4px;
            min-height: 30px;
            transition: background-color 0.3s ease;
        }

        .wall_s scrollbar slider:hover {
            background-color: rgba(255, 255, 255, 1.0);
        }

        .wall_s scrollbar trough {
            background-color: transparent;
            border-radius: 10px;
        }

        #current_wall {
            background-color: rgba(255, 255, 255, 0.09);
            padding: 5px;
            margin: 0;
        }

        .current_img {
            all: unset;
            padding: 0px;
            margin: 0;
        }

        switch {
            background-color: rgba(255, 255, 255, 0.1);
            border-radius: 20px;
            border: 1px solid rgba(255, 255, 255, 0.4);
            min-width: 60px;
            min-height: 30px;
            padding: 3px;
            transition: background-color 0.3s ease;
        }

        switch:checked {
            background-color: rgba(100, 200, 250, 0.5);
            border: 1px solid rgba(255, 255, 255, 0.6);
        }

        switch slider {
            background-color: white;
            border-radius: 50%;
            min-width: 24px;
            min-height: 24px;
            transition: transform 0.3s ease, background-color 0.3s ease;
        }

        switch:checked slider {
            background-color: #fff;
        }

        .grid-line {
            background-color: rgba(0,0,0,0.2);
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
        .default_width(1000)
        .default_height(600)
        .build();

    let main_box = GtkBox::builder().orientation(Orientation::Vertical).spacing(0).build();

    let notif_area = GtkBox::builder()
        .orientation(Orientation::Vertical)
        .spacing(0)
        .build();

    // header ----------------------------------------------------------------------------------------------------------------------------------------- //
    let header = HeaderBar::builder().build();
    header.set_decoration_layout(Some(""));

    let stack = Stack::builder().transition_type(gtk4::StackTransitionType::SlideLeftRight).build();
    stack.add_titled(&Label::new(Some("Home Page")), Some("home"), "Home");

    // home page --------------------------------------------------------------------------------------------------------------------------------------- //

    let hello = Label::new(Some(""));
    hello.set_justify(gtk4::Justification::Left);
    hello.set_halign(gtk4::Align::Start);
    hello.set_hexpand(true);
    hello.set_widget_name("hello");
    header.set_title_widget(Some(&hello));
    typing_effect(&hello, "Helllo !!", 150);

    let tabs_box = GtkBox::builder().orientation(Orientation::Horizontal).spacing(5).build();
    let stack_weak = stack.downgrade();
    let header_weak = header.downgrade();
    let tabs_box_clone = tabs_box.clone();

    let add_tab_button = |icon_name: &str, page_id: &str| {
        let button = Button::builder().tooltip_text(page_id).build();
        let image = Image::from_icon_name(icon_name);
        image.set_pixel_size(24);
        button.set_child(Some(&image));

        let page_id_string = page_id.to_lowercase();
        let tabs_box_clone = tabs_box_clone.clone();
        let stack_weak = stack_weak.clone();
        let header_weak = header_weak.clone();
        let hello_clone = hello.clone();

        button.connect_clicked(move |_| {
            if let Some(stack) = stack_weak.upgrade() {
                stack.set_visible_child_name(&page_id_string);
            }
            if let Some(header) = header_weak.upgrade() {
                if page_id_string == "home" {
                    header.set_title_widget(Some(&hello_clone));
                } else {
                    header.set_title_widget(Some(&tabs_box_clone));
                }
            }
        });
        button
    };

    let mut row = 0;
    let mut col = 0;
    let buttons = vec![
        ("preferences-desktop-wallpaper-symbolic", "wallpaper"),
        ("settings-app-symbolic", "cynide"),
        ("network-wired-acquiring-symbolic", "network"),
        ("bluetooth-active-symbolic", "bluetooth"),
        ("preferences-desktop-keyboard-shortcuts-symbolic", "keybindings"),
        ("preferences-system-notifications-symbolic", "notifications"),
        ("help-about-symbolic", "about"),
    ];

    let home_box = gtk4::Grid::builder()
        .row_spacing(2)
        .column_spacing(2)
        .margin_top(20)
        .margin_bottom(20)
        .margin_start(20)
        .margin_end(20)
        .halign(gtk4::Align::Center)
        .build();

    for (icon, page) in &buttons {
        let button = add_tab_button(icon, page);
        home_box.attach(&button, col, row, 1, 1);
        button.set_css_classes(&["home_btn"]);
        if let Some(child) = button.child() {
            if let Ok(image) = child.downcast::<Image>() {
                image.set_pixel_size(48);
            }
        }
        col += 1;
        if col >= 4 { col = 0; row += 1; }
    }

    tabs_box.append(&add_tab_button("go-home-symbolic", "home"));
    for (icon, page) in &buttons { tabs_box.append(&add_tab_button(icon, page)); }

    stack.remove(&stack.child_by_name("home").unwrap());
    stack.add_titled(&home_box, Some("home"), "Home");

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
    let resolution_label = gtk4::Label::new(Some(&format!("Resolution: {}", resolution_part)));

    // Load the image from ~/.config/swww/cynage/{image_filename}
    let home_dir = std::env::var("HOME").unwrap();
    let image_path = format!("{}/.config/swww/cynage/{}", home_dir, image_filename);

    let file = gtk4::gio::File::for_path(image_path);
    let texture = gtk4::gdk::Texture::from_file(&file).unwrap();
    let current_pic = gtk4::Picture::for_paintable(&texture);
    let current_pic_ref = Rc::new(RefCell::new(current_pic.clone()));

    let current_wall = GtkBox::new(Orientation::Horizontal, 2);

    let wall_info = GtkBox::new(Orientation::Vertical, 10);
    wall_info.set_hexpand(true);

    wall_info.append(&display_label);
    wall_info.append(&resolution_label);

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
    let home_dir = std::env::var("HOME").unwrap();
    let wallpaper_dir = PathBuf::from(format!("{}/.config/swww/cynage", home_dir));
    if let Ok(entries) = fs::read_dir(wallpaper_dir.clone()) {
        let notiv_clone_outer_for_wall = notif_area.clone();
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
                let current_pic_clone = current_pic_ref.clone();
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
                image_grid.append(&btn);
            }
        }
    }

    stack.add_titled(&wallpaper_box, Some("wallpaper"), "Wallpaper");

    // shell settings ---------------------------------------------------------------------------------------------------------------------------------- //
    
    let shell_settings_box = GtkBox::new(Orientation::Vertical, 2);

    // monitor
    let monitor_box = GtkBox::new(Orientation::Vertical, 10);
    let fixed = Fixed::new();
    fixed.set_size_request(4000, 3000); // Large canvas for multiple monitors

    let scrolled = gtk4::ScrolledWindow::new();
    scrolled.set_policy(gtk4::PolicyType::Automatic, gtk4::PolicyType::Automatic);
    scrolled.set_size_request(800, 400);

    scrolled.set_child(Some(&fixed));

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
    let switch_box = gtk4::Grid::builder()
        .column_homogeneous(true)
        .row_homogeneous(true)
        .column_spacing(10)
        .row_spacing(10)
        .build();

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


    switch_box.attach(&theme_switch_label, 0, 0, 1, 1);
    switch_box.attach(&theme_switch, 1, 0, 1, 1);

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
    
    switch_box.attach(&notiv_sound_label, 0, 1, 1, 1);
    switch_box.attach(&notiv_sound_switch, 1, 1, 1, 1);

    shell_settings_box.append(&monitor_box);
    shell_settings_box.append(&switch_box);
    stack.add_titled(&shell_settings_box, Some("cynide"), "Cynide Settings");


    // window ----------------------------------------------------------------------------------------------------------------------------------------- //

    main_box.append(&notif_area);
    main_box.append(&header);
    main_box.append(&stack);
    window.set_child(Some(&main_box));
    window.present();

}

fn main() {
    let app = Application::builder().application_id("ekah.scu.calibrate").build();
    app.connect_activate(|app| {
        load_css();
        build_ui(app);
    });
    app.run();
}
