//! The egui editor, modelled on the original Chordz window: a header with the
//! menu, preset name and info panel, two main pages (Chord Setup and
//! Timing/Velocity) plus a slim Presets tab, and modal preset dialogs.
//!
//! The design is laid out on a fixed 1160×700 canvas that is proportionally
//! zoomed to fit the (resizable) host window.

use std::path::PathBuf;
use std::sync::Arc;

use chordz_core::data::{default_data_dir, load_library, suggest_triads, Library};
use chordz_core::midi::{list_midi_files, midi_dir, seed_midi_examples, MidiAnalysis};
use chordz_core::model::{ChordzState, Trigger};
use chordz_core::preset::{list_presets, preset_dir, PresetFormat};
use nih_plug::prelude::{Editor, Param, ParamSetter};
use nih_plug_egui::create_egui_editor;
use nih_plug_egui::egui::{self, Frame, Layout, Margin, Rect, RichText, UiBuilder, Vec2};
use nih_plug_egui::resizable_window::ResizableWindow;

mod presets;
mod setup;
mod theme;
mod timing;
mod widgets;

use crate::params::ChordzParams;
use widgets::ParamKind;

const DESIGN_W: f32 = 1160.0;
const DESIGN_H: f32 = 700.0;

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Tab {
    Setup,
    Timing,
    Presets,
}

/// Which list the Load Preset dialog is showing.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum LoadTab {
    Presets,
    Midi,
}

/// Which trigger the chord-note keyboard is editing.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Selection {
    Degree(usize),
    Variation(usize),
}

impl Selection {
    pub fn variation(self) -> bool {
        matches!(self, Self::Variation(_))
    }

    pub fn index(self) -> usize {
        match self {
            Self::Degree(index) | Self::Variation(index) => index,
        }
    }

    pub fn get(self, state: &ChordzState) -> Option<&Trigger> {
        match self {
            Self::Degree(index) => state.triggers.get(index),
            Self::Variation(index) => state.variations.get(index),
        }
    }

    pub fn get_mut(self, state: &mut ChordzState) -> Option<&mut Trigger> {
        match self {
            Self::Degree(index) => state.triggers.get_mut(index),
            Self::Variation(index) => state.variations.get_mut(index),
        }
    }
}

pub struct EditorState {
    library: Library,
    data_dir: PathBuf,
    presets: Vec<PathBuf>,
    tab: Tab,
    /// Name of the last loaded/saved preset, shown in the header.
    preset_name: String,
    save_name: String,
    preset_format: PresetFormat,
    message: String,
    /// Info panel text: the hovered control or the selected trigger.
    info: String,
    selection: Option<Selection>,
    /// Template picked in the dropdown but not applied yet.
    template_choice: Option<usize>,
    clipboard: Option<Trigger>,
    hover_key: Option<usize>,
    load_dialog: bool,
    load_tab: LoadTab,
    load_selected: Option<usize>,
    load_midi_selected: Option<usize>,
    save_dialog: bool,
    delete_confirm: Option<PathBuf>,
    id_edit: Option<(Selection, String)>,
    value_entry: Option<(usize, ParamKind, String)>,
    about: bool,
    midi_files: Vec<PathBuf>,
    midi_selected: Option<usize>,
    midi_analysis: Option<MidiAnalysis>,
    midi_tonic: u8,
    midi_minor: bool,
    midi_import_path: String,
}

pub fn create(params: Arc<ChordzParams>) -> Option<Box<dyn Editor>> {
    let data_dir = default_data_dir();
    let (library, warnings) = load_library(&data_dir);

    let mut message = if warnings.is_empty() {
        format!(
            "Loaded {} chord types and {} scale templates",
            library.chords.len(),
            library.templates.len()
        )
    } else {
        warnings.join(" | ")
    };

    let midi_dir = midi_dir();
    if let Err(err) = seed_midi_examples(&midi_dir) {
        message = format!("Could not seed MIDI examples: {err}");
    }

    let state = EditorState {
        library,
        presets: list_presets(&preset_dir()),
        data_dir,
        tab: Tab::Setup,
        preset_name: "Init".to_string(),
        save_name: String::new(),
        preset_format: PresetFormat::Codefn42p,
        message,
        info: String::new(),
        selection: None,
        template_choice: None,
        clipboard: None,
        hover_key: None,
        load_dialog: false,
        load_tab: LoadTab::Presets,
        load_selected: None,
        load_midi_selected: None,
        save_dialog: false,
        delete_confirm: None,
        id_edit: None,
        value_entry: None,
        about: false,
        midi_files: list_midi_files(&midi_dir),
        midi_selected: None,
        midi_analysis: None,
        midi_tonic: 0,
        midi_minor: false,
        midi_import_path: String::new(),
    };

    let egui_state = params.editor_state.clone();
    create_egui_editor(
        egui_state,
        state,
        |ctx, _state| {
            theme::apply(ctx);
        },
        move |ctx, setter, state| {
            draw(ctx, setter, state, &params);
        },
    )
}

fn draw(ctx: &egui::Context, setter: &ParamSetter, es: &mut EditorState, params: &ChordzParams) {
    // Proportional zoom: fit the design canvas into the window, letterboxing
    // the leftovers. Computed from the zoom-independent size so it is stable.
    let screen = ctx.screen_rect();
    let base_w = screen.width() * ctx.zoom_factor();
    let base_h = screen.height() * ctx.zoom_factor();
    let zoom = (base_w / DESIGN_W).min(base_h / DESIGN_H).clamp(0.4, 4.0);
    ctx.set_zoom_factor(zoom);

    ResizableWindow::new("crabchordz-window")
        .min_size(Vec2::new(580.0, 350.0))
        .show(ctx, &params.editor_state, |ui| {
            let available = ui.available_rect_before_wrap();
            let design = Rect::from_center_size(available.center(), Vec2::new(DESIGN_W, DESIGN_H));
            let mut ui = ui.new_child(UiBuilder::new().max_rect(design));
            draw_design(&mut ui, setter, es, params);
        });

    // Dialogs are drawn after the page so they can take their own state locks.
    if es.load_dialog {
        presets::load_dialog(ctx, es, params, setter);
    }
    if es.save_dialog {
        presets::save_dialog(ctx, es, params);
    }
    if es.delete_confirm.is_some() {
        presets::delete_confirm(ctx, es);
    }
    value_entry_dialog(ctx, es, params, setter);
    id_edit_dialog(ctx, es, params);
    about_dialog(ctx, es);

    // Keep the persisted setup's copy of the timing values in sync with the
    // host parameters so saved state and presets stay coherent.
    let setups = params.setups();
    let needs_sync = params.chordz_state.read().note_setup != setups;
    if needs_sync {
        params.chordz_state.write().note_setup = setups;
    }
}

fn draw_design(
    ui: &mut egui::Ui,
    setter: &ParamSetter,
    es: &mut EditorState,
    params: &ChordzParams,
) {
    let mut state = params.chordz_state.write();

    // Header: menu, title, preset name, info panel.
    ui.horizontal(|ui| {
        ui.menu_button(RichText::new("☰").size(18.0), |ui| {
            menu_contents(ui, es, &mut state, params, setter);
        });
        ui.add_space(6.0);
        ui.label(
            RichText::new("CrabChordz")
                .size(26.0)
                .strong()
                .color(theme::TEAL),
        );
        ui.add_space(10.0);
        if ui
            .button(RichText::new(&es.preset_name).size(14.0))
            .on_hover_text("Load a preset")
            .clicked()
        {
            es.presets = list_presets(&preset_dir());
            es.load_selected = None;
            es.load_midi_selected = None;
            es.load_dialog = true;
        }

        ui.with_layout(Layout::right_to_left(egui::Align::Center), |ui| {
            let text = if es.info.is_empty() {
                es.message.clone()
            } else {
                es.info.clone()
            };
            Frame::new()
                .fill(theme::BG_DARK)
                .corner_radius(4)
                .inner_margin(Margin::symmetric(8, 4))
                .show(ui, |ui| {
                    ui.set_width(330.0);
                    ui.add(
                        egui::Label::new(RichText::new(text).size(12.0).color(theme::TEXT))
                            .truncate(),
                    );
                });
        });
    });

    ui.add_space(4.0);
    ui.horizontal(|ui| {
        ui.selectable_value(&mut es.tab, Tab::Setup, "Chord Setup");
        ui.selectable_value(&mut es.tab, Tab::Timing, "Timing / Velocity");
        ui.selectable_value(&mut es.tab, Tab::Presets, "Presets");
    });
    ui.separator();
    ui.add_space(4.0);

    match es.tab {
        Tab::Setup => setup::page(ui, es, &mut state, params),
        Tab::Timing => timing::page(ui, es, setter, params, &mut state),
        Tab::Presets => presets::tab(ui, es, &mut state, params, setter),
    }
}

fn menu_contents(
    ui: &mut egui::Ui,
    es: &mut EditorState,
    state: &mut ChordzState,
    params: &ChordzParams,
    setter: &ParamSetter,
) {
    if ui.button("Init").clicked() {
        presets::init(es, state, params, setter);
        ui.close_menu();
    }
    if ui.button("Load Preset…").clicked() {
        es.presets = list_presets(&preset_dir());
        es.load_selected = None;
        es.load_midi_selected = None;
        es.load_dialog = true;
        ui.close_menu();
    }
    if ui.button("Save Preset…").clicked() {
        es.save_name = es.preset_name.clone();
        es.save_dialog = true;
        ui.close_menu();
    }
    ui.separator();
    ui.menu_button("Lowest Octave", |ui| {
        for lowest in [-2i8, -1, 0] {
            if ui
                .selectable_label(state.lowest_octave == lowest, format!("{lowest}"))
                .clicked()
            {
                state.lowest_octave = lowest;
                ui.close_menu();
            }
        }
    });
    ui.separator();
    if ui.button("Suggest triads").clicked() {
        apply_suggested_triads(es, state);
        ui.close_menu();
    }
    if ui.button("Reset to Major").clicked() {
        *state = ChordzState::default();
        es.selection = None;
        es.message = "Reset to C major".to_string();
        ui.close_menu();
    }
    if ui
        .button("Reload library")
        .on_hover_text("Re-read Chords.txt and Scale Templates.txt")
        .clicked()
    {
        let (library, warnings) = load_library(&es.data_dir);
        es.library = library;
        es.message = if warnings.is_empty() {
            format!("Reloaded {} chord types", es.library.chords.len())
        } else {
            warnings.join(" | ")
        };
        ui.close_menu();
    }
    ui.separator();
    if ui.button("About").clicked() {
        es.about = true;
        ui.close_menu();
    }
}

// ---------------------------------------------------------------------------
// Small modal dialogs
// ---------------------------------------------------------------------------

fn value_entry_dialog(
    ctx: &egui::Context,
    es: &mut EditorState,
    params: &ChordzParams,
    setter: &ParamSetter,
) {
    if es.value_entry.is_none() {
        return;
    }
    let mut commit = false;
    let mut cancel = false;
    let (slot, kind, text) = es.value_entry.as_mut().unwrap();
    let param = widgets::param_for(params, *slot, *kind);
    let modal = egui::Modal::new(egui::Id::new("value-entry")).show(ctx, |ui| {
        ui.set_width(240.0);
        ui.heading(param.name());
        ui.add_space(4.0);
        let response = ui.text_edit_singleline(text);
        if response.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
            commit = true;
        }
        ui.add_space(6.0);
        ui.horizontal(|ui| {
            if ui.button("OK").clicked() {
                commit = true;
            }
            if ui.button("Cancel").clicked() {
                cancel = true;
            }
        });
    });
    if commit {
        if let Ok(value) = text.trim().parse::<f32>() {
            let (min, max) = widgets::param_bounds(param);
            setter.begin_set_parameter(param);
            setter.set_parameter(param, value.clamp(min, max));
            setter.end_set_parameter(param);
        } else {
            es.message = format!("\"{text}\" is not a number");
        }
        es.value_entry = None;
    } else if cancel || modal.should_close() {
        es.value_entry = None;
    }
}

fn id_edit_dialog(ctx: &egui::Context, es: &mut EditorState, params: &ChordzParams) {
    if es.id_edit.is_none() {
        return;
    }
    let mut commit = false;
    let mut cancel = false;
    let (selection, text) = es.id_edit.as_mut().unwrap();
    let selection = *selection;
    let modal = egui::Modal::new(egui::Id::new("chord-id")).show(ctx, |ui| {
        ui.set_width(240.0);
        ui.heading("Chord ID");
        ui.add_space(4.0);
        ui.label(RichText::new("Up to 12 characters.").small().weak());
        let response = ui.text_edit_singleline(text);
        if response.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
            commit = true;
        }
        ui.add_space(6.0);
        ui.horizontal(|ui| {
            if ui.button("OK").clicked() {
                commit = true;
            }
            if ui.button("Cancel").clicked() {
                cancel = true;
            }
        });
    });
    if commit {
        let mut state = params.chordz_state.write();
        if let Some(trigger) = selection.get_mut(&mut state) {
            trigger.set_chord_id(text.trim());
        }
        es.id_edit = None;
    } else if cancel || modal.should_close() {
        es.id_edit = None;
    }
}

fn about_dialog(ctx: &egui::Context, es: &mut EditorState) {
    if !es.about {
        return;
    }
    let mut close = false;
    let modal = egui::Modal::new(egui::Id::new("about")).show(ctx, |ui| {
        ui.set_width(380.0);
        ui.heading("CrabChordz");
        ui.add_space(4.0);
        ui.label(format!("Version {}", env!("CARGO_PKG_VERSION")));
        ui.add_space(4.0);
        ui.label(
            "A native macOS VST3/CLAP MIDI chord generator, an independent \
             reimplementation of CodeFN42 Chordz 1.14's behaviour built from the \
             plugin's observable metadata, embedded chord/scale data and the \
             official user guide. It shares no code with the original.",
        );
        ui.add_space(4.0);
        ui.label(
            RichText::new(
                "CrabChordz is a MIDI processor: place it before an instrument. \
                 In Ableton Live use the CLAP build.",
            )
            .small()
            .weak(),
        );
        ui.add_space(8.0);
        if ui.button("Close").clicked() {
            close = true;
        }
    });
    if close || modal.should_close() {
        es.about = false;
    }
}

// ---------------------------------------------------------------------------
// Helpers shared with the pages
// ---------------------------------------------------------------------------

pub fn set_tonic(state: &mut ChordzState, tonic: u8) {
    let tonic = tonic % 12;
    let delta = (tonic as i16 - state.tonic as i16).rem_euclid(12) as u8;
    state.tonic = tonic;
    for trigger in state.triggers.iter_mut().chain(state.variations.iter_mut()) {
        trigger.root = (trigger.root + delta) % 12;
    }
}

pub fn apply_suggested_triads(es: &mut EditorState, state: &mut ChordzState) {
    let Some(template) = es
        .library
        .templates
        .iter()
        .find(|template| template.name == state.template_name)
        .cloned()
    else {
        es.message = "Select a scale template first".to_string();
        return;
    };

    if state.triggers.len() != template.degrees.len() {
        es.message =
            "The trigger list has been edited; select the template again first".to_string();
        return;
    }

    let suggestions = suggest_triads(&template, state.tonic);
    for (index, (root, chord_id, notes)) in suggestions.into_iter().enumerate() {
        if let Some(trigger) = state.triggers.get_mut(index) {
            trigger.root = root;
            trigger.set_chord_id(&chord_id);
            trigger.notes = notes;
        }
    }
    es.message = format!(
        "Replaced the triggers with triads from \"{}\"",
        template.name
    );
}
