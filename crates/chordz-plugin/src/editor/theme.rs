//! The original Chordz palette: charcoal panels, teal accents, light keys.

use nih_plug_egui::egui::{self, Color32, Stroke};

pub const BG: Color32 = Color32::from_rgb(43, 43, 43);
pub const BG_DARK: Color32 = Color32::from_rgb(32, 32, 32);
pub const PANEL: Color32 = Color32::from_rgb(38, 38, 38);
pub const BORDER: Color32 = Color32::from_rgb(24, 24, 24);
pub const TEXT: Color32 = Color32::from_rgb(224, 224, 224);
pub const TEXT_WEAK: Color32 = Color32::from_rgb(150, 150, 150);
pub const TEXT_ON_TEAL: Color32 = Color32::from_rgb(20, 40, 38);

pub const TEAL: Color32 = Color32::from_rgb(86, 190, 180);
pub const TEAL_BRIGHT: Color32 = Color32::from_rgb(120, 214, 204);
pub const TEAL_DIM: Color32 = Color32::from_rgb(52, 118, 110);
pub const RANDOM_FILL: Color32 = Color32::from_rgb(196, 140, 40);

pub const WHITE_KEY: Color32 = Color32::from_rgb(242, 242, 240);
pub const WHITE_KEY_HOVER: Color32 = Color32::from_rgb(255, 255, 255);
pub const BLACK_KEY: Color32 = Color32::from_rgb(38, 38, 42);
pub const BLACK_KEY_HOVER: Color32 = Color32::from_rgb(58, 58, 64);
pub const KEY_TEXT: Color32 = Color32::from_rgb(70, 70, 70);

pub fn apply(ctx: &egui::Context) {
    let mut visuals = egui::Visuals::dark();
    visuals.panel_fill = BG;
    visuals.window_fill = BG_DARK;
    visuals.extreme_bg_color = BG_DARK;
    visuals.faint_bg_color = PANEL;
    visuals.code_bg_color = BG_DARK;
    visuals.window_stroke = Stroke::new(1.0_f32, BORDER);
    visuals.widgets.noninteractive.bg_fill = PANEL;
    visuals.widgets.noninteractive.fg_stroke = Stroke::new(1.0_f32, TEXT);
    visuals.widgets.inactive.bg_fill = Color32::from_rgb(58, 58, 58);
    visuals.widgets.inactive.fg_stroke = Stroke::new(1.0_f32, TEXT);
    visuals.widgets.hovered.bg_fill = Color32::from_rgb(74, 74, 74);
    visuals.widgets.hovered.fg_stroke = Stroke::new(1.0_f32, Color32::WHITE);
    visuals.widgets.active.bg_fill = TEAL_DIM;
    visuals.widgets.active.fg_stroke = Stroke::new(1.0_f32, Color32::WHITE);
    visuals.widgets.open.bg_fill = PANEL;
    visuals.selection.bg_fill = TEAL_DIM;
    visuals.selection.stroke = Stroke::new(1.0_f32, TEAL);
    visuals.hyperlink_color = TEAL;
    ctx.set_visuals(visuals);

    let mut style = (*ctx.style()).clone();
    style.spacing.item_spacing = egui::vec2(8.0, 6.0);
    style.spacing.button_padding = egui::vec2(10.0, 4.0);
    style.spacing.menu_margin = egui::Margin::same(6);
    style.visuals = ctx.style().visuals.clone();
    ctx.set_style(style);
}
