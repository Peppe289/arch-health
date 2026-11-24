//! Status glyphs and colors, shared by every page. Colors follow the KDE
//! Breeze palette (positive/neutral/negative/information).

use arch_health_core::Status;
use gtk::prelude::*;

pub const CSS: &str = "
.glyph { font-weight: bold; min-width: 1.4em; }
.big-glyph { font-size: 40px; font-weight: bold; }
.headline { font-size: 18px; font-weight: bold; }
.status-ok { color: #27ae60; }
.status-info { color: #3daee9; }
.status-warning { color: #f67400; }
.status-critical { color: #da4453; }
.status-unknown { color: #9b59b6; }
.status-skipped { opacity: 0.5; }
.dim { opacity: 0.7; }
.section-title { font-weight: bold; margin-top: 12px; }
.check-title { font-weight: bold; }
";

pub fn install() {
    let provider = gtk::CssProvider::new();
    provider.load_from_string(CSS);
    if let Some(display) = gtk::gdk::Display::default() {
        gtk::style_context_add_provider_for_display(
            &display,
            &provider,
            gtk::STYLE_PROVIDER_PRIORITY_APPLICATION,
        );
    }
}

pub fn glyph_text(s: Status) -> &'static str {
    match s {
        Status::Ok => "✔",
        Status::Skipped => "–",
        Status::Info => "ℹ",
        Status::Warning => "⚠",
        Status::Unknown => "?",
        Status::Critical => "✘",
    }
}

pub fn class(s: Status) -> &'static str {
    match s {
        Status::Ok => "status-ok",
        Status::Skipped => "status-skipped",
        Status::Info => "status-info",
        Status::Warning => "status-warning",
        Status::Unknown => "status-unknown",
        Status::Critical => "status-critical",
    }
}

const ALL_CLASSES: [&str; 6] = [
    "status-ok",
    "status-skipped",
    "status-info",
    "status-warning",
    "status-unknown",
    "status-critical",
];

/// A label showing the glyph of `s`, colored.
pub fn glyph(s: Status) -> gtk::Label {
    let l = gtk::Label::new(None);
    l.add_css_class("glyph");
    set_glyph(&l, s);
    l
}

/// Updates an existing glyph label to `s`.
pub fn set_glyph(label: &gtk::Label, s: Status) {
    label.set_text(glyph_text(s));
    for c in ALL_CLASSES {
        label.remove_css_class(c);
    }
    label.add_css_class(class(s));
}

/// Short sentence for the overall status.
pub fn headline(s: Status) -> &'static str {
    match s {
        Status::Ok | Status::Skipped | Status::Info => "System is healthy",
        Status::Warning | Status::Unknown => "There are warnings to look at",
        Status::Critical => "There are critical problems",
    }
}

/// Freedesktop icon names (present in Breeze and Adwaita) for the tray.
pub fn tray_icon(s: Status) -> &'static str {
    match s {
        Status::Ok | Status::Skipped | Status::Info => "security-high",
        Status::Warning | Status::Unknown => "security-medium",
        Status::Critical => "security-low",
    }
}
