//! Sortable, filterable table of timed events (time, level, source,
//! message), built on `gtk::ColumnView`.

use std::cell::RefCell;
use std::cmp::Ordering;
use std::rc::Rc;

use arch_health_core::{Status, TimedEvent};
use gtk::prelude::*;
use gtk::{gio, glib, pango};

use crate::style;
use crate::timeline;

/// One table row: an event and, on the overview, the check it comes from.
#[derive(Clone)]
pub struct Row {
    pub check: Option<&'static str>,
    pub event: TimedEvent,
}

fn row_of(obj: &glib::Object) -> std::cell::Ref<'_, Row> {
    obj.downcast_ref::<glib::BoxedAnyObject>()
        .expect("table items are BoxedAnyObject")
        .borrow::<Row>()
}

/// `width`: initial width in pixels (`None` for the expanding column).
fn label_column(
    title: &str,
    width: Option<i32>,
    text: impl Fn(&Row) -> String + 'static,
    sort: Option<fn(&Row, &Row) -> Ordering>,
) -> gtk::ColumnViewColumn {
    let factory = gtk::SignalListItemFactory::new();
    factory.connect_setup(|_, item| {
        let label = gtk::Label::new(None);
        label.set_xalign(0.0);
        label.set_margin_start(6);
        label.set_margin_end(6);
        label.set_ellipsize(pango::EllipsizeMode::End);
        label.set_selectable(true);
        item.downcast_ref::<gtk::ListItem>()
            .expect("list item")
            .set_child(Some(&label));
    });
    factory.connect_bind(move |_, item| {
        let item = item.downcast_ref::<gtk::ListItem>().expect("list item");
        let (Some(obj), Some(label)) = (item.item(), item.child().and_downcast::<gtk::Label>())
        else {
            return;
        };
        let t = text(&row_of(&obj));
        label.set_tooltip_text(Some(&t));
        label.set_text(&t);
    });
    let col = gtk::ColumnViewColumn::new(Some(title), Some(factory));
    match width {
        Some(w) => col.set_fixed_width(w),
        None => col.set_expand(true),
    }
    col.set_resizable(true);
    if let Some(cmp) = sort {
        col.set_sorter(Some(&gtk::CustomSorter::new(move |a, b| {
            cmp(&row_of(a), &row_of(b)).into()
        })));
    }
    col
}

fn level_column() -> gtk::ColumnViewColumn {
    let factory = gtk::SignalListItemFactory::new();
    factory.connect_setup(|_, item| {
        let b = gtk::Box::new(gtk::Orientation::Horizontal, 6);
        b.set_margin_start(6);
        b.set_margin_end(6);
        b.append(&style::glyph(Status::Ok));
        b.append(&gtk::Label::new(None));
        item.downcast_ref::<gtk::ListItem>()
            .expect("list item")
            .set_child(Some(&b));
    });
    factory.connect_bind(|_, item| {
        let item = item.downcast_ref::<gtk::ListItem>().expect("list item");
        let (Some(obj), Some(b)) = (item.item(), item.child().and_downcast::<gtk::Box>()) else {
            return;
        };
        let status = row_of(&obj).event.status;
        if let Some(g) = b.first_child().and_downcast::<gtk::Label>() {
            style::set_glyph(&g, status);
        }
        if let Some(l) = b.last_child().and_downcast::<gtk::Label>() {
            l.set_text(status.name());
        }
    });
    let col = gtk::ColumnViewColumn::new(Some("Level"), Some(factory));
    col.set_fixed_width(110);
    col.set_resizable(true);
    col.set_sorter(Some(&gtk::CustomSorter::new(|a, b| {
        row_of(a).event.status.cmp(&row_of(b).event.status).into()
    })));
    col
}

/// Search field + table, newest events first.
pub fn events_table(rows: Vec<Row>, show_check: bool) -> gtk::Box {
    let store = gio::ListStore::new::<glib::BoxedAnyObject>();
    for r in rows {
        store.append(&glib::BoxedAnyObject::new(r));
    }
    let count = store.n_items();

    let query = Rc::new(RefCell::new(String::new()));
    let q = query.clone();
    let filter = gtk::CustomFilter::new(move |obj| {
        let q = q.borrow();
        if q.is_empty() {
            return true;
        }
        let r = row_of(obj);
        r.event.source.to_lowercase().contains(q.as_str())
            || r.event.message.to_lowercase().contains(q.as_str())
            || r.check
                .is_some_and(|c| c.to_lowercase().contains(q.as_str()))
    });
    let filtered = gtk::FilterListModel::new(Some(store), Some(filter.clone()));
    let sorted = gtk::SortListModel::new(Some(filtered), None::<gtk::Sorter>);
    let view = gtk::ColumnView::new(Some(gtk::NoSelection::new(Some(sorted.clone()))));
    view.set_show_row_separators(true);
    view.set_reorderable(false);
    sorted.set_sorter(view.sorter().as_ref());

    let time = label_column(
        "Time",
        Some(150),
        |r| timeline::format_time(r.event.time),
        Some(|a, b| a.event.time.cmp(&b.event.time)),
    );
    view.append_column(&time);
    view.append_column(&level_column());
    if show_check {
        view.append_column(&label_column(
            "Check",
            Some(140),
            |r| r.check.unwrap_or_default().to_string(),
            Some(|a, b| a.check.cmp(&b.check)),
        ));
    }
    view.append_column(&label_column(
        "Source",
        Some(200),
        |r| r.event.source.clone(),
        Some(|a, b| a.event.source.cmp(&b.event.source)),
    ));
    view.append_column(&label_column(
        "Message",
        None,
        |r| r.event.message.clone(),
        None,
    ));
    view.sort_by_column(Some(&time), gtk::SortType::Descending);

    let search = gtk::SearchEntry::builder()
        .placeholder_text(format!("Filter {count} events by source or message"))
        .build();
    search.connect_search_changed(move |s| {
        *query.borrow_mut() = s.text().to_lowercase();
        filter.changed(gtk::FilterChange::Different);
    });

    let scrolled = gtk::ScrolledWindow::builder()
        .child(&view)
        .min_content_height(280)
        .max_content_height(420)
        .propagate_natural_height(true)
        .hscrollbar_policy(gtk::PolicyType::Automatic)
        .build();
    let frame = gtk::Frame::builder().child(&scrolled).build();

    let b = gtk::Box::new(gtk::Orientation::Vertical, 6);
    b.append(&search);
    b.append(&frame);
    b
}
