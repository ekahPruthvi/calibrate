use gtk4::prelude::*;
use gtk4::{
    Application, ApplicationWindow, Box as GtkBox, Button, HeaderBar,
    Label, ListBox, ListBoxRow, Orientation, ScrolledWindow, Stack,
    StackSidebar, Separator, Switch, Scale, SpinButton, Adjustment,
    ComboBoxText, Frame, Grid, Revealer, RevealerTransitionType,
    Align, SelectionMode, CssProvider,
};
use std::fs;

const APP_ID: &str = "ekah.scu.calibrate";

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

fn load_css() {
    let css = CssProvider::new();
    css.load_from_data(
        r#"
        window {
            background-color: #0f1117;
        }
        "#,
    );
    gtk4::style_context_add_provider_for_display(
        &gtk4::gdk::Display::default().expect("Could not connect to display"),
        &css,
        gtk4::STYLE_PROVIDER_PRIORITY_APPLICATION,
    );
}

fn build_ui(app: &Application) {
    load_css();

    let username = read_username();

    let window = ApplicationWindow::builder()
        .application(app)
        .title("Calibrate")
        .default_width(900)
        .default_height(660)
        .build();

    
    window.present();
}

fn main() -> gtk4::glib::ExitCode {
    let app = Application::builder()
        .application_id(APP_ID)
        .build();

    app.connect_activate(|app| {
        build_ui(app);
    });

    app.run()
}