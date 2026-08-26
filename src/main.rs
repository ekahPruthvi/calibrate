use gtk4::prelude::*;
use gtk4::{
    Application, ApplicationWindow, Box as GtkBox, Button, MenuButton, Popover,
    Label, ListBox, ListBoxRow, Orientation, ScrolledWindow, Stack, Overlay, 
    StackSwitcher, Separator, Switch, Scale, SpinButton, Adjustment, GestureClick,
    ComboBoxText, Frame, Grid, Align, CssProvider,
};
use std::fs;

const APP_ID: &str = "ekah.scu.calibrate";

const TAB_IDS: &[&str] = &[
    "home", "display", "sound", "network", "appearance", "about",
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
            background-color: #1a1a1a00;
        }
        
        stacksidebar {
            background-color: #14161f;
            border-right: 1px solid #22242f;
        }

        stacksidebar row {
            padding: 10px 12px;
            border-radius: 8px;
            margin: 2px 6px;
            color: #a6a6b3;
        }

        stacksidebar row:selected {
            background-color: #2a2d3d;
            color: #ffffff;
        }

        stacksidebar row:hover {
            background-color: #1c1e29;
        }

        .settings-page {
            background-color: #1a1a1a7d;
            padding: 24px;
        }

        .page-title {
            font-size: 20px;
            font-weight: 700;
            color: #f2f2f5;
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
            padding: 10px 20px 10px 10px;
            border-radius: 30px;
            border: 2px solid transparent;
            background-image: linear-gradient(rgb(6, 6, 6), rgb(6, 6, 6)),
                                linear-gradient(0deg, rgb(9, 9, 9), rgba(61, 61, 61, 0.686));
            background-origin: border-box;
            background-clip: padding-box, border-box;
            box-shadow: rgba(0, 0, 0, 0.24) 0px 3px 8px;

            transition: all 200ms cubic-bezier(0.4, 0, 0.2, 1);
        }
        
        .shortcut-card:active {
            border: none;
        }
        
        .shortcut-card:hover {
            background-color: #1d2030;
        }

        .shortcut-title {
            font-weight: 600;
            font-size: 14px;
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
            color: #7a7a89;
            font-size: 11px;
        }

        scrollbar {
            background-color: transparent;
        }

        scrollbar slider {
            background-color: #3a3d52;
            border-radius: 8px;
            min-width: 8px;
            min-height: 8px;
            border: 2px solid transparent;
            background-clip: padding-box;
        }

        scrollbar slider:hover {
            background-color: #4d5170;
        }

        scrollbar slider:active {
            background-color: #6c70a0;
        }

        scrollbar trough {
            background-color: #14161f;
            border-radius: 8px;
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
        .vscrollbar_policy(gtk4::PolicyType::Never)
        .vexpand(true)
        .hexpand(true)
        .child(content)
        .build()
}

fn page_header(title: &str, subtitle: &str) -> GtkBox {
    let header = GtkBox::new(Orientation::Vertical, 4);
    let title_lbl = Label::new(Some(title));
    title_lbl.add_css_class("page-subtitle");
    title_lbl.set_halign(Align::Start);

    let subtitle_lbl = Label::new(Some(subtitle));
    subtitle_lbl.add_css_class("page-title");
    subtitle_lbl.set_halign(Align::Start);

    header.append(&title_lbl);
    header.append(&subtitle_lbl);
    header
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


struct Shortcut {
    id: &'static str,
    title: &'static str,
    desc: &'static str,
}

const SHORTCUTS: &[Shortcut] = &[
    Shortcut { id: "display", title: "Display", desc: "Resolution, brightness, night light" },
    Shortcut { id: "sound", title: "Sound", desc: "Volume, output device, alerts" },
    Shortcut { id: "network", title: "Network", desc: "Wi-Fi, VPN, proxy settings" },
    Shortcut { id: "appearance", title: "Appearance", desc: "Theme, accent color, fonts" },
    Shortcut { id: "about", title: "About", desc: "System info and version" },
];

fn build_home_page(stack: &Stack, username: &str) -> ScrolledWindow {
    let content = GtkBox::new(Orientation::Vertical, 20);
    content.append(&page_header(
        "Hello,",
        &format!("{}", username),
    ));

    let grid = Grid::builder()
        .row_spacing(14)
        .column_spacing(14)
        .column_homogeneous(true)
        .build();

    for (i, sc) in SHORTCUTS.iter().enumerate() {
        let card = GtkBox::new(Orientation::Vertical, 6);
        card.add_css_class("shortcut-card");
        card.set_size_request(-1, 96);

        let title = Label::new(Some(sc.title));
        title.add_css_class("shortcut-title");
        title.set_halign(Align::Start);

        let desc = Label::new(Some(sc.desc));
        desc.add_css_class("shortcut-desc");
        desc.set_halign(Align::Start);
        desc.set_wrap(true);

        card.append(&title);
        card.append(&desc);

        let button = Button::new();
        button.set_child(Some(&card));
        button.add_css_class("flat");

        let stack_clone = stack.clone();
        let target_id = sc.id;
        button.connect_clicked(move |_| {
            stack_clone.set_visible_child_name(target_id);
        });

        let col = (i % 3) as i32;
        let row = (i / 3) as i32;
        grid.attach(&button, col, row, 1, 1);
    }

    content.append(&grid);
    page_scroller(&content)
}

fn build_display_page() -> ScrolledWindow {
    let content = GtkBox::new(Orientation::Vertical, 16);
    content.append(&page_header("Display", "Configure how your screen looks and behaves."));

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
    content.append(&page_header("Sound", "Control audio input, output, and alerts."));

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
    content.append(&page_header("Network", "Manage Wi-Fi, VPN, and connection settings."));

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
    content.append(&page_header("Appearance", "Personalize the look and feel of your desktop."));

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
    content.append(&page_header("About", "System and version information."));

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

fn build_ui(app: &Application, initial_tab: Option<String>) {
    load_css();

    let username = read_username();

    let window = ApplicationWindow::builder()
        .application(app)
        .title("Calibrate")
        .default_width(1000)
        .default_height(600)
        .build();

    let stack = Stack::new();
    stack.set_transition_type(gtk4::StackTransitionType::SlideLeftRight);
    stack.set_transition_duration(300);

    let home_page = build_home_page(&stack, &username);
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

    // let sidebar = StackSwitcher::new();
    // sidebar.set_stack(Some(&stack));
    // sidebar.set_size_request(180, -1);

    // let sidebar_scroller = ScrolledWindow::builder()
    //     .vscrollbar_policy(gtk4::PolicyType::Never)
    //     .child(&sidebar)
    //     .hexpand(true)
    //     .margin_bottom(10)
    //     .margin_start(30)
    //     .margin_end(30)
    //     .height_request(20)
    //     .width_request(100)
    //     .build();

    // Dropdown list of tabs shown inside the popover.
    let tab_list = ListBox::new();

    let tab_titles: &[(&str, &str)] = &[
        ("home", "Home"),
        ("display", "Display"),
        ("sound", "Sound"),
        ("network", "Network"),
        ("appearance", "Appearance"),
        ("about", "About"),
    ];

    for (_id, title) in tab_titles {
        let row_label = Label::new(Some(title));
        row_label.set_halign(Align::Start);
        row_label.set_margin_top(8);
        row_label.set_margin_bottom(8);
        row_label.set_margin_start(12);
        row_label.set_margin_end(12);

        let row = ListBoxRow::new();
        row.set_child(Some(&row_label));
        tab_list.append(&row);
    }

    let nav = Popover::builder()
        .child(&tab_list)
        .has_arrow(false)
        .build();

    let menu_button = MenuButton::builder()
        .label("Menu")
        .halign(Align::Center)
        .valign(Align::Start)
        .popover(&nav)
        .build();

    tab_list.set_selection_mode(gtk4::SelectionMode::Single);

    let dragmov = GestureClick::new();

    dragmov.connect_pressed(|gesture, _n_press, x, y| {
        if gesture.current_button() != gtk4::gdk::BUTTON_PRIMARY {
            return;
        }

        if let Some(widget) = gesture.widget() {
            if let Some(root) = widget.root() {
                if let Some(native) = root.dynamic_cast_ref::<gtk4::Native>() {
                    if let Some(toplevel) = native.surface().and_then(|s| s.dynamic_cast::<gtk4::gdk::Toplevel>().ok()) {
                        if let Some(device) = gesture.device() {
                            let button = gesture.current_button();

                            toplevel.begin_move(
                                &device,
                                button as i32,
                                x + widget.allocation().x() as f64,
                                y + widget.allocation().y() as f64,
                                gesture.current_event_time(),
                            );
                        }
                    }
                }
            }
        }
    });

    {
        let stack = stack.clone();
        let nav = nav.clone();
        let tab_ids: Vec<&'static str> = tab_titles.iter().map(|(id, _)| *id).collect();
        tab_list.connect_row_activated(move |_, row| {
            let index = row.index();
            if index >= 0 {
                if let Some(id) = tab_ids.get(index as usize) {
                    stack.set_visible_child_name(id);
                }
            }
            nav.popdown();
        });
    }

    let main_box = Overlay::new();

    main_box.add_controller(dragmov);
    
    main_box.add_overlay(&menu_button);
    main_box.set_child(Some(&stack));
    // main_box.append(&sidebar_scroller);
    

    window.set_child(Some(&main_box));

    let target = initial_tab.unwrap_or_else(|| "home".to_string());
    stack.set_visible_child_name(&target);

    window.present();
}

fn main() -> gtk4::glib::ExitCode {
    let initial_tab = parse_tab_arg();

    let app = Application::builder()
        .application_id(APP_ID)
        .build();

    app.connect_activate(move |app| {
        build_ui(app, initial_tab.clone());
    });

    app.run_with_args::<&str>(&[])
}