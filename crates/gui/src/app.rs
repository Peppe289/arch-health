//! Application state: runs checks on a worker thread, updates window and
//! tray with the results, notifies when the status gets worse.

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use std::sync::Arc;

use arch_health_adapters::helper::{HelperClient, HelperError};
use arch_health_adapters::{Options, system_ports, system_ports_with_helper};
use arch_health_core::checks::{Context, all_checks};
use arch_health_core::{Category, Config, Event, Report, Status, run};
use gtk::prelude::*;
use gtk::{gio, glib};
use ksni::blocking::{Handle, TrayMethods};

use crate::notify;
use crate::pages;
use crate::style;
use crate::tray::{HealthTray, TrayCommand};
use crate::window::{self, Ui};

/// Background check interval (G3). Each run refreshes the repository
/// databases and queries the AUR, so it should not be too frequent.
const CHECK_INTERVAL_SECS: u32 = 6 * 3600;
/// Notifications list at most this many problems.
const NOTIFY_MAX_LINES: usize = 3;

enum WorkerMsg {
    Progress(Event),
    Done(Report),
    /// Answer to an Unlock request (polkit).
    Unlocked(Result<bool, HelperError>),
}

pub struct App {
    gtk_app: gtk::Application,
    ui: Ui,
    total_checks: usize,
    running: Cell<bool>,
    /// Privileged reads go through the helper (after Unlock).
    unlocked: Cell<bool>,
    finished: Cell<usize>,
    last_overall: Cell<Option<Status>>,
    tray: RefCell<Option<Handle<HealthTray>>>,
    hold: RefCell<Option<gio::ApplicationHoldGuard>>,
    worker_tx: async_channel::Sender<WorkerMsg>,
    navigate: pages::Navigate,
}

impl App {
    pub fn new(gtk_app: &gtk::Application) -> Rc<Self> {
        style::install();
        let ui = window::build(gtk_app);
        let (worker_tx, worker_rx) = async_channel::unbounded();
        let (tray_tx, tray_rx) = async_channel::unbounded();

        // With a tray the app keeps running when the window is closed; without
        // one (no StatusNotifierItem host) closing the window quits.
        let tray = match HealthTray::new(tray_tx).spawn() {
            Ok(handle) => Some(handle),
            Err(e) => {
                eprintln!("arch-health-gui: tray icon not available: {e}");
                None
            }
        };
        let hold = tray.as_ref().map(|_| {
            ui.window.set_hide_on_close(true);
            gtk_app.hold()
        });

        let sidebar_ui = ui.sidebar.clone();
        let navigate: pages::Navigate = Rc::new(move |c| window::select_page(&sidebar_ui, c.key()));

        let app = Rc::new(Self {
            gtk_app: gtk_app.clone(),
            ui,
            total_checks: all_checks().len(),
            running: Cell::new(false),
            unlocked: Cell::new(false),
            finished: Cell::new(0),
            last_overall: Cell::new(None),
            tray: RefCell::new(tray),
            hold: RefCell::new(hold),
            worker_tx,
            navigate,
        });

        let weak = Rc::downgrade(&app);
        app.ui.run_button.connect_clicked(move |_| {
            if let Some(a) = weak.upgrade() {
                a.start_run();
            }
        });

        let weak = Rc::downgrade(&app);
        app.ui.unlock_button.connect_clicked(move |_| {
            if let Some(a) = weak.upgrade() {
                a.unlock();
            }
        });

        let weak = Rc::downgrade(&app);
        glib::spawn_future_local(async move {
            while let Ok(msg) = worker_rx.recv().await {
                let Some(a) = weak.upgrade() else { break };
                a.on_worker(msg);
            }
        });

        let weak = Rc::downgrade(&app);
        glib::spawn_future_local(async move {
            while let Ok(cmd) = tray_rx.recv().await {
                let Some(a) = weak.upgrade() else { break };
                match cmd {
                    TrayCommand::Open => a.present(),
                    TrayCommand::RunNow => a.start_run(),
                    TrayCommand::Quit => a.quit(),
                }
            }
        });

        let weak = Rc::downgrade(&app);
        glib::timeout_add_seconds_local(CHECK_INTERVAL_SECS, move || match weak.upgrade() {
            Some(a) => {
                a.start_run();
                glib::ControlFlow::Continue
            }
            None => glib::ControlFlow::Break,
        });

        pages::fill_overview(&app.ui.overview, None, None, &app.navigate);
        app.start_run();
        app
    }

    pub fn present(&self) {
        self.ui.window.present();
    }

    pub fn show_page(&self, name: &str) {
        window::select_page(&self.ui.sidebar, name);
    }

    fn quit(&self) {
        if let Some(tray) = self.tray.borrow_mut().take() {
            tray.shutdown().wait();
        }
        self.hold.borrow_mut().take();
        self.gtk_app.quit();
    }

    fn update_tray(&self, f: impl FnOnce(&mut HealthTray)) {
        if let Some(t) = self.tray.borrow().as_ref() {
            t.update(f);
        }
    }

    pub fn start_run(&self) {
        if self.running.replace(true) {
            return;
        }
        self.finished.set(0);
        self.ui.run_button.set_sensitive(false);
        self.ui.spinner.start();
        self.ui.status_bar.set_text("Running checks…");
        self.update_tray(|t| t.running = true);

        let tx = self.worker_tx.clone();
        let unlocked = self.unlocked.get();
        std::thread::spawn(move || {
            // Fresh ports each run: the package snapshot is cached per instance.
            let options = Options {
                sync_databases: true,
            };
            let ports = match unlocked.then(HelperClient::connect) {
                Some(Ok(client)) => system_ports_with_helper(options, Arc::new(client)),
                _ => system_ports(options),
            };
            let config = Config::default();
            let checks = all_checks();
            let ctx = Context {
                ports: &ports,
                config: &config,
            };
            let report = run(&checks, &ctx, |e| {
                let _ = tx.send_blocking(WorkerMsg::Progress(e));
            });
            let _ = tx.send_blocking(WorkerMsg::Done(report));
        });
    }

    fn on_worker(&self, msg: WorkerMsg) {
        match msg {
            WorkerMsg::Progress(Event::Started(_)) => {}
            WorkerMsg::Progress(Event::Finished(_)) => {
                self.finished.set(self.finished.get() + 1);
                self.ui.status_bar.set_text(&format!(
                    "Running checks… {}/{}",
                    self.finished.get(),
                    self.total_checks
                ));
            }
            WorkerMsg::Done(report) => self.on_report(report),
            WorkerMsg::Unlocked(result) => self.on_unlocked(result),
        }
    }

    /// Asks polkit (through the helper) for authorization; the desktop shows
    /// its password dialog. Blocking, so it runs on a worker thread.
    fn unlock(&self) {
        self.ui.unlock_button.set_sensitive(false);
        self.ui.status_bar.set_text("Waiting for authorization…");
        let tx = self.worker_tx.clone();
        std::thread::spawn(move || {
            let result = HelperClient::connect().and_then(|c| c.authorize(true));
            let _ = tx.send_blocking(WorkerMsg::Unlocked(result));
        });
    }

    fn on_unlocked(&self, result: Result<bool, HelperError>) {
        self.ui.unlock_button.set_sensitive(true);
        let (message, detail) = match result {
            Ok(true) => {
                self.unlocked.set(true);
                self.start_run();
                return;
            }
            Ok(false) | Err(HelperError::Denied) => (
                "Authorization not granted",
                "The results that need administrator privileges stay incomplete.".to_string(),
            ),
            Err(HelperError::NotInstalled) => (
                "The privileged helper is not installed",
                "Install arch-health-helper and its D-Bus and polkit files \
                 (see “Privileged helper” in the README), then try again."
                    .to_string(),
            ),
            Err(HelperError::Other(e)) => ("Could not unlock", e),
        };
        self.ui.status_bar.set_text(message);
        gtk::AlertDialog::builder()
            .message(message)
            .detail(detail)
            .modal(true)
            .build()
            .show(Some(&self.ui.window));
    }

    fn on_report(&self, report: Report) {
        self.running.set(false);
        self.ui.spinner.stop();
        self.ui.run_button.set_sensitive(true);
        // Still incomplete while unlocked: polkit's authorization expired
        // (or the helper went away). Offer Unlock again.
        if report.root_would_help {
            self.unlocked.set(false);
        }
        self.ui.unlock_button.set_visible(report.root_would_help);

        let now = glib::DateTime::now_local()
            .and_then(|d| d.format("%H:%M"))
            .map(|s| s.to_string())
            .unwrap_or_default();
        let overall = report.overall();
        self.ui.status_bar.set_text(&format!(
            "Last check at {now} · {} · next in {} h",
            overall.headline(),
            CHECK_INTERVAL_SECS / 3600
        ));

        pages::fill_overview(&self.ui.overview, Some(&report), Some(&now), &self.navigate);
        self.ui.set_overview_glyph(overall);
        for c in Category::ALL {
            if let Some(container) = self.ui.categories.get(&c) {
                pages::fill_category(container, &report, c);
            }
            self.ui.set_category_glyph(c, report.category_status(c));
        }

        let summary = problems_summary(&report, usize::MAX);
        self.update_tray(|t| {
            t.running = false;
            t.status = overall;
            t.summary = if summary.is_empty() {
                "Nothing needs attention".into()
            } else {
                summary
            };
        });

        self.maybe_notify(&report);
    }

    /// Notify only when the status gets worse, by severity level (ok,
    /// warning, critical): a check that could not run (Unknown, e.g. no
    /// network) is not a worse system. First run: critical only.
    fn maybe_notify(&self, report: &Report) {
        let overall = report.overall();
        let previous = self.last_overall.replace(Some(overall));
        let worse = match previous {
            None => overall == Status::Critical,
            Some(p) => overall.exit_code() > p.exit_code(),
        };
        if !worse {
            return;
        }
        let icon = if overall == Status::Critical {
            "dialog-error"
        } else {
            "dialog-warning"
        };
        let title = overall.headline().to_string();
        let body = problems_summary(report, NOTIFY_MAX_LINES);
        std::thread::spawn(move || {
            if let Err(e) = notify::send(icon, &title, &body) {
                eprintln!("arch-health-gui: notification failed: {e}");
            }
        });
    }
}

/// "Check: summary" lines for the checks that need attention, worst first.
fn problems_summary(report: &Report, max: usize) -> String {
    let mut bad: Vec<_> = report
        .reports
        .iter()
        .filter(|r| r.status >= Status::Warning)
        .collect();
    bad.sort_by_key(|r| std::cmp::Reverse(r.status));
    let mut lines: Vec<String> = bad
        .iter()
        .take(max)
        .map(|r| format!("{}: {}", r.id.title(), r.summary))
        .collect();
    if bad.len() > max {
        lines.push(format!("… and {} more", bad.len() - max));
    }
    lines.join("\n")
}
