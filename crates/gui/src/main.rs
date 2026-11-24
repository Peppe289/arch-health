//! arch-health-gui: desktop window and tray monitor (G1–G4).
//!
//! Single instance: launching it again shows the existing window.
//! `--background` starts it in the tray only (for session autostart).

mod app;
mod charts;
mod notify;
mod pages;
mod style;
mod table;
mod timeline;
mod tray;
mod window;

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use gtk::glib;
use gtk::prelude::*;

const APP_ID: &str = "io.github.archhealth.ArchHealth";

const USAGE: &str = "\
Usage: arch-health-gui [--background] [--page <PAGE>]

Desktop window and tray monitor of arch-health.

Options:
  --background    start in the tray without opening the window
  --page <PAGE>   open this page: overview, packages, security, reboot,
                  services, storage, logs, history, command-line
  -h, --help      print this help";

struct Args {
    background: bool,
    page: Option<String>,
}

fn parse_args() -> Result<Args, String> {
    let mut args = Args {
        background: false,
        page: None,
    };
    let mut it = std::env::args().skip(1);
    while let Some(a) = it.next() {
        match a.as_str() {
            "--background" => args.background = true,
            "--page" => {
                let page = it.next().ok_or("--page needs a value")?;
                if !window::page_names()
                    .iter()
                    .any(|n| !n.is_empty() && *n == page)
                {
                    return Err(format!("unknown page `{page}`"));
                }
                args.page = Some(page);
            }
            other => return Err(format!("unknown option `{other}`")),
        }
    }
    Ok(args)
}

fn main() -> glib::ExitCode {
    if std::env::args().any(|a| a == "-h" || a == "--help") {
        println!("{USAGE}");
        return glib::ExitCode::SUCCESS;
    }
    let Args { background, page } = match parse_args() {
        Ok(a) => a,
        Err(e) => {
            eprintln!("arch-health-gui: {e}\n\n{USAGE}");
            return glib::ExitCode::FAILURE;
        }
    };

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
            if let Some(p) = &page {
                a.show_page(p);
            }
            a.present();
        }
    });

    // Our options are handled above; GTK gets none.
    gtk_app.run_with_args::<&str>(&[])
}
