//! Main window: header bar, sidebar with one row per page, stack of pages
//! and a status bar.

use std::collections::HashMap;

use arch_health_core::{Category, Status};
use gtk::prelude::*;

use crate::pages;
use crate::style;

pub const OVERVIEW: &str = "overview";
pub const COMMAND_LINE: &str = "command-line";

pub struct Ui {
    pub window: gtk::ApplicationWindow,
    pub run_button: gtk::Button,
    pub spinner: gtk::Spinner,
    pub status_bar: gtk::Label,
    pub overview: gtk::Box,
    pub categories: HashMap<Category, gtk::Box>,
    /// Status glyph in the sidebar, by page name.
    pub sidebar_glyphs: HashMap<&'static str, gtk::Label>,
    pub sidebar: gtk::ListBox,
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
    let mut sidebar_glyphs = HashMap::new();
    let mut categories = HashMap::new();
    let mut overview = None;

    // Rows follow `pages()`, so a row index is a page index.
    for page in pages() {
        let (name, title, widget, glyph) = match page {
            Page::Overview => {
                let (scrolled, content) = pages::scrolled_page();
                overview = Some(content);
                (OVERVIEW, "Overview", scrolled.upcast(), true)
            }
            Page::Category(c) => {
                let (scrolled, content) = pages::scrolled_page();
                categories.insert(c, content);
                (c.key(), c.title(), scrolled.upcast(), true)
            }
            Page::Separator => {
                sidebar.append(
                    &gtk::ListBoxRow::builder()
                        .child(&gtk::Separator::new(gtk::Orientation::Horizontal))
                        .selectable(false)
                        .activatable(false)
                        .build(),
                );
                continue;
            }
            Page::CommandLine => (
                COMMAND_LINE,
                "Command line",
                pages::command_line_page(),
                false,
            ),
        };
        stack.add_named(&widget, Some(name));
        let g = glyph.then(|| style::glyph(Status::Skipped));
        sidebar.append(&sidebar_row(title, g.as_ref()));
        if let Some(g) = g {
            sidebar_glyphs.insert(name, g);
        }
    }

    let stack_for_sidebar = stack.clone();
    sidebar.connect_row_selected(move |_, row| {
        if let Some(name) = row.and_then(|r| pages().get(r.index() as usize)?.name()) {
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
        overview: overview.expect("pages() starts with the overview"),
        categories,
        sidebar_glyphs,
        sidebar,
    }
}

/// Entries of the sidebar, in order: the single source of truth for page
/// names and row indexes.
#[derive(Clone, Copy)]
enum Page {
    Overview,
    Category(Category),
    Separator,
    CommandLine,
}

impl Page {
    fn name(self) -> Option<&'static str> {
        match self {
            Page::Overview => Some(OVERVIEW),
            Page::Category(c) => Some(c.key()),
            Page::Separator => None,
            Page::CommandLine => Some(COMMAND_LINE),
        }
    }
}

fn pages() -> Vec<Page> {
    let mut v = vec![Page::Overview];
    v.extend(Category::ALL.map(Page::Category));
    v.extend([Page::Separator, Page::CommandLine]);
    v
}

/// Names accepted by `--page`.
pub fn page_names() -> Vec<&'static str> {
    pages().into_iter().filter_map(Page::name).collect()
}

/// Shows the page called `name`, if it exists.
pub fn select_page(sidebar: &gtk::ListBox, name: &str) {
    if let Some(i) = pages().iter().position(|p| p.name() == Some(name)) {
        sidebar.select_row(sidebar.row_at_index(i as i32).as_ref());
    }
}

impl Ui {
    pub fn set_category_glyph(&self, c: Category, s: Status) {
        if let Some(g) = self.sidebar_glyphs.get(c.key()) {
            style::set_glyph(g, s);
        }
    }

    pub fn set_overview_glyph(&self, s: Status) {
        if let Some(g) = self.sidebar_glyphs.get(OVERVIEW) {
            style::set_glyph(g, s);
        }
    }
}
