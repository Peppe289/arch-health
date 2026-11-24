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

use std::cell::RefCell;
use std::rc::Rc;

use gtk::prelude::*;
use gtk::{gio, glib};

const APP_ID: &str = "io.github.archhealth.ArchHealth";

const USAGE: &str = "\
Usage: arch-health-gui [--background] [--page <PAGE>]

Desktop window and tray monitor of arch-health.

Options:
  --background    start in the tray without opening the window
  --page <PAGE>   open this page: overview, packages, security, reboot,
                  services, storage, logs, command-line
  -h, --help      print this help";

#[derive(Debug, PartialEq, Eq)]
struct Args {
    background: bool,
    page: Option<String>,
}

/// Parses our options (without the program name).
fn parse_args(args: impl IntoIterator<Item = String>) -> Result<Args, String> {
    let mut parsed = Args {
        background: false,
        page: None,
    };
    let mut it = args.into_iter();
    while let Some(a) = it.next() {
        match a.as_str() {
            "--background" => parsed.background = true,
            "--page" => {
                let page = it.next().ok_or("--page needs a value")?;
                if !window::page_names().contains(&page.as_str()) {
                    return Err(format!("unknown page `{page}`"));
                }
                parsed.page = Some(page);
            }
            other => return Err(format!("unknown option `{other}`")),
        }
    }
    Ok(parsed)
}

fn main() -> glib::ExitCode {
    let argv: Vec<String> = std::env::args().collect();
    if argv.iter().any(|a| a == "-h" || a == "--help") {
        println!("{USAGE}");
        return glib::ExitCode::SUCCESS;
    }
    // Validate here, so errors are printed in the terminal that ran us.
    if let Err(e) = parse_args(argv.iter().skip(1).cloned()) {
        eprintln!("arch-health-gui: {e}\n\n{USAGE}");
        return glib::ExitCode::FAILURE;
    }

    // With HANDLES_COMMAND_LINE the arguments of a second launch are
    // forwarded to the running instance and handled by `command-line`.
    let gtk_app = gtk::Application::builder()
        .application_id(APP_ID)
        .flags(gio::ApplicationFlags::HANDLES_COMMAND_LINE)
        .build();
    let state: Rc<RefCell<Option<Rc<app::App>>>> = Rc::default();

    // `startup` runs once, in the primary instance only.
    let s = state.clone();
    gtk_app.connect_startup(move |a| {
        *s.borrow_mut() = Some(app::App::new(a));
    });

    // Runs in the primary instance for every launch, local or remote.
    gtk_app.connect_command_line(move |_, cmdline| {
        let args: Vec<String> = cmdline
            .arguments()
            .iter()
            .skip(1)
            .map(|a| a.to_string_lossy().into_owned())
            .collect();
        let Ok(Args { background, page }) = parse_args(args) else {
            return glib::ExitCode::FAILURE;
        };
        // --background: stay in the tray (first launch) or do nothing.
        if !background && let Some(a) = state.borrow().as_ref() {
            if let Some(p) = &page {
                a.show_page(p);
            }
            a.present();
        }
        glib::ExitCode::SUCCESS
    });

    gtk_app.run_with_args(&argv)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(args: &[&str]) -> Result<Args, String> {
        parse_args(args.iter().map(|s| s.to_string()))
    }

    #[test]
    fn parses_options() {
        assert_eq!(
            parse(&["--background", "--page", "logs"]),
            Ok(Args {
                background: true,
                page: Some("logs".into())
            })
        );
        assert!(parse(&["--page"]).is_err());
        assert!(parse(&["--page", "nope"]).is_err());
        assert!(parse(&["--wat"]).is_err());
    }
}
