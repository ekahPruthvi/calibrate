use gtk4::prelude::*;
use gtk4::{
    Application, ApplicationWindow, Box as GtkBox, Button, HeaderBar,
    Label, ListBox, ListBoxRow, Orientation, ScrolledWindow, Stack,
    StackSidebar, Separator, Switch, Scale, SpinButton, Adjustment,
    ComboBoxText, Frame, Grid, Revealer, RevealerTransitionType,
    Align, SelectionMode, CssProvider,
};
use gtk4::glib::clone;
use std::fs;

const APP_ID: &str = "com.calibrate.app";

fn read_username() -> String {
    let path = "/usr/share/octobacillus/user.octo";
    if let Ok(contents) = fs::read_to_string(path) {
        for line in contents.lines() {
            let line = line.trim();
            if line.starts_with("name") {
                // Handle "name = ekah" or "name=ekah"
                if let Some(val) = line.splitn(2, '=').nth(1) {
                    return val.trim().to_string();
                }
            }
        }
    }
    // Fallback if file not found or key missing
    "ekah".to_string()
}

fn load_css() {
    let css = CssProvider::new();
    css.load_from_data(
        r#"
        window {
            background-color: #0f1117;
        }
        .sidebar {
            background-color: #161b22;
            border-right: 1px solid #30363d;
            min-width: 200px;
        }
        .sidebar row {
            padding: 10px 16px;
            border-radius: 6px;
            margin: 2px 6px;
            color: #c9d1d9;
            font-size: 14px;
        }
        .sidebar row:selected {
            background-color: #21262d;
            color: #58a6ff;
        }
        .sidebar row:hover:not(:selected) {
            background-color: #1c2128;
        }
        .header-bar {
            background-color: #161b22;
            border-bottom: 1px solid #30363d;
            color: #c9d1d9;
        }
        .page-title {
            font-size: 22px;
            font-weight: bold;
            color: #e6edf3;
            margin-bottom: 4px;
        }
        .page-subtitle {
            font-size: 13px;
            color: #8b949e;
            margin-bottom: 24px;
        }
        .hello-card {
            background: linear-gradient(135deg, #1f2937 0%, #111827 100%);
            border: 1px solid #30363d;
            border-radius: 12px;
            padding: 32px;
            margin: 0 0 20px 0;
        }
        .hello-greeting {
            font-size: 32px;
            font-weight: 800;
            color: #e6edf3;
        }
        .hello-username {
            color: #58a6ff;
        }
        .hello-tagline {
            font-size: 14px;
            color: #8b949e;
            margin-top: 6px;
        }
        .stat-card {
            background-color: #161b22;
            border: 1px solid #30363d;
            border-radius: 8px;
            padding: 16px 20px;
        }
        .stat-label {
            font-size: 12px;
            color: #8b949e;
            text-transform: uppercase;
            letter-spacing: 0.5px;
        }
        .stat-value {
            font-size: 20px;
            font-weight: 700;
            color: #58a6ff;
            margin-top: 4px;
        }
        .section-frame {
            background-color: #161b22;
            border: 1px solid #30363d;
            border-radius: 8px;
            margin-bottom: 16px;
        }
        .section-frame > label {
            color: #8b949e;
            font-size: 12px;
            font-weight: 600;
            text-transform: uppercase;
            letter-spacing: 0.5px;
        }
        .settings-row {
            padding: 12px 16px;
            border-bottom: 1px solid #21262d;
        }
        .settings-row:last-child {
            border-bottom: none;
        }
        .settings-label {
            font-size: 14px;
            color: #c9d1d9;
            font-weight: 500;
        }
        .settings-sublabel {
            font-size: 12px;
            color: #6e7681;
            margin-top: 2px;
        }
        .nav-label {
            font-size: 13px;
            font-weight: 500;
        }
        .nav-icon {
            font-size: 16px;
            margin-right: 8px;
        }
        separator {
            background-color: #30363d;
            margin: 4px 0;
        }
        switch:checked {
            background-color: #238636;
        }
        "#,
    );
    gtk4::style_context_add_provider_for_display(
        &gtk4::gdk::Display::default().expect("Could not connect to display"),
        &css,
        gtk4::STYLE_PROVIDER_PRIORITY_APPLICATION,
    );
}

fn build_home_page(username: &str) -> GtkBox {
    let vbox = GtkBox::new(Orientation::Vertical, 0);
    vbox.set_margin_start(32);
    vbox.set_margin_end(32);
    vbox.set_margin_top(32);
    vbox.set_margin_bottom(32);

    // Hello card
    let hello_card = GtkBox::new(Orientation::Vertical, 0);
    hello_card.add_css_class("hello-card");

    let greeting_box = GtkBox::new(Orientation::Horizontal, 0);
    let greet_label = Label::new(Some("Hello, "));
    greet_label.add_css_class("hello-greeting");
    greet_label.set_xalign(0.0);

    let user_label = Label::new(Some(username));
    user_label.add_css_class("hello-greeting");
    user_label.add_css_class("hello-username");
    user_label.set_xalign(0.0);

    let excl_label = Label::new(Some(" 👋"));
    excl_label.add_css_class("hello-greeting");

    greeting_box.append(&greet_label);
    greeting_box.append(&user_label);
    greeting_box.append(&excl_label);

    let tagline = Label::new(Some("Welcome to Calibrate — your system control centre."));
    tagline.add_css_class("hello-tagline");
    tagline.set_xalign(0.0);

    hello_card.append(&greeting_box);
    hello_card.append(&tagline);

    // Stats row
    let stats_box = GtkBox::new(Orientation::Horizontal, 12);
    stats_box.set_homogeneous(true);

    let stats = [
        ("Shell", "bash", "🐚"),
        ("Network", "Connected", "🌐"),
        ("Audio", "100%", "🔊"),
    ];

    for (label, value, icon) in &stats {
        let card = GtkBox::new(Orientation::Vertical, 4);
        card.add_css_class("stat-card");

        let icon_lbl = Label::new(Some(&format!("{} {}", icon, label)));
        icon_lbl.add_css_class("stat-label");
        icon_lbl.set_xalign(0.0);

        let val_lbl = Label::new(Some(value));
        val_lbl.add_css_class("stat-value");
        val_lbl.set_xalign(0.0);

        card.append(&icon_lbl);
        card.append(&val_lbl);
        stats_box.append(&card);
    }

    vbox.append(&hello_card);
    vbox.append(&stats_box);

    vbox
}

fn build_shell_page() -> GtkBox {
    let vbox = GtkBox::new(Orientation::Vertical, 0);
    vbox.set_margin_start(32);
    vbox.set_margin_end(32);
    vbox.set_margin_top(32);
    vbox.set_margin_bottom(32);

    let title = Label::new(Some("Shell Settings"));
    title.add_css_class("page-title");
    title.set_xalign(0.0);

    let subtitle = Label::new(Some("Configure your shell environment and terminal behaviour."));
    subtitle.add_css_class("page-subtitle");
    subtitle.set_xalign(0.0);

    vbox.append(&title);
    vbox.append(&subtitle);

    // Default shell section
    let shell_frame = Frame::new(Some("Default Shell"));
    shell_frame.add_css_class("section-frame");
    let shell_box = GtkBox::new(Orientation::Vertical, 0);

    let shell_row = GtkBox::new(Orientation::Horizontal, 12);
    shell_row.add_css_class("settings-row");
    shell_row.set_hexpand(true);

    let shell_labels = GtkBox::new(Orientation::Vertical, 2);
    let shell_lbl = Label::new(Some("Default Shell"));
    shell_lbl.add_css_class("settings-label");
    shell_lbl.set_xalign(0.0);
    let shell_sub = Label::new(Some("Select the shell used for new terminal sessions"));
    shell_sub.add_css_class("settings-sublabel");
    shell_sub.set_xalign(0.0);
    shell_labels.append(&shell_lbl);
    shell_labels.append(&shell_sub);
    shell_labels.set_hexpand(true);

    let shell_combo = ComboBoxText::new();
    shell_combo.append_text("/bin/bash");
    shell_combo.append_text("/bin/zsh");
    shell_combo.append_text("/bin/fish");
    shell_combo.append_text("/bin/sh");
    shell_combo.set_active(Some(0));

    shell_row.append(&shell_labels);
    shell_row.append(&shell_combo);
    shell_box.append(&shell_row);

    // Login shell row
    let login_row = GtkBox::new(Orientation::Horizontal, 12);
    login_row.add_css_class("settings-row");
    login_row.set_hexpand(true);

    let login_labels = GtkBox::new(Orientation::Vertical, 2);
    let login_lbl = Label::new(Some("Use as Login Shell"));
    login_lbl.add_css_class("settings-label");
    login_lbl.set_xalign(0.0);
    let login_sub = Label::new(Some("Start shell as a login shell by default"));
    login_sub.add_css_class("settings-sublabel");
    login_sub.set_xalign(0.0);
    login_labels.append(&login_lbl);
    login_labels.append(&login_sub);
    login_labels.set_hexpand(true);

    let login_switch = Switch::new();
    login_switch.set_active(true);
    login_switch.set_valign(Align::Center);

    login_row.append(&login_labels);
    login_row.append(&login_switch);
    shell_box.append(&login_row);

    shell_frame.set_child(Some(&shell_box));
    vbox.append(&shell_frame);

    // History section
    let hist_frame = Frame::new(Some("History"));
    hist_frame.add_css_class("section-frame");
    let hist_box = GtkBox::new(Orientation::Vertical, 0);

    let histsize_row = GtkBox::new(Orientation::Horizontal, 12);
    histsize_row.add_css_class("settings-row");

    let hs_labels = GtkBox::new(Orientation::Vertical, 2);
    let hs_lbl = Label::new(Some("History Size"));
    hs_lbl.add_css_class("settings-label");
    hs_lbl.set_xalign(0.0);
    let hs_sub = Label::new(Some("Number of commands to keep in history"));
    hs_sub.add_css_class("settings-sublabel");
    hs_sub.set_xalign(0.0);
    hs_labels.append(&hs_lbl);
    hs_labels.append(&hs_sub);
    hs_labels.set_hexpand(true);

    let adj = Adjustment::new(1000.0, 100.0, 100000.0, 100.0, 1000.0, 0.0);
    let spin = SpinButton::new(Some(&adj), 1.0, 0);
    spin.set_valign(Align::Center);

    histsize_row.append(&hs_labels);
    histsize_row.append(&spin);
    hist_box.append(&histsize_row);

    let histdup_row = GtkBox::new(Orientation::Horizontal, 12);
    histdup_row.add_css_class("settings-row");

    let hd_labels = GtkBox::new(Orientation::Vertical, 2);
    let hd_lbl = Label::new(Some("Ignore Duplicates"));
    hd_lbl.add_css_class("settings-label");
    hd_lbl.set_xalign(0.0);
    let hd_sub = Label::new(Some("Skip saving duplicate consecutive commands"));
    hd_sub.add_css_class("settings-sublabel");
    hd_sub.set_xalign(0.0);
    hd_labels.append(&hd_lbl);
    hd_labels.append(&hd_sub);
    hd_labels.set_hexpand(true);

    let dup_switch = Switch::new();
    dup_switch.set_active(true);
    dup_switch.set_valign(Align::Center);

    histdup_row.append(&hd_labels);
    histdup_row.append(&dup_switch);
    hist_box.append(&histdup_row);

    hist_frame.set_child(Some(&hist_box));
    vbox.append(&hist_frame);

    // Environment section
    let env_frame = Frame::new(Some("Environment"));
    env_frame.add_css_class("section-frame");
    let env_box = GtkBox::new(Orientation::Vertical, 0);

    let env_row = GtkBox::new(Orientation::Horizontal, 12);
    env_row.add_css_class("settings-row");

    let env_labels = GtkBox::new(Orientation::Vertical, 2);
    let env_lbl = Label::new(Some("Source ~/.profile on Login"));
    env_lbl.add_css_class("settings-label");
    env_lbl.set_xalign(0.0);
    let env_sub = Label::new(Some("Load profile environment variables at login"));
    env_sub.add_css_class("settings-sublabel");
    env_sub.set_xalign(0.0);
    env_labels.append(&env_lbl);
    env_labels.append(&env_sub);
    env_labels.set_hexpand(true);

    let env_switch = Switch::new();
    env_switch.set_active(true);
    env_switch.set_valign(Align::Center);

    env_row.append(&env_labels);
    env_row.append(&env_switch);
    env_box.append(&env_row);

    env_frame.set_child(Some(&env_box));
    vbox.append(&env_frame);

    vbox
}

fn build_network_page() -> GtkBox {
    let vbox = GtkBox::new(Orientation::Vertical, 0);
    vbox.set_margin_start(32);
    vbox.set_margin_end(32);
    vbox.set_margin_top(32);
    vbox.set_margin_bottom(32);

    let title = Label::new(Some("Network Settings"));
    title.add_css_class("page-title");
    title.set_xalign(0.0);

    let subtitle = Label::new(Some("Manage connections, DNS, proxy and firewall settings."));
    subtitle.add_css_class("page-subtitle");
    subtitle.set_xalign(0.0);

    vbox.append(&title);
    vbox.append(&subtitle);

    // Connection section
    let conn_frame = Frame::new(Some("Connection"));
    conn_frame.add_css_class("section-frame");
    let conn_box = GtkBox::new(Orientation::Vertical, 0);

    let wifi_row = GtkBox::new(Orientation::Horizontal, 12);
    wifi_row.add_css_class("settings-row");

    let wifi_labels = GtkBox::new(Orientation::Vertical, 2);
    let wifi_lbl = Label::new(Some("Wi-Fi"));
    wifi_lbl.add_css_class("settings-label");
    wifi_lbl.set_xalign(0.0);
    let wifi_sub = Label::new(Some("Connected to HomeNetwork-5G"));
    wifi_sub.add_css_class("settings-sublabel");
    wifi_sub.set_xalign(0.0);
    wifi_labels.append(&wifi_lbl);
    wifi_labels.append(&wifi_sub);
    wifi_labels.set_hexpand(true);

    let wifi_switch = Switch::new();
    wifi_switch.set_active(true);
    wifi_switch.set_valign(Align::Center);

    wifi_row.append(&wifi_labels);
    wifi_row.append(&wifi_switch);
    conn_box.append(&wifi_row);

    let eth_row = GtkBox::new(Orientation::Horizontal, 12);
    eth_row.add_css_class("settings-row");

    let eth_labels = GtkBox::new(Orientation::Vertical, 2);
    let eth_lbl = Label::new(Some("Ethernet"));
    eth_lbl.add_css_class("settings-label");
    eth_lbl.set_xalign(0.0);
    let eth_sub = Label::new(Some("eth0 — Not connected"));
    eth_sub.add_css_class("settings-sublabel");
    eth_sub.set_xalign(0.0);
    eth_labels.append(&eth_lbl);
    eth_labels.append(&eth_sub);
    eth_labels.set_hexpand(true);

    let eth_switch = Switch::new();
    eth_switch.set_active(false);
    eth_switch.set_valign(Align::Center);

    eth_row.append(&eth_labels);
    eth_row.append(&eth_switch);
    conn_box.append(&eth_row);

    let bt_row = GtkBox::new(Orientation::Horizontal, 12);
    bt_row.add_css_class("settings-row");

    let bt_labels = GtkBox::new(Orientation::Vertical, 2);
    let bt_lbl = Label::new(Some("Bluetooth"));
    bt_lbl.add_css_class("settings-label");
    bt_lbl.set_xalign(0.0);
    let bt_sub = Label::new(Some("2 devices paired"));
    bt_sub.add_css_class("settings-sublabel");
    bt_sub.set_xalign(0.0);
    bt_labels.append(&bt_lbl);
    bt_labels.append(&bt_sub);
    bt_labels.set_hexpand(true);

    let bt_switch = Switch::new();
    bt_switch.set_active(true);
    bt_switch.set_valign(Align::Center);

    bt_row.append(&bt_labels);
    bt_row.append(&bt_switch);
    conn_box.append(&bt_row);

    conn_frame.set_child(Some(&conn_box));
    vbox.append(&conn_frame);

    // DNS section
    let dns_frame = Frame::new(Some("DNS"));
    dns_frame.add_css_class("section-frame");
    let dns_box = GtkBox::new(Orientation::Vertical, 0);

    let dns_row = GtkBox::new(Orientation::Horizontal, 12);
    dns_row.add_css_class("settings-row");

    let dns_labels = GtkBox::new(Orientation::Vertical, 2);
    let dns_lbl = Label::new(Some("DNS Provider"));
    dns_lbl.add_css_class("settings-label");
    dns_lbl.set_xalign(0.0);
    let dns_sub = Label::new(Some("Choose your preferred DNS resolver"));
    dns_sub.add_css_class("settings-sublabel");
    dns_sub.set_xalign(0.0);
    dns_labels.append(&dns_lbl);
    dns_labels.append(&dns_sub);
    dns_labels.set_hexpand(true);

    let dns_combo = ComboBoxText::new();
    dns_combo.append_text("Automatic (DHCP)");
    dns_combo.append_text("Cloudflare (1.1.1.1)");
    dns_combo.append_text("Google (8.8.8.8)");
    dns_combo.append_text("Quad9 (9.9.9.9)");
    dns_combo.set_active(Some(1));

    dns_row.append(&dns_labels);
    dns_row.append(&dns_combo);
    dns_box.append(&dns_row);

    let dnssec_row = GtkBox::new(Orientation::Horizontal, 12);
    dnssec_row.add_css_class("settings-row");

    let ds_labels = GtkBox::new(Orientation::Vertical, 2);
    let ds_lbl = Label::new(Some("DNSSEC Validation"));
    ds_lbl.add_css_class("settings-label");
    ds_lbl.set_xalign(0.0);
    let ds_sub = Label::new(Some("Validate DNS responses with DNSSEC"));
    ds_sub.add_css_class("settings-sublabel");
    ds_sub.set_xalign(0.0);
    ds_labels.append(&ds_lbl);
    ds_labels.append(&ds_sub);
    ds_labels.set_hexpand(true);

    let ds_switch = Switch::new();
    ds_switch.set_active(true);
    ds_switch.set_valign(Align::Center);

    dnssec_row.append(&ds_labels);
    dnssec_row.append(&ds_switch);
    dns_box.append(&dnssec_row);

    dns_frame.set_child(Some(&dns_box));
    vbox.append(&dns_frame);

    // Proxy section
    let proxy_frame = Frame::new(Some("Proxy"));
    proxy_frame.add_css_class("section-frame");
    let proxy_box = GtkBox::new(Orientation::Vertical, 0);

    let proxy_row = GtkBox::new(Orientation::Horizontal, 12);
    proxy_row.add_css_class("settings-row");

    let proxy_labels = GtkBox::new(Orientation::Vertical, 2);
    let proxy_lbl = Label::new(Some("Use System Proxy"));
    proxy_lbl.add_css_class("settings-label");
    proxy_lbl.set_xalign(0.0);
    let proxy_sub = Label::new(Some("Apply proxy settings system-wide"));
    proxy_sub.add_css_class("settings-sublabel");
    proxy_sub.set_xalign(0.0);
    proxy_labels.append(&proxy_lbl);
    proxy_labels.append(&proxy_sub);
    proxy_labels.set_hexpand(true);

    let proxy_switch = Switch::new();
    proxy_switch.set_active(false);
    proxy_switch.set_valign(Align::Center);

    proxy_row.append(&proxy_labels);
    proxy_row.append(&proxy_switch);
    proxy_box.append(&proxy_row);

    proxy_frame.set_child(Some(&proxy_box));
    vbox.append(&proxy_frame);

    vbox
}

fn build_audio_page() -> GtkBox {
    let vbox = GtkBox::new(Orientation::Vertical, 0);
    vbox.set_margin_start(32);
    vbox.set_margin_end(32);
    vbox.set_margin_top(32);
    vbox.set_margin_bottom(32);

    let title = Label::new(Some("Audio Settings"));
    title.add_css_class("page-title");
    title.set_xalign(0.0);

    let subtitle = Label::new(Some("Control output, input levels, and sound preferences."));
    subtitle.add_css_class("page-subtitle");
    subtitle.set_xalign(0.0);

    vbox.append(&title);
    vbox.append(&subtitle);

    // Output section
    let out_frame = Frame::new(Some("Output"));
    out_frame.add_css_class("section-frame");
    let out_box = GtkBox::new(Orientation::Vertical, 0);

    let dev_row = GtkBox::new(Orientation::Horizontal, 12);
    dev_row.add_css_class("settings-row");

    let dev_labels = GtkBox::new(Orientation::Vertical, 2);
    let dev_lbl = Label::new(Some("Output Device"));
    dev_lbl.add_css_class("settings-label");
    dev_lbl.set_xalign(0.0);
    let dev_sub = Label::new(Some("Select default audio output device"));
    dev_sub.add_css_class("settings-sublabel");
    dev_sub.set_xalign(0.0);
    dev_labels.append(&dev_lbl);
    dev_labels.append(&dev_sub);
    dev_labels.set_hexpand(true);

    let dev_combo = ComboBoxText::new();
    dev_combo.append_text("Built-in Speakers");
    dev_combo.append_text("HDMI Output");
    dev_combo.append_text("USB Headset");
    dev_combo.append_text("Bluetooth Audio");
    dev_combo.set_active(Some(0));

    dev_row.append(&dev_labels);
    dev_row.append(&dev_combo);
    out_box.append(&dev_row);

    let vol_row = GtkBox::new(Orientation::Horizontal, 12);
    vol_row.add_css_class("settings-row");

    let vol_labels = GtkBox::new(Orientation::Vertical, 2);
    let vol_lbl = Label::new(Some("Volume"));
    vol_lbl.add_css_class("settings-label");
    vol_lbl.set_xalign(0.0);
    let vol_sub = Label::new(Some("Master output volume level"));
    vol_sub.add_css_class("settings-sublabel");
    vol_sub.set_xalign(0.0);
    vol_labels.append(&vol_lbl);
    vol_labels.append(&vol_sub);

    let vol_adj = Adjustment::new(80.0, 0.0, 100.0, 1.0, 10.0, 0.0);
    let vol_scale = Scale::new(Orientation::Horizontal, Some(&vol_adj));
    vol_scale.set_hexpand(true);
    vol_scale.set_draw_value(true);
    vol_scale.set_value_pos(gtk4::PositionType::Right);

    vol_row.append(&vol_labels);
    vol_row.append(&vol_scale);
    out_box.append(&vol_row);

    let mute_row = GtkBox::new(Orientation::Horizontal, 12);
    mute_row.add_css_class("settings-row");

    let mute_labels = GtkBox::new(Orientation::Vertical, 2);
    let mute_lbl = Label::new(Some("Mute Output"));
    mute_lbl.add_css_class("settings-label");
    mute_lbl.set_xalign(0.0);
    let mute_sub = Label::new(Some("Silence all audio output"));
    mute_sub.add_css_class("settings-sublabel");
    mute_sub.set_xalign(0.0);
    mute_labels.append(&mute_lbl);
    mute_labels.append(&mute_sub);
    mute_labels.set_hexpand(true);

    let mute_switch = Switch::new();
    mute_switch.set_active(false);
    mute_switch.set_valign(Align::Center);

    mute_row.append(&mute_labels);
    mute_row.append(&mute_switch);
    out_box.append(&mute_row);

    out_frame.set_child(Some(&out_box));
    vbox.append(&out_frame);

    // Input section
    let inp_frame = Frame::new(Some("Input"));
    inp_frame.add_css_class("section-frame");
    let inp_box = GtkBox::new(Orientation::Vertical, 0);

    let mic_row = GtkBox::new(Orientation::Horizontal, 12);
    mic_row.add_css_class("settings-row");

    let mic_labels = GtkBox::new(Orientation::Vertical, 2);
    let mic_lbl = Label::new(Some("Microphone"));
    mic_lbl.add_css_class("settings-label");
    mic_lbl.set_xalign(0.0);
    let mic_sub = Label::new(Some("Select default input device"));
    mic_sub.add_css_class("settings-sublabel");
    mic_sub.set_xalign(0.0);
    mic_labels.append(&mic_lbl);
    mic_labels.append(&mic_sub);
    mic_labels.set_hexpand(true);

    let mic_combo = ComboBoxText::new();
    mic_combo.append_text("Built-in Microphone");
    mic_combo.append_text("USB Microphone");
    mic_combo.append_text("Headset Mic");
    mic_combo.set_active(Some(0));

    mic_row.append(&mic_labels);
    mic_row.append(&mic_combo);
    inp_box.append(&mic_row);

    let gain_row = GtkBox::new(Orientation::Horizontal, 12);
    gain_row.add_css_class("settings-row");

    let gain_labels = GtkBox::new(Orientation::Vertical, 2);
    let gain_lbl = Label::new(Some("Input Gain"));
    gain_lbl.add_css_class("settings-label");
    gain_lbl.set_xalign(0.0);
    let gain_sub = Label::new(Some("Microphone sensitivity / boost level"));
    gain_sub.add_css_class("settings-sublabel");
    gain_sub.set_xalign(0.0);
    gain_labels.append(&gain_lbl);
    gain_labels.append(&gain_sub);

    let gain_adj = Adjustment::new(70.0, 0.0, 100.0, 1.0, 10.0, 0.0);
    let gain_scale = Scale::new(Orientation::Horizontal, Some(&gain_adj));
    gain_scale.set_hexpand(true);
    gain_scale.set_draw_value(true);
    gain_scale.set_value_pos(gtk4::PositionType::Right);

    gain_row.append(&gain_labels);
    gain_row.append(&gain_scale);
    inp_box.append(&gain_row);

    inp_frame.set_child(Some(&inp_box));
    vbox.append(&inp_frame);

    // Advanced section
    let adv_frame = Frame::new(Some("Advanced"));
    adv_frame.add_css_class("section-frame");
    let adv_box = GtkBox::new(Orientation::Vertical, 0);

    let noise_row = GtkBox::new(Orientation::Horizontal, 12);
    noise_row.add_css_class("settings-row");

    let noise_labels = GtkBox::new(Orientation::Vertical, 2);
    let noise_lbl = Label::new(Some("Noise Suppression"));
    noise_lbl.add_css_class("settings-label");
    noise_lbl.set_xalign(0.0);
    let noise_sub = Label::new(Some("Reduce background noise from microphone input"));
    noise_sub.add_css_class("settings-sublabel");
    noise_sub.set_xalign(0.0);
    noise_labels.append(&noise_lbl);
    noise_labels.append(&noise_sub);
    noise_labels.set_hexpand(true);

    let noise_switch = Switch::new();
    noise_switch.set_active(true);
    noise_switch.set_valign(Align::Center);

    noise_row.append(&noise_labels);
    noise_row.append(&noise_switch);
    adv_box.append(&noise_row);

    let norm_row = GtkBox::new(Orientation::Horizontal, 12);
    norm_row.add_css_class("settings-row");

    let norm_labels = GtkBox::new(Orientation::Vertical, 2);
    let norm_lbl = Label::new(Some("Volume Normalisation"));
    norm_lbl.add_css_class("settings-label");
    norm_lbl.set_xalign(0.0);
    let norm_sub = Label::new(Some("Equalise loudness across different applications"));
    norm_sub.add_css_class("settings-sublabel");
    norm_sub.set_xalign(0.0);
    norm_labels.append(&norm_lbl);
    norm_labels.append(&norm_sub);
    norm_labels.set_hexpand(true);

    let norm_switch = Switch::new();
    norm_switch.set_active(false);
    norm_switch.set_valign(Align::Center);

    norm_row.append(&norm_labels);
    norm_row.append(&norm_switch);
    adv_box.append(&norm_row);

    adv_frame.set_child(Some(&adv_box));
    vbox.append(&adv_frame);

    vbox
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

    // Header bar
    let header = HeaderBar::new();
    header.add_css_class("header-bar");
    let title_label = Label::new(Some("Calibrate"));
    title_label.set_markup("<b>Calibrate</b>");
    header.set_title_widget(Some(&title_label));
    window.set_titlebar(Some(&header));

    // Main layout: sidebar + content
    let main_box = GtkBox::new(Orientation::Horizontal, 0);

    // Stack for pages
    let stack = Stack::new();
    stack.set_transition_type(gtk4::StackTransitionType::SlideLeftRight);
    stack.set_transition_duration(200);
    stack.set_hexpand(true);
    stack.set_vexpand(true);

    // Wrap pages in ScrolledWindow
    let make_scroll = |child: GtkBox| -> ScrolledWindow {
        let sw = ScrolledWindow::new();
        sw.set_hexpand(true);
        sw.set_vexpand(true);
        sw.set_policy(gtk4::PolicyType::Never, gtk4::PolicyType::Automatic);
        sw.set_child(Some(&child));
        sw
    };

    let home_page = build_home_page(&username);
    let shell_page = build_shell_page();
    let network_page = build_network_page();
    let audio_page = build_audio_page();

    stack.add_named(&make_scroll(home_page), Some("home"));
    stack.add_named(&make_scroll(shell_page), Some("shell"));
    stack.add_named(&make_scroll(network_page), Some("network"));
    stack.add_named(&make_scroll(audio_page), Some("audio"));

    // Sidebar nav
    let sidebar_box = GtkBox::new(Orientation::Vertical, 0);
    sidebar_box.add_css_class("sidebar");
    sidebar_box.set_vexpand(true);

    let nav_list = ListBox::new();
    nav_list.set_selection_mode(SelectionMode::Single);
    nav_list.set_vexpand(true);
    nav_list.add_css_class("sidebar");

    let pages: &[(&str, &str, &str)] = &[
        ("home",    "🏠", "Home"),
        ("shell",   "🐚", "Shell"),
        ("network", "🌐", "Network"),
        ("audio",   "🔊", "Audio"),
    ];

    for (name, icon, label) in pages {
        let row = ListBoxRow::new();
        let hbox = GtkBox::new(Orientation::Horizontal, 8);

        let icon_lbl = Label::new(Some(icon));
        icon_lbl.add_css_class("nav-icon");

        let text_lbl = Label::new(Some(label));
        text_lbl.add_css_class("nav-label");
        text_lbl.set_xalign(0.0);

        hbox.append(&icon_lbl);
        hbox.append(&text_lbl);
        row.set_child(Some(&hbox));
        nav_list.append(&row);

        // Tag the row with the page name via widget name
        row.set_widget_name(name);
    }

    // Select home by default
    if let Some(first_row) = nav_list.row_at_index(0) {
        nav_list.select_row(Some(&first_row));
    }

    let stack_clone = stack.clone();
    nav_list.connect_row_selected(move |_, row| {
        if let Some(row) = row {
            let name = row.widget_name();
            stack_clone.set_visible_child_name(&name);
        }
    });

    sidebar_box.append(&nav_list);

    let sep = Separator::new(Orientation::Vertical);
    main_box.append(&sidebar_box);
    main_box.append(&sep);
    main_box.append(&stack);

    window.set_child(Some(&main_box));
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