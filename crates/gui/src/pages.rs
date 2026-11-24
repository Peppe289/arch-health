//! Page contents built from core reports. Pure widget construction: no
//! state, no I/O.

use std::rc::Rc;
use std::time::SystemTime;

use arch_health_core::{Category, CheckId, CheckReport, Item, Report, Status};
use gtk::glib;
use gtk::prelude::*;

use crate::charts;
use crate::style;
use crate::table::{self, Row};

const MARGIN: i32 = 18;

/// Called when a category tile on the overview is clicked.
pub type Navigate = Rc<dyn Fn(Category)>;

pub fn clear(container: &gtk::Box) {
    while let Some(child) = container.first_child() {
        container.remove(&child);
    }
}

fn label(text: &str) -> gtk::Label {
    let l = gtk::Label::new(Some(text));
    l.set_xalign(0.0);
    l.set_wrap(true);
    l.set_wrap_mode(gtk::pango::WrapMode::WordChar);
    l
}

fn markup(m: &str) -> gtk::Label {
    let l = label("");
    l.set_markup(m);
    l
}

fn dim(text: &str) -> gtk::Label {
    let l = label(text);
    l.add_css_class("dim");
    l
}

fn section_title(text: &str) -> gtk::Label {
    let l = label(text);
    l.add_css_class("section-title");
    l
}

fn headline(text: &str) -> gtk::Label {
    let l = label(text);
    l.add_css_class("headline");
    l
}

fn card() -> gtk::Box {
    let b = gtk::Box::new(gtk::Orientation::Vertical, 10);
    b.add_css_class("card");
    b
}

/// Vertical box inside a scrolled window: the standard page layout.
pub fn scrolled_page() -> (gtk::ScrolledWindow, gtk::Box) {
    let content = gtk::Box::new(gtk::Orientation::Vertical, 12);
    content.set_margin_top(MARGIN);
    content.set_margin_bottom(MARGIN);
    content.set_margin_start(MARGIN);
    content.set_margin_end(MARGIN);
    // Explicit viewport: the implicit one scrolls to whatever gets focus
    // (e.g. the table), so pages would open scrolled down.
    let viewport = gtk::Viewport::builder()
        .scroll_to_focus(false)
        .child(&content)
        .build();
    let scrolled = gtk::ScrolledWindow::builder()
        .hscrollbar_policy(gtk::PolicyType::Never)
        .vexpand(true)
        .child(&viewport)
        .build();
    (scrolled, content)
}

fn item_row(item: &Item) -> gtk::Box {
    let row = gtk::Box::new(gtk::Orientation::Horizontal, 8);
    let g = style::glyph(item.status);
    if item.status <= Status::Info {
        // Only problems get a visible mark; keep alignment otherwise.
        g.set_opacity(0.0);
    }
    row.append(&g);
    let text = if item.detail.is_empty() {
        format!("<b>{}</b>", glib::markup_escape_text(&item.subject))
    } else {
        format!(
            "<b>{}</b>  {}",
            glib::markup_escape_text(&item.subject),
            glib::markup_escape_text(&item.detail)
        )
    };
    let l = markup(&text);
    l.set_selectable(true);
    l.set_hexpand(true);
    row.append(&l);
    row
}

/// Histogram + table for timed events.
fn timeline_view(rows: Vec<Row>, show_check: bool) -> gtk::Box {
    let b = gtk::Box::new(gtk::Orientation::Vertical, 10);
    let points: Vec<(SystemTime, Status)> = rows
        .iter()
        .map(|r| (r.event.time, r.event.status))
        .collect();
    if let Some(h) = charts::histogram(&points) {
        b.append(&h);
    }
    b.append(&table::events_table(rows, show_check));
    b
}

fn expander(title: &str, child: &impl IsA<gtk::Widget>, expanded: bool) -> gtk::Expander {
    let l = label(title);
    l.add_css_class("check-title");
    gtk::Expander::builder()
        .label_widget(&l)
        .child(child)
        .expanded(expanded)
        .build()
}

/// One check as a card: header, gauges, timeline + table, details.
fn report_card(r: &CheckReport) -> gtk::Box {
    let c = card();
    let header = gtk::Box::new(gtk::Orientation::Horizontal, 8);
    header.append(&style::glyph(r.status));
    let title = label(r.id.title());
    title.add_css_class("check-title");
    title.set_wrap(false);
    header.append(&title);
    let summary = dim(&r.summary);
    summary.set_hexpand(true);
    header.append(&summary);
    c.append(&header);

    if let Some(l) = &r.limitation {
        c.append(&dim(&format!("Incomplete: {l}")));
    }

    for g in &r.gauges {
        c.append(&charts::gauge(g));
    }

    let problem = r.status >= Status::Warning;
    if !r.events.is_empty() {
        let rows = r
            .events
            .iter()
            .map(|e| Row {
                check: None,
                event: e.clone(),
            })
            .collect();
        let title = format!("Timeline ({} events)", r.events.len());
        c.append(&expander(&title, &timeline_view(rows, false), problem));
    }

    // With gauges, the bars already tell the story: list only problems.
    let items: Vec<&Item> = if r.gauges.is_empty() {
        r.items.iter().collect()
    } else {
        r.items.iter().filter(|i| i.status > Status::Ok).collect()
    };
    if !items.is_empty() {
        let body = gtk::Box::new(gtk::Orientation::Vertical, 4);
        body.set_margin_top(4);
        for item in &items {
            body.append(&item_row(item));
        }
        let title = if r.events.is_empty() {
            format!("Details ({})", items.len())
        } else {
            format!("By source ({})", items.len())
        };
        c.append(&expander(&title, &body, problem && r.events.is_empty()));
    }
    c
}

pub fn fill_category(container: &gtk::Box, report: &Report, category: Category) {
    clear(container);
    container.append(&headline(category.title()));
    for r in report.by_category(category) {
        container.append(&report_card(r));
    }
}

/// Number of checks per status, worst first, for the donut.
fn status_counts(report: &Report) -> Vec<(Status, usize)> {
    let order = [
        Status::Critical,
        Status::Unknown,
        Status::Warning,
        Status::Info,
        Status::Ok,
        Status::Skipped,
    ];
    order
        .iter()
        .map(|s| (*s, report.reports.iter().filter(|r| r.status == *s).count()))
        .collect()
}

fn category_tile(report: &Report, category: Category, navigate: &Navigate) -> gtk::Button {
    let status = report.category_status(category);
    let checks = report.by_category(category).count();
    let problems = report
        .by_category(category)
        .filter(|r| r.status >= Status::Warning)
        .count();

    let b = gtk::Box::new(gtk::Orientation::Vertical, 4);
    let top = gtk::Box::new(gtk::Orientation::Horizontal, 8);
    top.append(&style::glyph(status));
    let name = label(category.title());
    name.add_css_class("check-title");
    top.append(&name);
    b.append(&top);
    let detail = match problems {
        0 => format!("{checks} checks, all fine"),
        1 => "1 check needs attention".to_string(),
        n => format!("{n} checks need attention"),
    };
    b.append(&dim(&detail));

    let button = gtk::Button::builder()
        .child(&b)
        .tooltip_text(format!("Open {}", category.title()))
        .build();
    button.add_css_class("tile");
    button.add_css_class(style::tile_class(status));
    let nav = navigate.clone();
    button.connect_clicked(move |_| nav(category));
    button
}

/// Donut, legend, overall headline and notes.
fn overview_summary(report: &Report, last_run: Option<&str>) -> gtk::Box {
    let overall = report.overall();
    let row = gtk::Box::new(gtk::Orientation::Horizontal, 24);
    let counts = status_counts(report);
    row.append(&charts::donut(counts.clone(), 150));
    row.append(&charts::legend(&counts));

    let text = gtk::Box::new(gtk::Orientation::Vertical, 6);
    text.set_valign(gtk::Align::Center);
    text.set_hexpand(true);
    let h = gtk::Box::new(gtk::Orientation::Horizontal, 10);
    let g = style::glyph(overall);
    g.add_css_class("headline");
    h.append(&g);
    h.append(&headline(overall.headline()));
    text.append(&h);
    if let Some(t) = last_run {
        text.append(&dim(&format!("Last check at {t}")));
    }
    if report.root_would_help {
        text.append(&dim(
            "Some results are incomplete without administrator privileges: click \
             Unlock… to complete them, or run `sudo arch-health` in a terminal.",
        ));
    }
    row.append(&text);

    let summary = card();
    summary.append(&row);
    summary
}

fn overview_tiles(report: &Report, navigate: &Navigate) -> gtk::FlowBox {
    let tiles = gtk::FlowBox::builder()
        .selection_mode(gtk::SelectionMode::None)
        .column_spacing(10)
        .row_spacing(10)
        .min_children_per_line(2)
        .max_children_per_line(3)
        .homogeneous(true)
        .build();
    for c in Category::ALL {
        tiles.insert(&category_tile(report, c, navigate), -1);
    }
    tiles
}

/// Problem items across checks; a check in trouble without problem items
/// (e.g. a degraded service manager) is listed by its summary.
fn overview_attention(report: &Report) -> gtk::Box {
    let attention = card();
    let mut any = false;
    for r in &report.reports {
        let problems: Vec<&Item> = r
            .items
            .iter()
            .filter(|i| i.status >= Status::Warning)
            .collect();
        if r.status >= Status::Warning && problems.is_empty() {
            any = true;
            let row = gtk::Box::new(gtk::Orientation::Horizontal, 8);
            row.append(&style::glyph(r.status));
            row.append(&markup(&format!(
                "<b>{}</b>  {}",
                glib::markup_escape_text(r.id.title()),
                glib::markup_escape_text(&r.summary)
            )));
            attention.append(&row);
        }
        for item in problems {
            any = true;
            let row = item_row(item);
            let check = dim(&format!("{} ·", r.id.title()));
            check.set_hexpand(false);
            row.insert_child_after(&check, row.first_child().as_ref());
            attention.append(&row);
        }
    }
    if !any {
        attention.append(&label("Nothing needs attention."));
    }
    attention
}

/// Timeline and table of the timed problems of every check.
fn overview_errors(report: &Report) -> gtk::Box {
    let rows: Vec<Row> = report
        .reports
        .iter()
        .flat_map(|r| {
            r.events
                .iter()
                .filter(|e| e.status >= Status::Warning)
                .map(|e| Row {
                    check: Some(r.id.title()),
                    event: e.clone(),
                })
        })
        .collect();
    let errors = card();
    if rows.is_empty() {
        errors.append(&label("No errors recorded."));
    } else {
        errors.append(&dim(
            "Errors and crashes with their time. Hover a bar for details; \
             click a column header to sort.",
        ));
        errors.append(&timeline_view(rows, true));
    }
    errors
}

pub fn fill_overview(
    container: &gtk::Box,
    report: Option<&Report>,
    last_run: Option<&str>,
    navigate: &Navigate,
) {
    clear(container);
    let Some(report) = report else {
        let spinner = gtk::Spinner::new();
        spinner.start();
        spinner.set_size_request(32, 32);
        container.append(&spinner);
        container.append(&label("Running the first check…"));
        return;
    };
    container.append(&overview_summary(report, last_run));
    container.append(&section_title("Categories"));
    container.append(&overview_tiles(report, navigate));
    container.append(&section_title("Needs attention"));
    container.append(&overview_attention(report));
    container.append(&section_title("Recent errors"));
    container.append(&overview_errors(report));
}

/// Generated from the CLI definition, so it never drifts from `--help`.
pub fn command_line_page() -> gtk::Widget {
    let (scrolled, content) = scrolled_page();
    content.append(&headline("Command line"));
    content.append(&label(
        "The same checks are available in a terminal, also over SSH, with the \
         arch-health command.",
    ));

    let mut cmd = arch_health_cli::command();
    cmd.build();

    content.append(&section_title("Usage"));
    let usage = label(&cmd.render_usage().to_string());
    usage.add_css_class("monospace");
    usage.set_selectable(true);
    content.append(&usage);

    content.append(&section_title("Options"));
    let grid = gtk::Grid::builder()
        .column_spacing(18)
        .row_spacing(6)
        .build();
    let mut row = 0;
    for arg in cmd.get_arguments().filter(|a| !a.is_hide_set()) {
        let mut flags = Vec::new();
        if let Some(s) = arg.get_short() {
            flags.push(format!("-{s}"));
        }
        if let Some(l) = arg.get_long() {
            flags.push(format!("--{l}"));
        }
        if flags.is_empty() {
            continue;
        }
        let mut flag = flags.join(", ");
        if arg.get_action().takes_values() {
            let name = arg
                .get_value_names()
                .and_then(|v| v.first())
                .map(|v| v.to_string())
                .unwrap_or_else(|| arg.get_id().to_string().to_uppercase());
            flag.push_str(&format!(" <{name}>"));
        }
        let f = label(&flag);
        f.add_css_class("monospace");
        f.set_selectable(true);
        f.set_wrap(false);
        grid.attach(&f, 0, row, 1, 1);
        let help = arg.get_help().map(|h| h.to_string()).unwrap_or_default();
        let h = label(&help);
        h.set_hexpand(true);
        grid.attach(&h, 1, row, 1, 1);
        row += 1;
    }
    content.append(&grid);

    content.append(&section_title("Examples"));
    for (example, what) in [
        (
            "arch-health",
            "all checks, refreshing a private copy of the repository databases",
        ),
        ("arch-health --offline", "no network access"),
        ("arch-health --only kernel,disk-space", "only some checks"),
        (
            "sudo arch-health",
            "complete results for checks that need root",
        ),
    ] {
        let r = gtk::Box::new(gtk::Orientation::Horizontal, 18);
        let e = label(example);
        e.add_css_class("monospace");
        e.set_selectable(true);
        e.set_width_chars(38);
        e.set_wrap(false);
        r.append(&e);
        r.append(&label(what));
        content.append(&r);
    }

    content.append(&section_title("Check keys (for --only)"));
    let keys = gtk::Grid::builder()
        .column_spacing(18)
        .row_spacing(4)
        .build();
    for (i, id) in CheckId::ALL.iter().enumerate() {
        let k = label(id.key());
        k.add_css_class("monospace");
        k.set_selectable(true);
        k.set_wrap(false);
        keys.attach(&k, 0, i as i32, 1, 1);
        keys.attach(&label(id.category().title()), 1, i as i32, 1, 1);
        keys.attach(&label(id.title()), 2, i as i32, 1, 1);
    }
    content.append(&keys);

    if let Some(after) = cmd.get_after_help() {
        content.append(&section_title("Notes"));
        content.append(&label(&after.to_string()));
    }
    scrolled.upcast()
}
