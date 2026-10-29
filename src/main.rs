use gtk4::prelude::*;
use gtk4::{
    Application, ApplicationWindow, Box as GtkBox, Label, ListBox, ListBoxRow,
    Orientation, ScrolledWindow, SearchEntry, Stack, Separator,
    Frame, Align, CssProvider, Image, gio, glib,
};
use std::cell::RefCell;
use std::fs;
use std::process::exit;
use std::rc::Rc;
use std::time::Duration;

use self_update::cargo_crate_version;

const APP_ID: &str = "ekah.scu.calibrate";

const TAB_IDS: &[&str] = &[
    "home", "appearance", "shellset", "display", "sound", "net", "blue", 
    "Storage", "bat", "shortcuts", "pills", "apps", "help", "abt",
];

mod home;
mod appearence;
mod shellsettings;
mod display;
mod sound;
mod network;

struct MenuItem {
    id: &'static str,
    title: &'static str,
}

const MENU_ITEMS: &[MenuItem] = &[
    MenuItem { id: "home", title: "Home" },
    MenuItem { id: "appearance", title: "Appearance" },
    MenuItem { id: "shellset", title: "Shell" },
    MenuItem { id: "display", title: "Display" },
    MenuItem { id: "sound", title: "Sound" },
    MenuItem { id: "net", title: "Network" },
    MenuItem { id: "blue", title: "Bluetooth" },
    MenuItem { id: "Storage", title: "Storage" },
    MenuItem { id: "bat", title: "Battery" },
    MenuItem { id: "shortcuts", title: "Shortcuts" },
    MenuItem { id: "pills", title: "Pills" },
    MenuItem { id: "apps", title: "Applications" },
    MenuItem { id: "help", title: "Help" },
    MenuItem { id: "abt", title: "About" },
];

/// Icon shown next to each entry in the side tab switcher (theme icon names).
fn menu_icon(id: &str) -> &'static str {
    match id {
        "home" => "go-home-symbolic",
        "appearance" => "gnumeric-brush",
        "shellset" => "automated-tasks",
        "display" => "video-display-symbolic",
        "sound" => "audio-speakers-symbolic",
        "net" => "network-wireless-symbolic",
        "blue" => "bluetooth-symbolic",
        "Storage" => "media-floppy-symbolic",
        "bat" => "battery-symbolic",
        "shortcuts" => "input-keyboard-symbolic",
        "pills" => "applications-other-symbolic",
        "apps" => "view-app-grid-symbolic",
        "help" => "help-browser-symbolic",
        "abt" => "help-about-symbolic",
        _ => "preferences-system-symbolic",
    }
}

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

const BASE_CSS: &str = include_str!("../assets/style.css");
const LIGHT_CSS: &str = include_str!("../assets/light.css");

/// True when GNOME is set to dark (or the schema is missing, in which case we keep the original dark look).
fn system_is_dark() -> bool {
    let has_schema = gio::SettingsSchemaSource::default()
        .and_then(|s| s.lookup("org.gnome.desktop.interface", true))
        .is_some();
    if !has_schema {
        return true;
    }
    let settings = gio::Settings::new("org.gnome.desktop.interface");
    // "default" and "prefer-dark" => dark, "prefer-light" => light
    settings.string("color-scheme") != "prefer-light"
}

fn apply_theme(base: &CssProvider, light: &CssProvider) {
    let dark = system_is_dark();

    base.load_from_data(BASE_CSS);
    light.load_from_data(if dark { "" } else { LIGHT_CSS });
}

fn load_css() {
    let base = CssProvider::new();
    let light = CssProvider::new();
    let display = gtk4::gdk::Display::default().expect("Could not connect to display");

    gtk4::style_context_add_provider_for_display(&display, &base, gtk4::STYLE_PROVIDER_PRIORITY_APPLICATION);
    // one step higher so the light overrides win over the base sheet
    gtk4::style_context_add_provider_for_display(&display, &light, gtk4::STYLE_PROVIDER_PRIORITY_APPLICATION + 1);

    apply_theme(&base, &light);

    // re-theme live when the user flips GNOME between light and dark
    let has_schema = gio::SettingsSchemaSource::default()
        .and_then(|s| s.lookup("org.gnome.desktop.interface", true))
        .is_some();
    if has_schema {
        let settings = gio::Settings::new("org.gnome.desktop.interface");
        settings.connect_changed(Some("color-scheme"), move |_, _| apply_theme(&base, &light));
        // keep the Settings object alive for the life of the app
        std::mem::forget(settings);
    }
}

fn page_scroller(content: &GtkBox) -> ScrolledWindow {
    content.add_css_class("settings-page");
    ScrolledWindow::builder()
        .vscrollbar_policy(gtk4::PolicyType::Always)
        .hscrollbar_policy(gtk4::PolicyType::Never)
        .vexpand(true)
        .hexpand(true)
        .child(content)
        .build()
}

fn build_placeholder_page(title: &str, message: &str) -> ScrolledWindow {
    let content = GtkBox::new(Orientation::Vertical, 16);

    let frame = Frame::new(None);
    let inner = GtkBox::new(Orientation::Vertical, 8);
    inner.set_margin_top(40);
    inner.set_margin_bottom(40);
    inner.set_margin_start(20);
    inner.set_margin_end(20);
    inner.set_halign(Align::Center);
    inner.set_valign(Align::Center);

    let title_lbl = Label::new(Some(title));
    title_lbl.add_css_class("shortcut-title");
    title_lbl.set_halign(Align::Center);

    let msg_lbl = Label::new(Some(message));
    msg_lbl.add_css_class("shortcut-desc");
    msg_lbl.set_halign(Align::Center);
    msg_lbl.set_justify(gtk4::Justification::Center);
    msg_lbl.set_wrap(true);

    inner.append(&title_lbl);
    inner.append(&msg_lbl);

    frame.set_child(Some(&inner));
    content.append(&frame);
    page_scroller(&content)
}

fn build_page_header() -> (GtkBox, Label, Label) {
    let header = GtkBox::new(Orientation::Horizontal, 4);
    header.add_css_class("page-header-bar");

    let title_lbl = Label::new(None);
    title_lbl.add_css_class("page-title");
    title_lbl.set_halign(Align::Start);
    // title_lbl.set_justify(gtk4::Justification::Right);

    let subtitle_lbl = Label::new(None);
    subtitle_lbl.add_css_class("page-subtitle");
    subtitle_lbl.set_halign(Align::Start);

    header.append(&title_lbl);
    // header.append(&subtitle_lbl);
    header.set_margin_start(10);

    (header, title_lbl, subtitle_lbl)
}

fn page_meta(id: &str, username: &str) -> (String, String) {
    match id {
        "home" => (
            format!("Porfile: [{}] - Calibrate by SCU", username),
            "Jump straight into a settings category below.".to_string(),
        ),
        "appearance" => (
            "Appearance".to_string(),
            "Change the overall look and theme of the OS.".to_string(),
        ),
        "shellset" => (
            "Shell".to_string(),
            "Configure shell behavior and startup options.".to_string(),
        ),
        "display" => (
            "Display".to_string(),
            "Configure how your screen looks and behaves.".to_string(),
        ),
        "sound" => (
            "Sound".to_string(),
            "Control audio input, output, and alerts.".to_string(),
        ),
        "net" => (
            "Network".to_string(),
            "Manage Wi-Fi, VPN, and connection settings.".to_string(),
        ),
        "blue" => (
            "Bluetooth".to_string(),
            "Manage paired devices and Bluetooth visibility.".to_string(),
        ),
        "Storage" => (
            "Storage".to_string(),
            "Review disk usage across mounted partitions.".to_string(),
        ),
        "bat" => (
            "Battery".to_string(),
            "Power usage and battery health settings.".to_string(),
        ),
        "shortcuts" => (
            "Shortcuts".to_string(),
            "Customize keyboard shortcuts and hotkeys.".to_string(),
        ),
        "pills" => (
            "Pills".to_string(),
            "Placeholder section — not yet defined.".to_string(),
        ),
        "apps" => (
            "Applications".to_string(),
            "Manage installed applications and defaults.".to_string(),
        ),
        "help" => (
            "Help".to_string(),
            "Troubleshooting tips and common fixes.".to_string(),
        ),
        "abt" => (
            "About".to_string(),
            "System and version information.".to_string(),
        ),
        _ => ("Settings".to_string(), String::new()),
    }
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

fn build_help_page() -> ScrolledWindow {
    let content = GtkBox::new(Orientation::Vertical, 16);

    let card = GtkBox::new(Orientation::Vertical, 10);
    card.add_css_class("shortcut-card");
    card.add_css_class("detail-card");
    card.set_margin_top(10);
    card.set_hexpand(true);

    let title = Label::new(Some("Help"));
    title.add_css_class("shortcut-title");
    title.set_halign(Align::Center);

    let subtitle = Label::new(Some("Common problems and how to fix them."));
    subtitle.add_css_class("shortcut-desc");
    subtitle.set_halign(Align::Center);

    card.append(&title);
    card.append(&subtitle);
    content.append(&card);

    let topics: [(&str, &str); 4] = [
        (
            "The shell isn't showing",
            "Open the Shell tab. If it says Capsule shell is dead, press Restart.",
        ),
        (
            "Widget changes don't show up",
            "Toggling a widget only updates info.probe. Restart Capsule from the Shell tab to apply it.",
        ),
        (
            "The shell is on the wrong screen",
            "Pick the display you want under Main Shell Output in the Shell tab.",
        ),
        (
            "Settings won't save",
            "Calibrate writes to /var/lib/cynager/info.probe. Check that your user can write to that file.",
        ),
    ];

    for (heading, body) in topics {
        let frame = Frame::new(None);
        let inner = GtkBox::new(Orientation::Vertical, 6);
        inner.set_margin_top(16);
        inner.set_margin_bottom(16);
        inner.set_margin_start(20);
        inner.set_margin_end(20);

        let h = Label::new(Some(heading));
        h.add_css_class("frame-title");
        h.set_halign(Align::Start);
        h.set_xalign(0.0);

        let b = Label::new(Some(body));
        b.add_css_class("frame-subtitle");
        b.set_halign(Align::Start);
        b.set_xalign(0.0);
        b.set_wrap(true);

        inner.append(&h);
        inner.append(&b);
        frame.set_child(Some(&inner));
        content.append(&frame);
    }

    page_scroller(&content)
}

fn build_about_page(username: &str) -> ScrolledWindow {
    let content = GtkBox::new(Orientation::Vertical, 16);

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

// ───────────────────────────── settings search ─────────────────────────────

/// One searchable title/subtitle somewhere inside a page.
struct SearchHit {
    page: &'static str,
    page_title: &'static str,
    label: Label,
}

fn collect_labels(root: &gtk4::Widget, out: &mut Vec<Label>) {
    if let Ok(l) = root.clone().downcast::<Label>() {
        out.push(l);
    }
    let mut child = root.first_child();
    while let Some(c) = child {
        collect_labels(&c, out);
        child = c.next_sibling();
    }
}

/// Walks every page and keeps the labels that are titles or subtitles.
/// The label widget is stored (not its text), so text that changes later is searched as-is.
fn build_search_index(stack: &Stack) -> Vec<SearchHit> {
    let mut hits = Vec::new();
    for item in MENU_ITEMS {
        let Some(page) = stack.child_by_name(item.id) else { continue };
        let mut labels = Vec::new();
        collect_labels(&page, &mut labels);

        // sound / network build their rows with `labeled_row` (row-label + row-caption)
        let rows_too = matches!(item.id, "sound" | "net");
        for l in labels {
            let is_title = l.has_css_class("frame-title")
                || l.has_css_class("frame-subtitle")
                || l.has_css_class("setting-title")
                || l.has_css_class("shortcut-title")
                || l.has_css_class("shortcut-desc")
                || (rows_too && (l.has_css_class("row-label") || l.has_css_class("row-caption")));
            if is_title && !l.text().trim().is_empty() {
                hits.push(SearchHit { page: item.id, page_title: item.title, label: l });
            }
        }
    }
    hits
}

/// Briefly highlights `label` and scrolls its page so it is visible.
fn scroll_to_label(label: Label, tries: u8) {
    glib::timeout_add_local_once(Duration::from_millis(80), move || {
        // nearest ancestor that actually scrolls vertically
        let mut scroller: Option<ScrolledWindow> = None;
        let mut w = label.parent();
        while let Some(x) = w {
            if let Ok(sc) = x.clone().downcast::<ScrolledWindow>() {
                if sc.policy().1 != gtk4::PolicyType::Never {
                    scroller = Some(sc);
                    break;
                }
            }
            w = x.parent();
        }

        let pos = scroller.as_ref().and_then(|sc| {
            let content = sc.child().and_downcast::<gtk4::Viewport>().and_then(|v| v.child())?;
            label.translate_coordinates(&content, 0.0, 0.0)
        });

        match (scroller, pos) {
            (Some(sc), Some((_, y))) if label.is_mapped() => {
                let adj = sc.vadjustment();
                let max = (adj.upper() - adj.page_size()).max(0.0);
                adj.set_value((y - 80.0).max(0.0).min(max));

                label.add_css_class("search-flash");
                let l = label.clone();
                glib::timeout_add_local_once(Duration::from_millis(1600), move || {
                    l.remove_css_class("search-flash");
                });
            }
            _ => {
                // page may not be laid out yet (stack is still switching)
                if tries > 0 {
                    scroll_to_label(label, tries - 1);
                }
            }
        }
    });
}

fn build_right_panel(stack: &Stack) -> (GtkBox, ListBox) {
    let panel = GtkBox::new(Orientation::Vertical, 0);
    panel.add_css_class("right-panel");
    panel.set_size_request(220, -1);
    panel.set_margin_start(10);
    panel.set_hexpand(false);

    let search = SearchEntry::new();
    search.add_css_class("menu-search");
    search.set_placeholder_text(Some("Search in settings"));

    // ── normal page menu ──
    let list = ListBox::new();
    list.set_selection_mode(gtk4::SelectionMode::Single);
    list.add_css_class("menu-list");

    for item in MENU_ITEMS {
        let row = ListBoxRow::new();
        row.set_widget_name(item.id);
        row.set_margin_bottom(5);

        let icon = Image::from_icon_name(menu_icon(item.id));
        icon.set_pixel_size(20);
        icon.set_valign(Align::Center);
        icon.add_css_class("card-icons-smol");

        let label = Label::new(Some(item.title));
        label.set_halign(Align::Start);
        label.set_valign(Align::Center);

        let row_content = GtkBox::new(Orientation::Horizontal, 12);
        row_content.append(&icon);
        row_content.append(&label);
        row.set_child(Some(&row_content));

        list.append(&row);
    }

    let stack_for_activate = stack.clone();
    list.connect_row_activated(move |_, row| {
        stack_for_activate.set_visible_child_name(&row.widget_name());
    });

    let list_scroller = ScrolledWindow::builder()
        .vscrollbar_policy(gtk4::PolicyType::Automatic)
        .hscrollbar_policy(gtk4::PolicyType::Never)
        .vexpand(true)
        .child(&list)
        .build();

    // ── search results (shown instead of the menu while there is a query) ──
    let index = Rc::new(build_search_index(stack));
    // row index -> (page id, label to scroll to)
    let targets: Rc<RefCell<Vec<Option<(&'static str, Option<Label>)>>>> =
        Rc::new(RefCell::new(Vec::new()));

    let results = ListBox::new();
    results.set_selection_mode(gtk4::SelectionMode::Single);
    results.add_css_class("menu-list");

    let results_scroller = ScrolledWindow::builder()
        .vscrollbar_policy(gtk4::PolicyType::Automatic)
        .hscrollbar_policy(gtk4::PolicyType::Never)
        .vexpand(true)
        .child(&results)
        .visible(false)
        .build();

    let result_row = |title: &str, sub: &str| -> ListBoxRow {
        let b = GtkBox::new(Orientation::Vertical, 2);
        let t = Label::new(Some(title));
        t.set_halign(Align::Start);
        t.set_xalign(0.0);
        t.set_wrap(true);
        t.set_max_width_chars(24);
        let p = Label::new(Some(sub));
        p.add_css_class("frame-subtitle");
        p.set_halign(Align::Start);
        b.append(&t);
        b.append(&p);
        let row = ListBoxRow::new();
        row.set_margin_bottom(5);
        row.set_child(Some(&b));
        row
    };

    let refill: Rc<dyn Fn(&str)> = {
        let results = results.clone();
        let results_scroller = results_scroller.clone();
        let list_scroller = list_scroller.clone();
        let index = index.clone();
        let targets = targets.clone();
        Rc::new(move |query: &str| {
            let q = query.trim().to_lowercase();
            list_scroller.set_visible(q.is_empty());
            results_scroller.set_visible(!q.is_empty());

            while let Some(r) = results.first_child() {
                results.remove(&r);
            }
            let mut t = targets.borrow_mut();
            t.clear();
            if q.is_empty() {
                return;
            }

            // page names first
            for item in MENU_ITEMS {
                if item.title.to_lowercase().contains(&q) {
                    results.append(&result_row(item.title, "Page"));
                    t.push(Some((item.id, None)));
                }
            }
            // then every matching title / subtitle
            let mut seen: Vec<(&str, String)> = Vec::new();
            for hit in index.iter() {
                let text = hit.label.text().to_string();
                if !text.to_lowercase().contains(&q) {
                    continue;
                }
                if seen.iter().any(|(p, s)| *p == hit.page && *s == text) {
                    continue;
                }
                if t.len() >= 40 {
                    break;
                }
                results.append(&result_row(&text, hit.page_title));
                t.push(Some((hit.page, Some(hit.label.clone()))));
                seen.push((hit.page, text));
            }

            if t.is_empty() {
                let row = result_row("No results", "Try a different word");
                row.set_activatable(false);
                row.set_selectable(false);
                results.append(&row);
                t.push(None);
            }
        })
    };

    {
        let refill = refill.clone();
        search.connect_search_changed(move |s| refill(&s.text()));
    }

    let open_result: Rc<dyn Fn(usize)> = {
        let stack = stack.clone();
        let targets = targets.clone();
        Rc::new(move |i: usize| {
            let target = targets.borrow().get(i).cloned().flatten();
            if let Some((page, label)) = target {
                stack.set_visible_child_name(page);
                if let Some(l) = label {
                    scroll_to_label(l, 6);
                }
            }
        })
    };

    {
        let open_result = open_result.clone();
        results.connect_row_activated(move |_, row| open_result(row.index() as usize));
    }
    {
        // Enter in the search box opens the first result
        let open_result = open_result.clone();
        search.connect_activate(move |_| open_result(0));
    }

    panel.append(&search);
    panel.append(&list_scroller);
    panel.append(&results_scroller);

    (panel, list)
}

fn build_ui(app: &Application, initial_tab: Option<String>) {
    load_css();

    let username = read_username();

    let window = ApplicationWindow::builder()
        .application(app)
        .title("Calibrate")
        .default_width(1300)
        .default_height(800)
        .resizable(true)
        .build();

    window.connect_map(|_| println!("ready"));

    let stack = Stack::new();
    stack.set_transition_type(gtk4::StackTransitionType::SlideUpDown);
    stack.set_transition_duration(300);

    let home_page = home::build_home_page();
    stack.add_titled(&home_page, Some("home"), "Home");

    let appearance_page = appearence::build_appearance_page(&window);
    stack.add_titled(&appearance_page, Some("appearance"), "Appearance");

    let stack_help = stack.clone();
    let stack_apps = stack.clone();
    let shellset_page = shellsettings::build_shell_page(
        move || stack_help.set_visible_child_name("help"),
        move || stack_apps.set_visible_child_name("apps"),
    );
    stack.add_titled(&shellset_page, Some("shellset"), "Shell");

    let display_page = display::build_display_page();
    stack.add_titled(&display_page, Some("display"), "Display");

    let sound_page = sound::build_sound_page();
    stack.add_titled(&sound_page, Some("sound"), "Sound");

    let network_page = network::build_net_page();
    stack.add_titled(&network_page, Some("net"), "Network");

    let blue_page = build_placeholder_page("Bluetooth", "Bluetooth settings are coming soon.");
    stack.add_titled(&blue_page, Some("blue"), "Bluetooth");

    let storage_page = build_placeholder_page("Storage", "Detailed storage settings are coming soon.");
    stack.add_titled(&storage_page, Some("Storage"), "Storage");

    let battery_page = build_placeholder_page("Battery", "Battery settings are coming soon.");
    stack.add_titled(&battery_page, Some("bat"), "Battery");

    let shortcuts_page = build_placeholder_page("Shortcuts", "Keyboard shortcut settings are coming soon.");
    stack.add_titled(&shortcuts_page, Some("shortcuts"), "Shortcuts");

    let pills_page = build_placeholder_page("Pills", "This section is not yet defined.");
    stack.add_titled(&pills_page, Some("pills"), "Pills");

    let apps_page = build_placeholder_page("Applications", "Application settings are coming soon.");
    stack.add_titled(&apps_page, Some("apps"), "Applications");

    let help_page = build_help_page();
    stack.add_titled(&help_page, Some("help"), "Help");

    let about_page = build_about_page(&username);
    stack.add_titled(&about_page, Some("abt"), "About");

    let (header, title_lbl, subtitle_lbl) = build_page_header();

    let (right_panel, menu_list) = build_right_panel(&stack);

    let sync_for_change: Rc<dyn Fn(&str)> = {
        let title_lbl = title_lbl.clone();
        let subtitle_lbl = subtitle_lbl.clone();
        let username = username.clone();
        let menu_list = menu_list.clone();
        Rc::new(move |id: &str| {
            let (title, subtitle) = page_meta(id, &username);
            title_lbl.set_text(&title);
            subtitle_lbl.set_text(&subtitle);

            let mut idx = 0;
            while let Some(row) = menu_list.row_at_index(idx) {
                if row.widget_name() == id {
                    menu_list.select_row(Some(&row));
                    break;
                }
                idx += 1;
            }
        })
    };

    let sync_for_signal = sync_for_change.clone();
    stack.connect_notify_local(Some("visible-child-name"), move |stack, _| {
        if let Some(name) = stack.visible_child_name() {
            sync_for_signal(&name);
        }
    });

    let content_area = GtkBox::new(Orientation::Vertical, 0);
    content_area.set_hexpand(true);
    content_area.append(&stack);
    content_area.append(&header);

    let main_box = GtkBox::new(Orientation::Horizontal, 0);
    main_box.append(&content_area);
    main_box.append(&right_panel);

    window.set_child(Some(&main_box));

    let target = initial_tab.unwrap_or_else(|| "home".to_string());
    stack.set_visible_child_name(&target);
    sync_for_change(&target);

    window.present();
}

struct UpdateOutcome {
    updated: bool,
    version: String,
}

const CALIBRATE_BIN_PATH: &str = "/usr/bin/calibrate";

fn ensure_writable(path: &str) -> Result<(), String> {
    use std::ffi::CString;

    let c_path = CString::new(path).map_err(|e| e.to_string())?;

    let path_obj = std::path::Path::new(path);
    if path_obj.exists() {
        let writable = unsafe { libc::access(c_path.as_ptr(), libc::W_OK) == 0 };
        if !writable {
            return Err(format!(
                "no write permission for '{}'. Try: sudo calibrate --update",
                path
            ));
        }
    }

    if let Some(parent) = path_obj.parent() {
        let c_parent = CString::new(parent.to_string_lossy().to_string())
            .map_err(|e| e.to_string())?;
        let dir_writable = unsafe { libc::access(c_parent.as_ptr(), libc::W_OK) == 0 };
        if !dir_writable {
            return Err(format!(
                "no write permission for directory '{}'. Try: sudo calibrate --update",
                parent.display()
            ));
        }
    }

    Ok(())
}

fn check_for_updates() -> Result<UpdateOutcome, Box<dyn std::error::Error>> {
    let status = self_update::backends::github::Update::configure()
        .repo_owner("ekahPruthvi")
        .repo_name("calibrate")
        .bin_name("calibrate")
        .target("x86_64")
        .bin_install_path(CALIBRATE_BIN_PATH)
        .show_download_progress(true)
        .no_confirm(true)
        .current_version(cargo_crate_version!())
        .build()?
        .update()?;

    Ok(UpdateOutcome {
        updated: status.is_updated(),
        version: status.version().to_string(),
    })
}

fn run_update() -> i32 {
    println!("Checking for updates...");

    if let Err(msg) = ensure_writable(CALIBRATE_BIN_PATH) {
        eprintln!("Cannot update calibrate: {}", msg);
        return 1;
    }

    match check_for_updates() {
        Ok(outcome) => {
            if outcome.updated {
                println!("calibrate updated to version {}", outcome.version);
            } else {
                println!("calibrate is already up to date ({})", outcome.version);
            }
            0
        }
        Err(e) => {
            eprintln!("Update failed: {}", e);
            1
        }
    }
}

fn main() -> gtk4::glib::ExitCode {
    let args: Vec<String> = std::env::args().collect();
    
    if args.len() > 1 && (args[1] == "--version" || args[1] == "-V") {
        println!("{} {}", env!("CARGO_PKG_NAME"), env!("CARGO_PKG_VERSION"));
        exit(1);
    } else if args.len() > 1 && (args[1] == "--update" || args[1] == "-U") {
        exit(run_update());
    }

    let initial_tab = parse_tab_arg();

    let app = Application::builder()
        .application_id(APP_ID)
        .build();

    app.connect_activate(move |app| {
        build_ui(app, initial_tab.clone());
    });

    app.run_with_args::<&str>(&[])
}