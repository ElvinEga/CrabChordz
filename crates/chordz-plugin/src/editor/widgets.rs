//! Custom controls cloned from the original Chordz: rotary knobs, delay
//! slider boxes with a random-range overlay, and the piano keyboards.

use chordz_core::model::NUM_NOTE_SLOTS;
use nih_plug::prelude::{FloatParam, FloatRange, Param, ParamSetter};
use nih_plug_egui::egui::{
    self, Align2, Color32, CornerRadius, FontId, Pos2, Rect, Response, Sense, Stroke, Vec2,
};

use super::theme;
use crate::params::ChordzParams;

/// Identifies one of the 30 host parameters for value-entry dialogs.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ParamKind {
    Start,
    StartRand,
    End,
    EndRand,
    Vel,
    VelRand,
}

pub fn param_for<'a>(params: &'a ChordzParams, slot: usize, kind: ParamKind) -> &'a FloatParam {
    let slot = &params.slots[slot.min(NUM_NOTE_SLOTS - 1)];
    match kind {
        ParamKind::Start => &slot.start_ms,
        ParamKind::StartRand => &slot.start_random_ms,
        ParamKind::End => &slot.end_ms,
        ParamKind::EndRand => &slot.end_random_ms,
        ParamKind::Vel => &slot.velocity_scale,
        ParamKind::VelRand => &slot.velocity_random,
    }
}

pub fn param_bounds(param: &FloatParam) -> (f32, f32) {
    match param.range() {
        FloatRange::Linear { min, max } => (min, max),
        _ => (0.0, 1.0),
    }
}

pub fn format_value(param: &FloatParam) -> String {
    format!("{:.0}{}", param.value(), param.unit())
}

fn set_param(setter: &ParamSetter, param: &FloatParam, value: f32) {
    let (min, max) = param_bounds(param);
    setter.begin_set_parameter(param);
    setter.set_parameter(param, value.clamp(min, max));
    setter.end_set_parameter(param);
}

/// Shared drag/wheel/entry gestures for knobs and slider boxes. Returns the
/// value change requested this frame, if any. `alt` selects the random param.
fn edit_gestures(
    ui: &egui::Ui,
    response: &Response,
    param: &FloatParam,
    setter: &ParamSetter,
    slot: usize,
    kind: ParamKind,
    value_entry: &mut Option<(usize, ParamKind, String)>,
    horizontal: bool,
) {
    let (min, max) = param_bounds(param);
    let span = max - min;
    let modifiers = ui.input(|i| i.modifiers);

    if response.dragged() {
        let delta = response.drag_delta();
        let pixels = if horizontal { delta.x } else { -delta.y };
        let mut step = pixels * span / 150.0;
        if modifiers.shift {
            step /= 10.0;
        }
        set_param(setter, param, param.value() + step);
    }

    if response.hovered() {
        let scroll = ui.input(|i| i.raw_scroll_delta.y);
        if scroll != 0.0 {
            let mut step = scroll.signum() * span / 100.0;
            if modifiers.shift {
                step /= 10.0;
            }
            set_param(setter, param, param.value() + step);
        }
    }

    if response.double_clicked() {
        *value_entry = Some((slot, kind, format!("{:.1}", param.value())));
    }

    if response.clicked() && modifiers.ctrl {
        set_param(setter, param, param.default_plain_value());
    }

    response.context_menu(|ui| {
        if ui.button("Enter Value…").clicked() {
            *value_entry = Some((slot, kind, format!("{:.1}", param.value())));
            ui.close_menu();
        }
        if ui.button("Reset to Default").clicked() {
            set_param(setter, param, param.default_plain_value());
            ui.close_menu();
        }
    });
}

/// A rotary knob like the original's velocity controls.
pub fn knob(
    ui: &mut egui::Ui,
    setter: &ParamSetter,
    params: &ChordzParams,
    slot: usize,
    kind: ParamKind,
    label: &str,
    info: &mut String,
    value_entry: &mut Option<(usize, ParamKind, String)>,
) -> Response {
    let param = param_for(params, slot, kind);
    let size = Vec2::new(92.0, 108.0);
    let (rect, response) = ui.allocate_exact_size(size, Sense::click_and_drag());
    let painter = ui.painter_at(rect);
    let center = Pos2::new(rect.center().x, rect.top() + 40.0);
    let radius = 26.0;

    painter.text(
        Pos2::new(rect.center().x, rect.top() + 2.0),
        Align2::CENTER_TOP,
        label,
        FontId::proportional(12.0),
        theme::TEXT,
    );

    let (min, max) = param_bounds(param);
    let frac = ((param.value() - min) / (max - min)).clamp(0.0, 1.0);
    let start = std::f32::consts::PI * 0.75;
    let sweep = std::f32::consts::PI * 1.5;
    let angle = start + frac * sweep;
    let point = |a: f32, r: f32| Pos2::new(center.x + a.cos() * r, center.y + a.sin() * r);

    painter.circle_filled(center, radius + 3.0, theme::BG_DARK);
    let segments = 40;
    for i in 0..segments {
        let a0 = start + sweep * i as f32 / segments as f32;
        let a1 = start + sweep * (i + 1) as f32 / segments as f32;
        let lit = a1 <= angle + 0.001;
        painter.line_segment(
            [point(a0, radius), point(a1, radius)],
            Stroke::new(4.0_f32, if lit { theme::TEAL } else { theme::BORDER }),
        );
    }
    painter.circle_filled(center, radius - 7.0, theme::PANEL);
    painter.line_segment(
        [point(angle, radius - 20.0), point(angle, radius - 8.0)],
        Stroke::new(3.0_f32, theme::TEAL_BRIGHT),
    );

    painter.text(
        Pos2::new(rect.center().x, rect.bottom() - 16.0),
        Align2::CENTER_BOTTOM,
        format_value(param),
        FontId::proportional(12.0),
        theme::TEXT,
    );

    if response.hovered() {
        *info = format!("{}: {}", param.name(), format_value(param));
        ui.ctx().set_cursor_icon(egui::CursorIcon::ResizeVertical);
    }

    edit_gestures(ui, &response, param, setter, slot, kind, value_entry, false);
    response
}

/// A horizontal delay slider box. The main value fills teal; the random range
/// is an amber overlay edited by holding Alt.
pub fn delay_slider(
    ui: &mut egui::Ui,
    setter: &ParamSetter,
    params: &ChordzParams,
    slot: usize,
    kind: ParamKind,
    random_kind: ParamKind,
    label: &str,
    info: &mut String,
    value_entry: &mut Option<(usize, ParamKind, String)>,
) -> Response {
    let alt = ui.input(|i| i.modifiers.alt);
    let active_kind = if alt { random_kind } else { kind };
    let param = param_for(params, slot, active_kind);
    let main = param_for(params, slot, kind);
    let random = param_for(params, slot, random_kind);

    let size = Vec2::new(200.0, 34.0);
    let (rect, response) = ui.allocate_exact_size(size, Sense::click_and_drag());
    let painter = ui.painter_at(rect);

    painter.text(
        Pos2::new(rect.left(), rect.top()),
        Align2::LEFT_TOP,
        label,
        FontId::proportional(12.0),
        theme::TEXT,
    );

    let track = Rect::from_min_size(
        Pos2::new(rect.left(), rect.top() + 16.0),
        Vec2::new(150.0, 14.0),
    );
    painter.rect_filled(track, CornerRadius::same(2), theme::BG_DARK);
    painter.rect_stroke(
        track,
        CornerRadius::same(2),
        Stroke::new(1.0_f32, theme::BORDER),
        egui::StrokeKind::Outside,
    );

    let (min, max) = param_bounds(main);
    let main_frac = ((main.value() - min) / (max - min)).clamp(0.0, 1.0);
    let rand_frac = ((random.value() - min) / (max - min)).clamp(0.0, 1.0);
    let main_w = track.width() * main_frac;
    let mut fill = Rect::from_min_size(track.min, Vec2::new(main_w, track.height()));
    if main_w > 0.0 {
        painter.rect_filled(fill, CornerRadius::same(2), theme::TEAL_DIM);
    }
    let rand_w = (track.width() * (main_frac + rand_frac)).min(track.width()) - main_w;
    if rand_w > 0.0 {
        fill = Rect::from_min_size(
            Pos2::new(track.left() + main_w, track.top()),
            Vec2::new(rand_w, track.height()),
        );
        painter.rect_filled(fill, CornerRadius::same(2), theme::RANDOM_FILL);
    }

    painter.text(
        Pos2::new(rect.right(), track.center().y),
        Align2::RIGHT_CENTER,
        format_value(param),
        FontId::proportional(11.0),
        if alt {
            theme::RANDOM_FILL
        } else {
            theme::TEXT_WEAK
        },
    );

    if response.hovered() {
        *info = format!("{}: {}", param.name(), format_value(param));
        ui.ctx().set_cursor_icon(egui::CursorIcon::ResizeHorizontal);
    }

    edit_gestures(
        ui,
        &response,
        param,
        setter,
        slot,
        active_kind,
        value_entry,
        true,
    );
    response
}

// ---------------------------------------------------------------------------
// Piano keyboard geometry
// ---------------------------------------------------------------------------

pub const BLACK_KEY_PATTERN: [bool; 12] = [
    false, true, false, true, false, false, true, false, true, false, true, false,
];

/// Rects for a keyboard of `octaves` octaves starting at C. White keys are
/// numbered 0..; black keys sit between the white keys like on a real piano.
pub struct KeyboardLayout {
    pub rect: Rect,
    pub white_width: f32,
    pub white_height: f32,
    pub octaves: usize,
}

impl KeyboardLayout {
    pub fn new(rect: Rect, octaves: usize) -> Self {
        let whites = octaves * 7;
        Self {
            rect,
            white_width: rect.width() / whites as f32,
            white_height: rect.height(),
            octaves,
        }
    }

    /// Rect of white key `index` (0 = leftmost C).
    pub fn white_rect(&self, index: usize) -> Rect {
        Rect::from_min_size(
            Pos2::new(
                self.rect.left() + index as f32 * self.white_width,
                self.rect.top(),
            ),
            Vec2::new(self.white_width - 1.0, self.white_height),
        )
    }

    /// Rect of the black key whose pitch class is `pc` in octave `octave`.
    /// `pc` must be one of 1, 3, 6, 8, 10.
    pub fn black_rect(&self, octave: usize, pc: usize) -> Rect {
        let white_before = octave * 7 + WHITE_BEFORE_BLACK[pc];
        let x = self.rect.left() + white_before as f32 * self.white_width - self.white_width * 0.32;
        Rect::from_min_size(
            Pos2::new(x, self.rect.top()),
            Vec2::new(self.white_width * 0.64, self.white_height * 0.62),
        )
    }

    /// Absolute semitone offset from middle C for a key index.
    pub fn absolute(&self, key: usize) -> i16 {
        key as i16 - (self.octaves as i16 / 2) * 12
    }

    pub fn is_black(&self, key: usize) -> bool {
        BLACK_KEY_PATTERN[key % 12]
    }

    /// Rect of any key by absolute key index.
    pub fn key_rect(&self, key: usize) -> Rect {
        let octave = key / 12;
        let pc = key % 12;
        if self.is_black(key) {
            self.black_rect(octave, pc)
        } else {
            let white = octave * 7 + WHITE_INDEX[pc];
            self.white_rect(white)
        }
    }
}

/// White keys before each black key within an octave (C#=1, D#=3, ...).
const WHITE_BEFORE_BLACK: [usize; 12] = [0, 1, 0, 2, 0, 0, 3, 0, 4, 0, 5, 0];
/// White-key index of each white pitch class.
const WHITE_INDEX: [usize; 12] = [0, 0, 1, 0, 2, 3, 0, 4, 0, 5, 0, 6];

pub fn paint_key(painter: &egui::Painter, rect: Rect, fill: Color32, radius: u8) {
    painter.rect_filled(rect, CornerRadius::same(radius), fill);
    painter.rect_stroke(
        rect,
        CornerRadius::same(radius),
        Stroke::new(1.0_f32, theme::BORDER),
        egui::StrokeKind::Outside,
    );
}
