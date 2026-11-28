//! Solus Interactive look: dark-first, gold used sparingly as an accent.

use eframe::egui::{self, Color32, CornerRadius, FontData, FontDefinitions, FontFamily, FontId, Stroke};
use std::sync::Arc;

pub const GOLD: Color32 = Color32::from_rgb(0xFF, 0xAE, 0x00); // Solus Gold
pub const GOLD_PALE: Color32 = Color32::from_rgb(0xFF, 0xD2, 0x7A); // Pale Gold
pub const CHARCOAL: Color32 = Color32::from_rgb(0x23, 0x21, 0x20); // surfaces
pub const GROUND: Color32 = Color32::from_rgb(0x18, 0x16, 0x15); // page background

pub const BORDER: Color32 = Color32::from_rgb(0x34, 0x30, 0x2d);
pub const TEXT: Color32 = Color32::from_rgb(0xec, 0xe6, 0xdc);
pub const MUTED: Color32 = Color32::from_rgb(0x9a, 0x92, 0x8a);
pub const MISSING: Color32 = Color32::from_rgb(0x8f, 0x85, 0x7c);
pub const MISSING_WARN: Color32 = Color32::from_rgb(0xc9, 0x7a, 0x6a);
pub const SELECT: Color32 = Color32::from_rgb(0xd8, 0xd2, 0xc8);

pub const CARD_OWNED: Color32 = Color32::from_rgb(0x2a, 0x26, 0x21);
pub const SLOT: Color32 = Color32::from_rgb(0x1d, 0x1b, 0x1a);
pub const SLOT_BORDER: Color32 = Color32::from_rgb(0x3a, 0x36, 0x33);
pub const SLOT_LIT: Color32 = Color32::from_rgb(0x2b, 0x27, 0x20);
pub const SLOT_GOLD: Color32 = Color32::from_rgb(0x33, 0x29, 0x1a);

pub fn pale_gold_a(alpha: u8) -> Color32 {
    Color32::from_rgba_unmultiplied(0xFF, 0xD2, 0x7A, alpha)
}

pub fn bold() -> FontFamily {
    FontFamily::Name("bold".into())
}
pub fn font(size: f32) -> FontId {
    FontId::new(size, FontFamily::Proportional)
}
pub fn font_bold(size: f32) -> FontId {
    FontId::new(size, bold())
}

pub fn install(ctx: &egui::Context) {
    // Fonts: Inter (SIL Open Font License, see assets/fonts/OFL.txt), egui's fonts as fallback for symbols.
    let mut fonts = FontDefinitions::default();
    fonts.font_data.insert(
        "inter".into(),
        Arc::new(FontData::from_static(include_bytes!("../assets/fonts/Inter-Regular.otf"))),
    );
    fonts.font_data.insert(
        "inter-bold".into(),
        Arc::new(FontData::from_static(include_bytes!("../assets/fonts/Inter-Bold.otf"))),
    );
    let fallback = fonts.families.get(&FontFamily::Proportional).cloned().unwrap_or_default();
    fonts.families.entry(FontFamily::Proportional).or_default().insert(0, "inter".into());
    let mut bold_family = vec!["inter-bold".to_string()];
    bold_family.extend(fallback);
    fonts.families.insert(bold(), bold_family);
    ctx.set_fonts(fonts);

    ctx.global_style_mut(|style| {
        style.text_styles.insert(egui::TextStyle::Body, font(13.5));
        style.text_styles.insert(egui::TextStyle::Button, font(13.5));
        style.text_styles.insert(egui::TextStyle::Small, font(11.0));
        style.spacing.item_spacing = egui::vec2(8.0, 6.0);
        style.spacing.button_padding = egui::vec2(10.0, 5.0);
        style.spacing.interact_size.y = 26.0;

        let v = &mut style.visuals;
        *v = egui::Visuals::dark();
        v.panel_fill = GROUND;
        v.window_fill = CHARCOAL;
        v.window_stroke = Stroke::new(1.0, BORDER);
        v.extreme_bg_color = CHARCOAL; // text edits
        v.faint_bg_color = CHARCOAL;
        // No override_text_color: selected items (gold background) need their own dark text.
        v.hyperlink_color = GOLD_PALE;
        v.selection.bg_fill = GOLD;
        v.selection.stroke = Stroke::new(1.0, GROUND);
        v.text_cursor.stroke = Stroke::new(2.0, GOLD_PALE);
        let r = CornerRadius::same(6);
        v.window_corner_radius = CornerRadius::same(10);
        v.menu_corner_radius = CornerRadius::same(6);
        for w in [
            &mut v.widgets.noninteractive,
            &mut v.widgets.inactive,
            &mut v.widgets.hovered,
            &mut v.widgets.active,
            &mut v.widgets.open,
        ] {
            w.corner_radius = r;
            w.bg_fill = CHARCOAL;
            w.weak_bg_fill = CHARCOAL;
            w.bg_stroke = Stroke::new(1.0, BORDER);
            w.fg_stroke = Stroke::new(1.0, TEXT);
        }
        // Checkbox boxes use bg_fill: keep them darker than the panels so they're visible.
        v.widgets.inactive.bg_fill = GROUND;
        v.widgets.inactive.bg_stroke = Stroke::new(1.0, Color32::from_rgb(0x4a, 0x44, 0x3e));
        v.widgets.hovered.bg_fill = GROUND;
        v.widgets.active.bg_fill = GROUND;
        v.widgets.noninteractive.bg_fill = GROUND;
        v.widgets.noninteractive.bg_stroke = Stroke::new(1.0, BORDER);
        v.widgets.hovered.weak_bg_fill = BORDER; // visible hover in menus and dropdowns
        v.widgets.hovered.bg_stroke = Stroke::new(1.0, GOLD_PALE);
        v.widgets.hovered.fg_stroke = Stroke::new(1.5, GOLD_PALE);
        v.widgets.active.bg_stroke = Stroke::new(1.0, GOLD);
        v.widgets.active.fg_stroke = Stroke::new(1.5, GOLD);
        v.widgets.open.bg_stroke = Stroke::new(1.0, GOLD_PALE);
    });
}

// ----------------------------------------------------------------------------- card style
/// The shape of cards, slots, buttons and panels. Chosen in Preferences, saved in settings.json.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum CardStyle {
    Rounded,
    Chamfered,
    Clipped,
    Machined,
}

impl CardStyle {
    pub const ALL: [CardStyle; 4] = [CardStyle::Rounded, CardStyle::Chamfered, CardStyle::Clipped, CardStyle::Machined];

    pub fn label(self) -> &'static str {
        match self {
            CardStyle::Rounded => "Rounded",
            CardStyle::Chamfered => "Chamfered",
            CardStyle::Clipped => "Clipped",
            CardStyle::Machined => "Machined",
        }
    }
    pub fn blurb(self) -> &'static str {
        match self {
            CardStyle::Rounded => "Original",
            CardStyle::Chamfered => "Default",
            CardStyle::Clipped => "Two corners",
            CardStyle::Machined => "Chamfer + bevel",
        }
    }
    /// Name in settings.json.
    pub fn key(self) -> &'static str {
        match self {
            CardStyle::Rounded => "rounded",
            CardStyle::Chamfered => "chamfered",
            CardStyle::Clipped => "clipped",
            CardStyle::Machined => "machined",
        }
    }
    pub fn from_key(s: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|c| c.key() == s)
    }
    pub fn industrial(self) -> bool {
        self != CardStyle::Rounded
    }
}

static STYLE: std::sync::atomic::AtomicU8 = std::sync::atomic::AtomicU8::new(1); // Chamfered

pub fn card_style() -> CardStyle {
    CardStyle::ALL[STYLE.load(std::sync::atomic::Ordering::Relaxed) as usize % 4]
}

/// Switches the style and squares off egui's own widgets (dropdowns, text boxes, dialogs) to match.
pub fn set_card_style(ctx: &egui::Context, style: CardStyle) {
    let i = CardStyle::ALL.iter().position(|c| *c == style).unwrap_or(1) as u8;
    STYLE.store(i, std::sync::atomic::Ordering::Relaxed);
    let (widget, window) = if style.industrial() { (1, 2) } else { (6, 10) };
    ctx.global_style_mut(|s| {
        let v = &mut s.visuals;
        v.window_corner_radius = CornerRadius::same(window);
        v.menu_corner_radius = CornerRadius::same(widget);
        for w in [
            &mut v.widgets.noninteractive,
            &mut v.widgets.inactive,
            &mut v.widgets.hovered,
            &mut v.widgets.active,
            &mut v.widgets.open,
        ] {
            w.corner_radius = CornerRadius::same(widget);
        }
    });
    ctx.request_repaint();
}

/// Corner radius for egui Frames (banners, note boxes) in the current style.
pub fn frame_radius(rounded: u8) -> u8 {
    if card_style().industrial() { 1 } else { rounded }
}

/// Outline of `r` in `style`: `radius` is used for Rounded, `cut` (the 45° corner) for the others.
fn outline(style: CardStyle, r: egui::Rect, cut: f32) -> Vec<egui::Pos2> {
    use egui::pos2;
    let k = cut.min(r.width() / 2.0).min(r.height() / 2.0);
    let (l, t, rt, b) = (r.left(), r.top(), r.right(), r.bottom());
    match style {
        CardStyle::Clipped => vec![pos2(l + k, t), pos2(rt, t), pos2(rt, b - k), pos2(rt - k, b), pos2(l, b), pos2(l, t + k)],
        _ => vec![
            pos2(l + k, t),
            pos2(rt - k, t),
            pos2(rt, t + k),
            pos2(rt, b - k),
            pos2(rt - k, b),
            pos2(l + k, b),
            pos2(l, b - k),
            pos2(l, t + k),
        ],
    }
}

/// Fills a panel shape in the given style.
pub fn shape_fill_as(style: CardStyle, p: &egui::Painter, r: egui::Rect, radius: f32, cut: f32, fill: Color32) {
    if style == CardStyle::Rounded {
        p.rect_filled(r, radius, fill);
        return;
    }
    p.add(egui::Shape::convex_polygon(outline(style, r, cut), fill, Stroke::NONE));
    if style == CardStyle::Machined && r.height() > 30.0 {
        // faint machined highlight along the top edge
        let y = r.top() + 1.5;
        p.line_segment(
            [egui::pos2(r.left() + cut, y), egui::pos2(r.right() - cut, y)],
            Stroke::new(1.0, Color32::from_white_alpha(16)),
        );
    }
}

/// Draws a panel's border inside `r` in the given style.
pub fn shape_stroke_as(style: CardStyle, p: &egui::Painter, r: egui::Rect, radius: f32, cut: f32, stroke: Stroke) {
    if style == CardStyle::Rounded {
        p.rect_stroke(r, radius, stroke, egui::StrokeKind::Inside);
        return;
    }
    let h = stroke.width / 2.0;
    // Shrinking the rect pulls the 45° edges in by h·√2 − h; trim the cut so the border stays parallel.
    let pts = outline(style, r.shrink(h), (cut - h * (std::f32::consts::SQRT_2 - 1.0)).max(0.0));
    p.add(egui::Shape::closed_line(pts, stroke));
}

pub fn shape_fill(p: &egui::Painter, r: egui::Rect, radius: f32, cut: f32, fill: Color32) {
    shape_fill_as(card_style(), p, r, radius, cut, fill);
}
pub fn shape_stroke(p: &egui::Painter, r: egui::Rect, radius: f32, cut: f32, stroke: Stroke) {
    shape_stroke_as(card_style(), p, r, radius, cut, stroke);
}
/// Fill + border in the current style.
pub fn shape(p: &egui::Painter, r: egui::Rect, radius: f32, cut: f32, fill: Color32, stroke: Stroke) {
    shape_fill(p, r, radius, cut, fill);
    shape_stroke(p, r, radius, cut, stroke);
}

/// Card-sized cut and small-part cut, matching the mockup.
pub const CUT: f32 = 12.0;
pub const CUT_SMALL: f32 = 7.0;

// ----------------------------------------------------------------------------- animations
static ANIMATIONS: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(true);

/// The gold sweep on cards with running jobs. Turned off in Preferences.
pub fn animations() -> bool {
    ANIMATIONS.load(std::sync::atomic::Ordering::Relaxed)
}
pub fn set_animations(on: bool) {
    ANIMATIONS.store(on, std::sync::atomic::Ordering::Relaxed);
}
