//! Main window: header bar, sidebar with one row per page, stack of pages
//! and a status bar.

use std::collections::HashMap;

use arch_health_core::{Category, Status};
use gtk::prelude::*;

use crate::pages;
use crate::style;

pub const OVERVIEW: &str = "overview";
pub const HISTORY: &str = "history";
pub const COMMAND_LINE: &str = "command-line";

pub struct Ui {
    pub window: gtk::ApplicationWindow,
    pub run_button: gtk::Button,
    pub spinner: gtk::Spinner,
    pub status_bar: gtk::Label,
    pub overview: gtk::Box,
    pub categories: HashMap<Category, gtk::Box>,
    pub sidebar_glyphs: HashMap<String, gtk::Label>,
}

fn page_key(c: Category) -> String {
    format!("{c:?}").to_lowercase()
}

fn sidebar_row(title: &str, glyph: Option<&gtk::Label>) -> gtk::ListBoxRow {
    let b = gtk::Box::new(gtk::Orientation::Horizontal, 8);
    b.set_margin_top(6);
    b.set_margin_bottom(6);
    b.set_margin_start(10);
    b.set_margin_end(10);
    match glyph {
        Some(g) => b.append(g),
        None => {
            // Keep titles aligned with the rows that have a glyph.
            let spacer = style::glyph(Status::Ok);
            spacer.set_opacity(0.0);
            b.append(&spacer);
        }
    }
    let l = gtk::Label::new(Some(title));
    l.set_xalign(0.0);
    b.append(&l);
    gtk::ListBoxRow::builder().child(&b).build()
}

pub fn build(app: &gtk::Application) -> Ui {
    let window = gtk::ApplicationWindow::builder()
        .application(app)
        .title("arch-health")
        .default_width(1000)
        .default_height(680)
        .build();

    let header = gtk::HeaderBar::new();
    let run_button = gtk::Button::builder()
        .label("Run checks")
        .tooltip_text("Run all checks now")
        .build();
    header.pack_start(&run_button);
    let spinner = gtk::Spinner::new();
    header.pack_start(&spinner);
    window.set_titlebar(Some(&header));

    let stack = gtk::Stack::builder()
        .transition_type(gtk::StackTransitionType::Crossfade)
        .hexpand(true)
        .build();
    let sidebar = gtk::ListBox::builder()
        .selection_mode(gtk::SelectionMode::Browse)
        .build();
    let mut page_names: Vec<String> = Vec::new();
    let mut sidebar_glyphs = HashMap::new();

    // Overview
    let (scrolled, overview) = pages::scrolled_page();
    stack.add_named(&scrolled, Some(OVERVIEW));
    let g = style::glyph(Status::Skipped);
    sidebar.append(&sidebar_row("Overview", Some(&g)));
    sidebar_glyphs.insert(OVERVIEW.to_string(), g);
    page_names.push(OVERVIEW.into());

    // One page per category
    let mut categories = HashMap::new();
    for c in Category::ALL {
        let (scrolled, content) = pages::scrolled_page();
        let key = page_key(c);
        stack.add_named(&scrolled, Some(&key));
        let g = style::glyph(Status::Skipped);
        sidebar.append(&sidebar_row(c.title(), Some(&g)));
        sidebar_glyphs.insert(key.clone(), g);
        categories.insert(c, content);
        page_names.push(key);
    }

    // Static pages, after a separator
    let sep = gtk::ListBoxRow::builder()
        .child(&gtk::Separator::new(gtk::Orientation::Horizontal))
        .selectable(false)
        .activatable(false)
        .build();
    sidebar.append(&sep);
    page_names.push(String::new());
    for (name, title, page) in [
        (HISTORY, "History", pages::history_page()),
        (COMMAND_LINE, "Command line", pages::command_line_page()),
    ] {
        stack.add_named(&page, Some(name));
        sidebar.append(&sidebar_row(title, None));
        page_names.push(name.into());
    }

    let stack_for_sidebar = stack.clone();
    sidebar.connect_row_selected(move |_, row| {
        if let Some(name) = row.and_then(|r| page_names.get(r.index() as usize))
            && !name.is_empty()
        {
            stack_for_sidebar.set_visible_child_name(name);
        }
    });
    sidebar.select_row(sidebar.row_at_index(0).as_ref());

    let sidebar_scroll = gtk::ScrolledWindow::builder()
        .hscrollbar_policy(gtk::PolicyType::Never)
        .child(&sidebar)
        .width_request(200)
        .build();
    let paned = gtk::Paned::builder()
        .orientation(gtk::Orientation::Horizontal)
        .start_child(&sidebar_scroll)
        .end_child(&stack)
        .shrink_start_child(false)
        .resize_start_child(false)
        .position(210)
        .vexpand(true)
        .build();

    let status_bar = gtk::Label::new(Some("Starting…"));
    status_bar.set_xalign(0.0);
    status_bar.add_css_class("dim");
    status_bar.set_margin_start(10);
    status_bar.set_margin_end(10);
    status_bar.set_margin_top(4);
    status_bar.set_margin_bottom(4);

    let root = gtk::Box::new(gtk::Orientation::Vertical, 0);
    root.append(&paned);
    root.append(&gtk::Separator::new(gtk::Orientation::Horizontal));
    root.append(&status_bar);
    window.set_child(Some(&root));

    Ui {
        window,
        run_button,
        spinner,
        status_bar,
        overview,
        categories,
        sidebar_glyphs,
    }
}

impl Ui {
    pub fn set_category_glyph(&self, c: Category, s: Status) {
        if let Some(g) = self.sidebar_glyphs.get(&page_key(c)) {
            style::set_glyph(g, s);
        }
    }

    pub fn set_overview_glyph(&self, s: Status) {
        if let Some(g) = self.sidebar_glyphs.get(OVERVIEW) {
            style::set_glyph(g, s);
        }
    }
}
