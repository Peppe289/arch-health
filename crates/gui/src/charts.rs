//! Charts drawn with cairo on `gtk::DrawingArea`, text with pango.
//! No charting library: three small, purpose-built charts (ADR-0011).

use std::f64::consts::PI;
use std::rc::Rc;
use std::time::SystemTime;

use arch_health_core::{Gauge, Status};
use gtk::prelude::*;
use gtk::{cairo, pango};

use crate::style;
use crate::timeline::{self, Buckets};

type Rgb = (f64, f64, f64);

/// Breeze palette, same as the CSS in `style`.
pub fn rgb(s: Status) -> Rgb {
    match s {
        Status::Ok => (0.153, 0.682, 0.376),       // #27ae60
        Status::Info => (0.239, 0.682, 0.914),     // #3daee9
        Status::Warning => (0.965, 0.455, 0.0),    // #f67400
        Status::Critical => (0.855, 0.267, 0.325), // #da4453
        Status::Unknown => (0.608, 0.349, 0.714),  // #9b59b6
        Status::Skipped => (0.5, 0.5, 0.5),
    }
}

/// Foreground color of the theme, so charts work in light and dark themes.
fn fg(widget: &impl IsA<gtk::Widget>) -> Rgb {
    let c = widget.as_ref().color();
    (c.red() as f64, c.green() as f64, c.blue() as f64)
}

fn set(cr: &cairo::Context, (r, g, b): Rgb, alpha: f64) {
    cr.set_source_rgba(r, g, b, alpha);
}

fn rounded_rect(cr: &cairo::Context, x: f64, y: f64, w: f64, h: f64, r: f64) {
    let r = r.min(w / 2.0).min(h / 2.0);
    cr.new_sub_path();
    cr.arc(x + w - r, y + r, r, -PI / 2.0, 0.0);
    cr.arc(x + w - r, y + h - r, r, 0.0, PI / 2.0);
    cr.arc(x + r, y + h - r, r, PI / 2.0, PI);
    cr.arc(x + r, y + r, r, PI, 1.5 * PI);
    cr.close_path();
}

/// Pango layout with the widget's font (the theme font), not cairo's default.
fn layout(widget: &impl IsA<gtk::Widget>, markup: &str) -> pango::Layout {
    let layout = widget.as_ref().create_pango_layout(None);
    layout.set_markup(markup);
    layout
}

/// Draws `markup` with its top-left at (x, y); returns its pixel size.
fn text(
    widget: &impl IsA<gtk::Widget>,
    cr: &cairo::Context,
    markup: &str,
    x: f64,
    y: f64,
    align_right: bool,
) -> (f64, f64) {
    let layout = layout(widget, markup);
    let (w, h) = layout.pixel_size();
    let x = if align_right { x - w as f64 } else { x };
    cr.move_to(x, y);
    pangocairo::functions::show_layout(cr, &layout);
    (w as f64, h as f64)
}

// ------------------------------------------------------------------- donut

/// Ring chart of how many checks are in each status, total in the middle.
pub fn donut(segments: Vec<(Status, usize)>, size: i32) -> gtk::DrawingArea {
    let area = gtk::DrawingArea::builder()
        .content_width(size)
        .content_height(size)
        .halign(gtk::Align::Center)
        .valign(gtk::Align::Center)
        .build();
    area.set_draw_func(move |a, cr, w, h| {
        let total: usize = segments.iter().map(|s| s.1).sum();
        let (cx, cy) = (w as f64 / 2.0, h as f64 / 2.0);
        let radius = cx.min(cy) - 2.0;
        let thickness = radius * 0.28;
        let r = radius - thickness / 2.0;
        cr.set_line_width(thickness);
        // Track
        set(cr, fg(a), 0.1);
        cr.arc(cx, cy, r, 0.0, 2.0 * PI);
        let _ = cr.stroke();
        if total == 0 {
            return;
        }
        let gap = if segments.iter().filter(|s| s.1 > 0).count() > 1 {
            0.03
        } else {
            0.0
        };
        let mut angle = -PI / 2.0;
        for (status, n) in segments.iter().filter(|s| s.1 > 0) {
            let sweep = 2.0 * PI * *n as f64 / total as f64;
            set(cr, rgb(*status), 1.0);
            cr.arc(cx, cy, r, angle + gap / 2.0, angle + sweep - gap / 2.0);
            let _ = cr.stroke();
            angle += sweep;
        }
        set(cr, fg(a), 1.0);
        let layout = layout(a, &format!(
            "<span size='xx-large' weight='bold'>{total}</span>\n<span size='small'>checks</span>"
        ));
        layout.set_alignment(pango::Alignment::Center);
        let (tw, th) = layout.pixel_size();
        cr.move_to(cx - tw as f64 / 2.0, cy - th as f64 / 2.0);
        pangocairo::functions::show_layout(cr, &layout);
    });
    area
}

/// Legend for the donut: colored dot, status name, count.
pub fn legend(segments: &[(Status, usize)]) -> gtk::Box {
    let b = gtk::Box::new(gtk::Orientation::Vertical, 4);
    b.set_valign(gtk::Align::Center);
    for (status, n) in segments.iter().filter(|s| s.1 > 0) {
        let row = gtk::Box::new(gtk::Orientation::Horizontal, 8);
        let dot = gtk::Label::new(Some("●"));
        dot.add_css_class(style::class(*status));
        row.append(&dot);
        let l = gtk::Label::new(Some(&format!("{n} {}", style::status_name(*status))));
        l.set_xalign(0.0);
        row.append(&l);
        b.append(&row);
    }
    b
}

// ------------------------------------------------------------------- gauge

/// Labelled horizontal bar: label and text on top, bar below.
pub fn gauge(g: &Gauge) -> gtk::Box {
    let b = gtk::Box::new(gtk::Orientation::Vertical, 4);
    let top = gtk::Box::new(gtk::Orientation::Horizontal, 8);
    let label = gtk::Label::new(Some(&g.label));
    label.add_css_class("check-title");
    label.set_xalign(0.0);
    label.set_hexpand(true);
    label.set_ellipsize(pango::EllipsizeMode::Middle);
    top.append(&label);
    let text = gtk::Label::new(Some(&g.text));
    text.add_css_class("dim");
    top.append(&text);
    b.append(&top);

    let fraction = g.fraction.clamp(0.0, 1.0);
    let status = g.status;
    let bar = gtk::DrawingArea::builder()
        .content_height(12)
        .hexpand(true)
        .build();
    bar.set_draw_func(move |a, cr, w, h| {
        let (w, h) = (w as f64, h as f64);
        set(cr, fg(a), 0.12);
        rounded_rect(cr, 0.0, 0.0, w, h, h / 2.0);
        let _ = cr.fill();
        if fraction > 0.0 {
            set(
                cr,
                rgb(if status == Status::Ok {
                    Status::Info
                } else {
                    status
                }),
                1.0,
            );
            rounded_rect(cr, 0.0, 0.0, (w * fraction).max(h), h, h / 2.0);
            let _ = cr.fill();
        }
    });
    b.append(&bar);
    b
}

// --------------------------------------------------------------- histogram

const AXIS_W: f64 = 34.0;
const AXIS_H: f64 = 20.0;

/// Stacked bars of events per time slot (critical, warning, other), with a
/// tooltip per bar. `None` when there are no events.
pub fn histogram(events: &[(SystemTime, Status)]) -> Option<gtk::DrawingArea> {
    let buckets = Rc::new(timeline::bucketize(
        events,
        SystemTime::now(),
        timeline::local_offset(),
    )?);
    let area = gtk::DrawingArea::builder()
        .content_height(130)
        .hexpand(true)
        .build();

    let b = buckets.clone();
    area.set_draw_func(move |a, cr, w, h| draw_histogram(a, cr, w as f64, h as f64, &b));

    area.set_has_tooltip(true);
    let b = buckets.clone();
    area.connect_query_tooltip(move |a, x, _y, _kb, tooltip| {
        let plot_w = a.width() as f64 - AXIS_W;
        let n = b.counts.len();
        if plot_w <= 0.0 || n == 0 || (x as f64) < AXIS_W {
            return false;
        }
        let i = (((x as f64 - AXIS_W) / plot_w) * n as f64) as usize;
        if i >= n {
            return false;
        }
        tooltip.set_text(Some(&timeline::describe_bucket(&b, i)));
        true
    });
    Some(area)
}

fn draw_histogram(a: &gtk::DrawingArea, cr: &cairo::Context, w: f64, h: f64, b: &Buckets) {
    let fg = fg(a);
    let n = b.counts.len();
    let max = b.max_total().max(1);
    let (plot_x, plot_w, plot_h) = (AXIS_W, w - AXIS_W, h - AXIS_H);

    // Y axis: max value and baseline
    set(cr, fg, 0.6);
    text(
        a,
        cr,
        &format!("<small>{max}</small>"),
        AXIS_W - 6.0,
        0.0,
        true,
    );
    text(a, cr, "<small>0</small>", AXIS_W - 6.0, plot_h - 12.0, true);
    set(cr, fg, 0.15);
    cr.set_line_width(1.0);
    cr.move_to(plot_x, plot_h + 0.5);
    cr.line_to(w, plot_h + 0.5);
    cr.move_to(plot_x, 0.5);
    cr.line_to(w, 0.5);
    let _ = cr.stroke();

    // Bars
    let slot = plot_w / n as f64;
    let bar_w = (slot * 0.75).max(1.0);
    for (i, c) in b.counts.iter().enumerate() {
        let x = plot_x + i as f64 * slot + (slot - bar_w) / 2.0;
        let mut y = plot_h;
        for (count, status) in [
            (c.other, Status::Info),
            (c.warning, Status::Warning),
            (c.critical, Status::Critical),
        ] {
            if count == 0 {
                continue;
            }
            let bh = plot_h * count as f64 / max as f64;
            y -= bh;
            set(cr, rgb(status), 1.0);
            cr.rectangle(x, y, bar_w, bh);
            let _ = cr.fill();
        }
    }

    // X axis: first, middle and last slot
    set(cr, fg, 0.6);
    let ticks = if n > 2 {
        vec![0, n / 2, n - 1]
    } else {
        (0..n).collect()
    };
    for i in ticks {
        let label = timeline::format_tick(b.bucket_start(i), b.step);
        let x = plot_x + i as f64 * slot;
        let right = i == n - 1 && n > 1;
        let x = if right { x + slot } else { x };
        text(
            a,
            cr,
            &format!("<small>{label}</small>"),
            x,
            plot_h + 3.0,
            right,
        );
    }
}
