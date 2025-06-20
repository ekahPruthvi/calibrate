use gtk4::prelude::*;
use gtk4::subclass::widget;
use gtk4::{
    Application, ApplicationWindow, Box as GtkBox, Button, HeaderBar, Image, Label,
    Orientation, Stack,
};
use gtk4::gdk::Display;
use gtk4::CssProvider;
use std::{cell::RefCell, rc::Rc};
use gtk4::glib;
// use std::env;
// use gtk4::gio::File;

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

fn load_css() {
    // let css = CssProvider::new();
    // let home_dir = env::var("HOME").unwrap();
    // let css_path = format!("{}/.config/capsule/style.css", home_dir);
    // let file = File::for_path(css_path);

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

    "#;

    // css.load_from_file(&file);
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
        // .resizable(false)
        .build();

    let main_box = GtkBox::builder()
        .orientation(Orientation::Vertical)
        .spacing(0)
        .build();

    let header = HeaderBar::builder().build();
    header.set_decoration_layout(Some(""));

    let stack = Stack::builder()
        .transition_type(gtk4::StackTransitionType::SlideLeftRight)
        .build();

    stack.add_titled(&Label::new(Some("Home Page")), Some("home"), "Home");
    stack.add_titled(&Label::new(Some("Wallpaper Page")), Some("wallpaper"), "Wallpaper");
    stack.add_titled(&Label::new(Some("Cynide Page")), Some("cynide"), "Cynide");
    stack.add_titled(&Label::new(Some("Network Page")), Some("network"), "Network");
    stack.add_titled(&Label::new(Some("Bluetooth Page")), Some("bluetooth"), "Bluetooth");
    stack.add_titled(&Label::new(Some("Keybindings Page")), Some("keybindings"), "Keybindings");
    stack.add_titled(&Label::new(Some("Notifications Page")), Some("notifications"), "Notification History");
    stack.add_titled(&Label::new(Some("About Page")), Some("about"), "About System");

    let home_box = gtk4::Grid::builder()
        .row_spacing(2)
        .column_spacing(2)
        .margin_top(20)
        .margin_bottom(20)
        .margin_start(20)
        .margin_end(20)
        .halign(gtk4::Align::Center)
        .build();

    let hello = Label::new(Some(""));
    hello.set_justify(gtk4::Justification::Left);
    hello.set_halign(gtk4::Align::Start);
    hello.set_hexpand(true);
    hello.set_widget_name("hello");
    header.set_title_widget(Some(&hello));

    typing_effect(&hello, "Helllo !!", 200);


    let tabs_box = GtkBox::builder()
        .orientation(Orientation::Horizontal)
        .spacing(5)
        .build();

    let stack_weak = stack.downgrade();
    let header_weak = header.downgrade();
    let tabs_box_clone = tabs_box.clone();

    let add_tab_button = |icon_name: &str, page_id: &str| {
        let button = Button::builder().tooltip_text(page_id).build();
        let image = Image::from_icon_name(icon_name);
        image.set_pixel_size(24);
        button.set_child(Some(&image));

        let page_id_string = page_id.to_lowercase().replace(" ", "");
        let tabs_box_clone = tabs_box_clone.clone();
        let stack_weak = stack_weak.clone();
        let header_weak = header_weak.clone();
        let hollo_clone = hello.clone();

        button.connect_clicked(move |_| {
            if let Some(stack) = stack_weak.upgrade() {
                stack.set_visible_child_name(&page_id_string);
            }

             if let Some(header) = header_weak.upgrade() {
                if page_id_string == "home" {
                   header.set_title_widget(Some(&hollo_clone));
                } else {
                    header.set_title_widget(Some(&tabs_box_clone));
                }
            }
        });

        button
    };

    // Add all buttons initially to home page layout
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

    for (_i, (icon, page)) in buttons.iter().enumerate() {
        let button = add_tab_button(icon, page);
        home_box.attach(&button, col, row, 1, 1);
        button.set_css_classes(&["home_btn"]);

        if let Some(child) = button.child() {
            if let Ok(image) = child.downcast::<Image>() {
                image.set_pixel_size(48);
            }
        }

        col += 1;
        if col >= 4 {
            col = 0;
            row += 1;
        }
    }

    // Also add all buttons to tabs_box (for future header usage)
    tabs_box.append(&add_tab_button("go-home-symbolic", "home"));
    tabs_box.append(&add_tab_button("preferences-desktop-wallpaper-symbolic", "wallpaper"));
    tabs_box.append(&add_tab_button("settings-app-symbolic", "cynide"));
    tabs_box.append(&add_tab_button("network-wired-acquiring-symbolic", "network"));
    tabs_box.append(&add_tab_button("bluetooth-active-symbolic", "bluetooth"));
    tabs_box.append(&add_tab_button("preferences-desktop-keyboard-shortcuts-symbolic", "keybindings"));
    tabs_box.append(&add_tab_button("preferences-system-notifications-symbolic", "notifications"));
    tabs_box.append(&add_tab_button("help-about-symbolic", "about"));

    // Replace home page with buttons
    stack.remove(&stack.child_by_name("home").unwrap());
    stack.add_titled(&home_box, Some("home"), "Home");

    main_box.append(&header);
    main_box.append(&stack);

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
