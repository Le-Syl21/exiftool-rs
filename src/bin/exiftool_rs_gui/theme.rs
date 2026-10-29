//! The GUI's colors, for the dark and the light theme, and the WCAG
//! contrast ratio they are checked with (tests at the end: every text
//! color against every background it is drawn on, AA or better).

use eframe::egui::{self, Color32, Visuals};

/// The colors of the tag table and the bars, for one theme.
#[derive(Clone, Copy, Debug)]
pub struct Palette {
    /// Tag descriptions (the first column)
    pub label: Color32,
    /// A read-only value
    pub value: Color32,
    /// An editable value, and its pencil mark
    pub writable: Color32,
    /// A value with an edit waiting to be saved, and its mark
    pub modified: Color32,
    /// Secondary text: group counts, the tag name in the edit dialog
    pub muted: Color32,
    /// Feedback after a copy or a save
    pub success: Color32,
    /// The border of the text field (a UI component: 3:1)
    pub border: Color32,
}

impl Palette {
    pub const DARK: Palette = Palette {
        label: Color32::from_rgb(160, 170, 184),
        value: Color32::from_gray(232),
        writable: Color32::from_rgb(130, 188, 255),
        modified: Color32::from_rgb(255, 200, 92),
        muted: Color32::from_gray(160),
        success: Color32::from_rgb(112, 212, 132),
        border: Color32::from_gray(130),
    };

    pub const LIGHT: Palette = Palette {
        label: Color32::from_rgb(72, 82, 96),
        value: Color32::from_gray(24),
        writable: Color32::from_rgb(0, 84, 168),
        modified: Color32::from_rgb(132, 68, 0),
        muted: Color32::from_gray(92),
        success: Color32::from_rgb(18, 112, 44),
        border: Color32::from_gray(118),
    };

    pub fn of(visuals: &Visuals) -> Palette {
        if visuals.dark_mode {
            Self::DARK
        } else {
            Self::LIGHT
        }
    }
}

/// egui's own visuals, with the few colors that miss AA contrast on their
/// background brought up to it (light-theme links, warnings and errors,
/// dark-theme errors), and the normal text a little stronger.
pub fn visuals(dark: bool) -> Visuals {
    let mut v = if dark { Visuals::dark() } else { Visuals::light() };
    let pal = if dark { Palette::DARK } else { Palette::LIGHT };
    if dark {
        v.widgets.noninteractive.fg_stroke.color = Color32::from_gray(200);
        v.hyperlink_color = Color32::from_rgb(110, 180, 255);
        v.error_fg_color = Color32::from_rgb(255, 120, 110);
    } else {
        v.widgets.noninteractive.fg_stroke.color = Color32::from_gray(40);
        v.widgets.inactive.fg_stroke.color = Color32::from_gray(30);
        v.hyperlink_color = Color32::from_rgb(0, 88, 176);
        v.warn_fg_color = Color32::from_rgb(150, 70, 0);
        v.error_fg_color = Color32::from_rgb(180, 0, 0);
        // the stripes and the button borders, a shade darker so that they
        // are seen on a light screen
        v.faint_bg_color = Color32::from_gray(238);
    }
    v.weak_text_color = Some(pal.muted);
    v
}

/// Applies the GUI's visuals to both of egui's styles (the theme preference
/// then picks one, the system's by default).
pub fn install(ctx: &egui::Context) {
    ctx.style_mut_of(egui::Theme::Dark, |s| s.visuals = visuals(true));
    ctx.style_mut_of(egui::Theme::Light, |s| s.visuals = visuals(false));
}

/// The background a striped row is painted with.
pub fn stripe(v: &Visuals) -> Color32 {
    if v.dark_mode {
        // egui's additive "faint" color, over the panel
        let p = v.panel_fill;
        let f = v.faint_bg_color;
        Color32::from_rgb(
            p.r().saturating_add(f.r()),
            p.g().saturating_add(f.g()),
            p.b().saturating_add(f.b()),
        )
    } else {
        v.faint_bg_color
    }
}

/// WCAG 2 relative luminance of an sRGB color.
#[cfg(test)]
pub fn luminance(c: Color32) -> f64 {
    let lin = |u: u8| {
        let s = u as f64 / 255.0;
        if s <= 0.04045 {
            s / 12.92
        } else {
            ((s + 0.055) / 1.055).powf(2.4)
        }
    };
    0.2126 * lin(c.r()) + 0.7152 * lin(c.g()) + 0.0722 * lin(c.b())
}

/// WCAG 2 contrast ratio of two opaque colors, from 1 to 21.
#[cfg(test)]
pub fn contrast(a: Color32, b: Color32) -> f64 {
    let (la, lb) = (luminance(a), luminance(b));
    let (hi, lo) = if la > lb { (la, lb) } else { (lb, la) };
    (hi + 0.05) / (lo + 0.05)
}

/// Every text color of a theme with every background it is drawn on, and
/// the ratio WCAG AA asks of the pair (4.5 for text, 3 for UI components
/// such as the focus ring).
#[cfg(test)]
pub fn pairs(dark: bool) -> Vec<(&'static str, Color32, &'static str, Color32, f64)> {
    let v = visuals(dark);
    let p = if dark { Palette::DARK } else { Palette::LIGHT };
    let panel = v.panel_fill;
    let stripe = stripe(&v);
    let hover = v.widgets.hovered.weak_bg_fill;
    let window = v.window_fill;
    let edit = v.text_edit_bg_color();
    let button = v.widgets.inactive.weak_bg_fill;
    let mut out = Vec::new();
    for (bg_name, bg) in [("panel", panel), ("stripe", stripe)] {
        out.push(("label", p.label, bg_name, bg, 4.5));
        out.push(("value", p.value, bg_name, bg, 4.5));
        out.push(("writable", p.writable, bg_name, bg, 4.5));
        out.push(("modified", p.modified, bg_name, bg, 4.5));
        out.push(("muted", p.muted, bg_name, bg, 4.5));
        out.push(("success", p.success, bg_name, bg, 4.5));
        out.push(("text", v.text_color(), bg_name, bg, 4.5));
        out.push(("focus ring", v.selection.stroke.color, bg_name, bg, 3.0));
    }
    out.push(("writable", p.writable, "hovered value", hover, 4.5));
    out.push(("modified", p.modified, "hovered value", hover, 4.5));
    out.push(("value", p.value, "hovered value", hover, 4.5));
    out.push(("text", v.text_color(), "window", window, 4.5));
    out.push(("muted", p.muted, "window", window, 4.5));
    out.push(("link", v.hyperlink_color, "window", window, 4.5));
    out.push(("error", v.error_fg_color, "panel", panel, 4.5));
    out.push(("warning", v.warn_fg_color, "panel", panel, 4.5));
    out.push(("text field", v.text_color(), "text field", edit, 4.5));
    out.push(("text field border", p.border, "window", window, 3.0));
    out.push(("text field border", p.border, "text field", edit, 3.0));
    out.push(("button text", v.widgets.inactive.fg_stroke.color, "button", button, 4.5));
    out.push(("hovered button text", v.widgets.hovered.fg_stroke.color, "hovered button", hover, 4.5));
    out.push(("selected text", v.selection.stroke.color, "selection", v.selection.bg_fill, 4.5));
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_ratio_is_wcags() {
        assert!((contrast(Color32::BLACK, Color32::WHITE) - 21.0).abs() < 1e-9);
        assert!((contrast(Color32::WHITE, Color32::WHITE) - 1.0).abs() < 1e-9);
        // #777 on white: the classic 4.48, just below AA
        let r = contrast(Color32::from_gray(0x77), Color32::WHITE);
        assert!((r - 4.48).abs() < 0.01, "{r}");
    }

    #[test]
    fn every_text_is_aa_on_its_backgrounds_in_both_themes() {
        let mut failures = Vec::new();
        for dark in [true, false] {
            for (fg_name, fg, bg_name, bg, min) in pairs(dark) {
                let r = contrast(fg, bg);
                if r < min {
                    failures.push(format!(
                        "{} {fg_name} on {bg_name}: {r:.2} < {min}",
                        if dark { "dark" } else { "light" }
                    ));
                }
            }
        }
        assert!(failures.is_empty(), "{failures:#?}");
    }

    #[test]
    fn the_old_dark_only_colors_failed_in_light_mode() {
        // what the table used before: green, red and light blue text,
        // fine on a dark panel and unreadable on a light one
        let light = Visuals::light().panel_fill;
        for old in [
            Color32::from_rgb(100, 200, 100),
            Color32::from_rgb(200, 100, 100),
            Color32::LIGHT_BLUE,
            Color32::YELLOW,
        ] {
            assert!(contrast(old, light) < 4.5, "{old:?}");
        }
    }

    /// Prints the table of the report: `cargo test ... contrast_table -- --nocapture`
    #[test]
    fn contrast_table() {
        for dark in [true, false] {
            for (fg_name, fg, bg_name, bg, _) in pairs(dark) {
                println!(
                    "{}\t{fg_name}\t#{:02x}{:02x}{:02x}\t{bg_name}\t#{:02x}{:02x}{:02x}\t{:.2}",
                    if dark { "dark" } else { "light" },
                    fg.r(),
                    fg.g(),
                    fg.b(),
                    bg.r(),
                    bg.g(),
                    bg.b(),
                    contrast(fg, bg)
                );
            }
        }
    }
}
