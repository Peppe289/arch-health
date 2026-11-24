//! arch-health-gui: desktop window and tray monitor (G1–G4).
//!
//! Single instance: launching it again shows the existing window.
//! `--background` starts it in the tray only (for session autostart).

mod app;
mod notify;
mod pages;
mod style;
mod tray;
mod window;

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use gtk::glib;
use gtk::prelude::*;

const APP_ID: &str = "io.github.archhealth.ArchHealth";

const USAGE: &str = "\
Usage: arch-health-gui [--background]

Desktop window and tray monitor of arch-health.

Options:
  --background  start in the tray without opening the window
  -h, --help    print this help";

fn main() -> glib::ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.iter().any(|a| a == "-h" || a == "--help") {
        println!("{USAGE}");
        return glib::ExitCode::SUCCESS;
    }
    if let Some(unknown) = args.iter().find(|a| *a != "--background") {
        eprintln!("arch-health-gui: unknown option `{unknown}`\n\n{USAGE}");
        return glib::ExitCode::FAILURE;
    }
    let background = !args.is_empty();

    let gtk_app = gtk::Application::builder().application_id(APP_ID).build();
    let state: Rc<RefCell<Option<Rc<app::App>>>> = Rc::default();

    // `startup` runs once, in the primary instance only.
    let s = state.clone();
    gtk_app.connect_startup(move |a| {
        *s.borrow_mut() = Some(app::App::new(a));
    });

    // `activate` runs on every launch, including from a second instance.
    let first = Cell::new(true);
    gtk_app.connect_activate(move |_| {
        if first.replace(false) && background {
            return;
        }
        if let Some(a) = state.borrow().as_ref() {
            a.present();
        }
    });

    // Our options are handled above; GTK gets none.
    gtk_app.run_with_args::<&str>(&[])
}
