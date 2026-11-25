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
        v.override_text_color = Some(TEXT);
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
        v.widgets.hovered.bg_stroke = Stroke::new(1.0, GOLD_PALE);
        v.widgets.hovered.fg_stroke = Stroke::new(1.5, GOLD_PALE);
        v.widgets.active.bg_stroke = Stroke::new(1.0, GOLD);
        v.widgets.active.fg_stroke = Stroke::new(1.5, GOLD);
        v.widgets.open.bg_stroke = Stroke::new(1.0, GOLD_PALE);
    });
}
