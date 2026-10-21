use gtk4::prelude::*;
use gtk4::{
    Align, Box as GtkBox, Button, Frame, Label, Orientation, Overlay, Revealer,
    RevealerTransitionType, ScrolledWindow, glib,
};
use std::cell::RefCell;
use std::fs;
use std::os::unix::process::CommandExt;
use std::process::{Command, Stdio};
use std::rc::Rc;
use std::time::Duration;

use infoprober::{parse, Value};

use crate::appearence::get_monitors;
use crate::home::page_scroller;

const CFGPATH: &str = "/var/lib/cynager/info.probe";

const CAPSULE_BIN: &str = "capsule";

fn read_shellout() -> Option<String> {
    let src = fs::read_to_string(CFGPATH).ok()?;
    let doc = parse(&src).ok()?;
    doc.get("set", "shellout")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string())
}

fn write_shellout(monitor: &str) -> Result<(), String> {
    let src = fs::read_to_string(CFGPATH).map_err(|e| format!("failed to read cfg: {e}"))?;
    let mut doc = parse(&src).map_err(|e| format!("failed to parse cfg: {e}"))?;

    let set = doc.section_or_insert("set");
    match set.get_mut("shellout") {
        Some(v) => *v = Value::from(monitor.to_string()),
        None => set.push("shellout", monitor.to_string()),
    }

    doc.validate().map_err(|e| format!("not writing cfg: {e}"))?;
    fs::write(CFGPATH, doc.render()).map_err(|e| format!("failed to write cfg: {e}"))?;

    println!("[calibrate] shellout -> {monitor}");
    Ok(())
}

fn restart_capsule() {
    std::thread::spawn(|| {
        let _ = Command::new("pkill").args(["-x", CAPSULE_BIN]).status();
        std::thread::sleep(Duration::from_millis(400));

        match Command::new(CAPSULE_BIN)
            .process_group(0)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
        {
            Ok(mut child) => {
                let _ = child.wait();
            }
            Err(e) => eprintln!("[calibrate] failed to relaunch {CAPSULE_BIN}: {e}"),
        }
    });
}

fn capsule_running() -> bool {
    Command::new("pgrep")
        .args(["-x", CAPSULE_BIN])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}

#[derive(Clone)]
struct Toast {
    revealer: Revealer,
    label: Label,
    generation: Rc<RefCell<u32>>,
}

impl Toast {
    fn new() -> Self {
        let label = Label::new(None);
        label.add_css_class("shell-toast-label");

        let bubble = GtkBox::new(Orientation::Horizontal, 0);
        bubble.add_css_class("shell-toast");
        bubble.append(&label);

        let revealer = Revealer::new();
        revealer.set_transition_type(RevealerTransitionType::SlideUp);
        revealer.set_transition_duration(200);
        revealer.set_halign(Align::Center);
        revealer.set_valign(Align::End);
        revealer.set_margin_bottom(24);
        revealer.set_can_target(false);
        revealer.set_child(Some(&bubble));

        Self { revealer, label, generation: Rc::new(RefCell::new(0)) }
    }

    fn show(&self, text: &str) {
        self.label.set_text(text);
        self.revealer.set_reveal_child(true);

        let id = {
            let mut g = self.generation.borrow_mut();
            *g += 1;
            *g
        };
        let this = self.clone();
        glib::timeout_add_local_once(Duration::from_millis(2500), move || {
            if *this.generation.borrow() == id {
                this.revealer.set_reveal_child(false);
            }
        });
    }
}


fn build_monitor_button(name: &str, make: &str) -> Button {
    let name_lbl = Label::new(Some(name));
    name_lbl.add_css_class("monitor-btn-label");
    name_lbl.set_halign(Align::Center);
    name_lbl.set_valign(Align::Center);

    let btn = Button::new();
    btn.set_height_request(200);
    btn.set_width_request(300);
    btn.set_child(Some(&name_lbl));
    btn.add_css_class("monitor-btn");
    btn.set_tooltip_text(Some(make));
    btn.set_cursor_from_name(Some("pointer"));
    btn
}

fn mark_selected(buttons: &[(String, Button)], selected: &str) {
    for (name, btn) in buttons {
        if name == selected {
            btn.add_css_class("monitor-selected");
        } else {
            btn.remove_css_class("monitor-selected");
        }
    }
}

fn build_status_bar(toast: &Toast) -> GtkBox {

    let dead_lbl = Label::new(Some("Capsule shell is dead :("));
    dead_lbl.set_halign(Align::Start);
    dead_lbl.set_margin_start(10);
    dead_lbl.set_hexpand(true);

    let restart_btn = Button::with_label("Restart");
    restart_btn.add_css_class("destructive-action");
    restart_btn.set_cursor_from_name(Some("pointer"));

    let dead_bar = GtkBox::new(Orientation::Horizontal, 12);
    dead_bar.add_css_class("capsule-dead");
    dead_bar.set_hexpand(true);
    dead_bar.append(&dead_lbl);
    dead_bar.append(&restart_btn);

    let update = |stack: &GtkBox| {
        stack.set_visible(!capsule_running());
    };
    update(&dead_bar);

    restart_btn.connect_clicked({
        let toast = toast.clone();
        move |_| {
            toast.show("Restarting capsule...");
            restart_capsule();
        }
    });

    let weak = dead_bar.downgrade();
    glib::timeout_add_local(Duration::from_secs(2), move || match weak.upgrade() {
        Some(stack) => {
            update(&stack);
            glib::ControlFlow::Continue
        }
        None => glib::ControlFlow::Break,
    });

    dead_bar
}

pub fn build_shell_page() -> Overlay {
    let content = GtkBox::new(Orientation::Vertical, 16);

    let card = GtkBox::new(Orientation::Vertical, 10);
    card.add_css_class("shortcut-card");
    card.add_css_class("detail-card");
    card.set_margin_top(10);
    card.set_hexpand(true);

    let infoicon = gtk4::Image::from_file("/var/lib/cynager/icons/shell.svg");
    infoicon.set_pixel_size(54);
    infoicon.set_halign(Align::Center);
    infoicon.set_css_classes(&["card-icons"]);

    let title = Label::new(Some("Shell Settings"));
    title.add_css_class("shortcut-title");
    title.set_halign(Align::Center);

    let subtitle = Label::new(Some("Configure the Capsule shell the way you want."));
    subtitle.add_css_class("shortcut-desc");
    subtitle.set_halign(Align::Center);

    card.append(&infoicon);
    card.append(&title);
    card.append(&subtitle);

    let toast = Toast::new();
    card.append(&build_status_bar(&toast));

    content.append(&card);

    let outframe = Frame::new(None);

    let outbox = GtkBox::new(Orientation::Vertical, 10);

    let outtitle = Label::new(Some("Main Shell Output"));
    outtitle.add_css_class("frame-title");
    outtitle.set_halign(Align::Start);
    outtitle.set_margin_start(20);
    outtitle.set_margin_top(20);
    outtitle.set_margin_end(20);

    let outsubtitle = Label::new(Some("Choose your main display"));
    outsubtitle.add_css_class("frame-subtitle");
    outsubtitle.set_halign(Align::Start);
    outsubtitle.set_margin_start(20);
    outsubtitle.set_margin_end(20);

    let current = read_shellout().unwrap_or_else(|| "unknown".to_string());

    let monrow = GtkBox::new(Orientation::Horizontal, 10);
    monrow.set_margin_start(20);
    monrow.set_margin_end(20);
    monrow.set_margin_bottom(20);
    monrow.set_halign(Align::Start);

    let buttons: Rc<Vec<(String, Button)>> = Rc::new(
        get_monitors()
            .into_iter()
            .map(|(name, make)| {
                let btn = build_monitor_button(&name, &make);
                monrow.append(&btn);
                (name, btn)
            })
            .collect(),
    );

    mark_selected(&buttons, &current);

    for (name, btn) in buttons.iter() {
        let name = name.clone();
        let buttons = buttons.clone();
        let toast = toast.clone();

        btn.connect_clicked(move |_| match write_shellout(&name) {
            Ok(()) => {
                mark_selected(&buttons, &name);
                toast.show("Shell display output changed");
                restart_capsule();
            }
            Err(e) => {
                eprintln!("[calibrate] {e}");
                toast.show("Couldn't change shell display output");
            }
        });
    }

    outbox.append(&outtitle);
    outbox.append(&outsubtitle);
    outbox.append(&monrow);

    outframe.set_child(Some(&outbox));
    content.append(&outframe);

    // scroller underneath, toast floating above it
    let overlay = Overlay::new();
    let scroller: ScrolledWindow = page_scroller(&content);
    overlay.set_child(Some(&scroller));
    overlay.add_overlay(&toast.revealer);
    overlay
}