//! Page contents built from core reports. Pure widget construction: no
//! state, no I/O.

use arch_health_core::{Category, CheckId, CheckReport, Item, Report, Status};
use gtk::glib;
use gtk::prelude::*;

use crate::style;

const MARGIN: i32 = 18;

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

fn section_title(text: &str) -> gtk::Label {
    let l = label(text);
    l.add_css_class("section-title");
    l
}

/// Vertical box inside a scrolled window: the standard page layout.
pub fn scrolled_page() -> (gtk::ScrolledWindow, gtk::Box) {
    let content = gtk::Box::new(gtk::Orientation::Vertical, 10);
    content.set_margin_top(MARGIN);
    content.set_margin_bottom(MARGIN);
    content.set_margin_start(MARGIN);
    content.set_margin_end(MARGIN);
    let scrolled = gtk::ScrolledWindow::builder()
        .hscrollbar_policy(gtk::PolicyType::Never)
        .vexpand(true)
        .child(&content)
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

/// One check: header (glyph, title, summary) and, when there is something to
/// show, an expander with items and limitation. Problems start expanded.
fn report_row(r: &CheckReport) -> gtk::Widget {
    let header = gtk::Box::new(gtk::Orientation::Horizontal, 8);
    header.append(&style::glyph(r.status));
    let title = label(r.id.title());
    title.add_css_class("check-title");
    title.set_width_chars(20);
    title.set_wrap(false);
    header.append(&title);
    let summary = label(&r.summary);
    summary.set_hexpand(true);
    header.append(&summary);

    if r.items.is_empty() && r.limitation.is_none() {
        header.set_margin_start(22); // aligned with expander rows
        return header.upcast();
    }

    let body = gtk::Box::new(gtk::Orientation::Vertical, 4);
    body.set_margin_start(30);
    body.set_margin_top(6);
    body.set_margin_bottom(6);
    if let Some(l) = &r.limitation {
        let note = label(&format!("Incomplete: {l}"));
        note.add_css_class("dim");
        body.append(&note);
    }
    for item in &r.items {
        body.append(&item_row(item));
    }
    gtk::Expander::builder()
        .label_widget(&header)
        .child(&body)
        .expanded(r.status >= Status::Warning)
        .build()
        .upcast()
}

pub fn fill_category(container: &gtk::Box, report: &Report, category: Category) {
    clear(container);
    let title = label(category.title());
    title.add_css_class("headline");
    container.append(&title);
    let list = gtk::Box::new(gtk::Orientation::Vertical, 10);
    list.set_margin_top(6);
    for r in report.by_category(category) {
        list.append(&report_row(r));
        list.append(&gtk::Separator::new(gtk::Orientation::Horizontal));
    }
    container.append(&list);
}

pub fn fill_overview(container: &gtk::Box, report: Option<&Report>, last_run: Option<&str>) {
    clear(container);
    let Some(report) = report else {
        container.append(&label("Running the first check…"));
        return;
    };
    let overall = report.overall();

    let top = gtk::Box::new(gtk::Orientation::Horizontal, 16);
    let big = style::glyph(overall);
    big.add_css_class("big-glyph");
    top.append(&big);
    let text = gtk::Box::new(gtk::Orientation::Vertical, 4);
    text.set_valign(gtk::Align::Center);
    let headline = label(style::headline(overall));
    headline.add_css_class("headline");
    text.append(&headline);
    if let Some(t) = last_run {
        let l = label(&format!("Last check: {t}"));
        l.add_css_class("dim");
        text.append(&l);
    }
    top.append(&text);
    container.append(&top);

    // Everything that needs action, across categories.
    container.append(&section_title("Needs attention"));
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
            container.append(&row);
        }
        for item in problems {
            any = true;
            let row = item_row(item);
            let check = label(&format!("{} ·", r.id.title()));
            check.add_css_class("dim");
            row.insert_child_after(&check, row.first_child().as_ref());
            container.append(&row);
        }
    }
    if !any {
        container.append(&label("Nothing needs attention."));
    }

    container.append(&section_title("Categories"));
    let grid = gtk::Grid::builder()
        .column_spacing(12)
        .row_spacing(6)
        .build();
    for (row, category) in Category::ALL.iter().enumerate() {
        let reports: Vec<&CheckReport> = report.by_category(*category).collect();
        let worst = reports.iter().map(|r| r.status).max().unwrap_or(Status::Ok);
        let problems = reports
            .iter()
            .filter(|r| r.status >= Status::Warning)
            .count();
        grid.attach(&style::glyph(worst), 0, row as i32, 1, 1);
        grid.attach(&label(category.title()), 1, row as i32, 1, 1);
        let detail = match problems {
            0 => format!("{} checks, all fine", reports.len()),
            1 => "1 check needs attention".to_string(),
            n => format!("{n} checks need attention"),
        };
        let d = label(&detail);
        d.add_css_class("dim");
        grid.attach(&d, 2, row as i32, 1, 1);
    }
    container.append(&grid);

    if report.reports.iter().any(|r| r.limitation.is_some()) {
        let note = label(
            "Some checks are incomplete without root privileges. \
             Run `sudo arch-health` in a terminal for a complete report.",
        );
        note.add_css_class("dim");
        note.set_margin_top(12);
        container.append(&note);
    }
}

pub fn history_page() -> gtk::Widget {
    let (scrolled, content) = scrolled_page();
    let title = label("History");
    title.add_css_class("headline");
    content.append(&title);
    content.append(&label(
        "Not available yet. How the status history is stored and shown is still \
         being designed (docs/adr/0010-history.md).",
    ));
    scrolled.upcast()
}

/// Generated from the CLI definition, so it never drifts from `--help`.
pub fn command_line_page() -> gtk::Widget {
    let (scrolled, content) = scrolled_page();
    let title = label("Command line");
    title.add_css_class("headline");
    content.append(&title);
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
