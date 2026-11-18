//! The egui editor.
//!
//! The layout follows the original Chordz window: a trigger keyboard at the top,
//! the scale/chord setup below it, a note timing page, and preset handling.
//! The timing page uses host parameters so those values can be automated.

use std::path::PathBuf;
use std::sync::Arc;

use chordz_core::data::{default_data_dir, load_library, suggest_triads, Library, MenuEntry};
use chordz_core::model::{
    note_name, note_name_with_octave, ChordzState, NoteSetup, NUM_NOTE_SLOTS,
};
use chordz_core::preset::{
    list_presets, load_preset_file, preset_dir, save_preset_as, Preset, PresetFormat,
};
use nih_plug::prelude::{Editor, ParamSetter};
use nih_plug_egui::create_egui_editor;
use nih_plug_egui::egui::{
    self, Align2, Color32, ComboBox, CornerRadius, FontId, RichText, Sense, Vec2,
};
use nih_plug_egui::widgets::ParamSlider;

use crate::params::ChordzParams;

const KEYS_START: u8 = 48; // C3
const KEYS_END: u8 = 83; // B5

const WHITE_KEY_WIDTH: f32 = 34.0;
const KEYBOARD_HEIGHT: f32 = 92.0;

const TRIGGER_FILL: Color32 = Color32::from_rgb(247, 224, 158);
const TRIGGER_FILL_HOVER: Color32 = Color32::from_rgb(255, 238, 188);
const TRIGGER_BLACK_FILL: Color32 = Color32::from_rgb(196, 140, 40);
const TRIGGER_BLACK_HOVER: Color32 = Color32::from_rgb(228, 168, 56);
const WHITE_FILL: Color32 = Color32::from_rgb(238, 238, 236);
const BLACK_FILL: Color32 = Color32::from_rgb(54, 54, 58);

#[derive(Clone, Copy, PartialEq, Eq)]
enum Tab {
    Chords,
    Timing,
    Presets,
    Help,
}

struct EditorState {
    library: Library,
    data_dir: PathBuf,
    presets: Vec<PathBuf>,
    tab: Tab,
    preset_name: String,
    preset_format: PresetFormat,
    message: String,
}

pub fn create(params: Arc<ChordzParams>) -> Option<Box<dyn Editor>> {
    let data_dir = default_data_dir();
    let (library, warnings) = load_library(&data_dir);

    let message = if warnings.is_empty() {
        format!(
            "Loaded {} chord types and {} scale templates from {}",
            library.chords.len(),
            library.templates.len(),
            data_dir.display()
        )
    } else {
        warnings.join(" | ")
    };

    let state = EditorState {
        library,
        presets: list_presets(&preset_dir()),
        data_dir,
        tab: Tab::Chords,
        preset_name: "My Chords".to_string(),
        preset_format: PresetFormat::Codefn42p,
        message,
    };

    let egui_state = params.editor_state.clone();
    create_egui_editor(
        egui_state,
        state,
        |ctx, _state| {
            ctx.set_visuals(egui::Visuals::dark());
        },
        move |ctx, setter, state| {
            draw(ctx, setter, state, &params);
        },
    )
}

fn draw(ctx: &egui::Context, setter: &ParamSetter, es: &mut EditorState, params: &ChordzParams) {
    egui::TopBottomPanel::top("chordzmac_header").show(ctx, |ui| {
        ui.add_space(4.0);
        ui.horizontal(|ui| {
            ui.heading(RichText::new("ChordzMac").strong());
            ui.label(RichText::new("MIDI chord generator").weak());
            ui.separator();
            ui.selectable_value(&mut es.tab, Tab::Chords, "Chords");
            ui.selectable_value(&mut es.tab, Tab::Timing, "Note Timing");
            ui.selectable_value(&mut es.tab, Tab::Presets, "Presets");
            ui.selectable_value(&mut es.tab, Tab::Help, "Help");
        });
        ui.add_space(4.0);
    });

    egui::TopBottomPanel::bottom("chordzmac_status").show(ctx, |ui| {
        ui.horizontal_wrapped(|ui| {
            ui.label(RichText::new(&es.message).small());
        });
    });

    egui::CentralPanel::default().show(ctx, |ui| {
        let mut state = params.chordz_state.write();
        match es.tab {
            Tab::Chords => chords_tab(ui, es, &mut state, params),
            Tab::Timing => timing_tab(ui, setter, params),
            Tab::Presets => presets_tab(ui, setter, es, &mut state, params),
            Tab::Help => help_tab(ui),
        }
    });

    // Keep the persisted setup's copy of the timing values in sync with the
    // host parameters so saved state and presets stay coherent.
    let setups = params.setups();
    let needs_sync = params.chordz_state.read().note_setup != setups;
    if needs_sync {
        params.chordz_state.write().note_setup = setups;
    }
}

fn chords_tab(
    ui: &mut egui::Ui,
    es: &mut EditorState,
    state: &mut ChordzState,
    params: &ChordzParams,
) {
    ui.horizontal(|ui| {
        ui.label("Tonic");
        ComboBox::from_id_salt("tonic")
            .selected_text(RichText::new(note_name(state.tonic)).strong())
            .width(64.0)
            .show_ui(ui, |ui| {
                for pc in 0u8..12 {
                    if ui
                        .selectable_label(state.tonic == pc, note_name(pc))
                        .clicked()
                    {
                        set_tonic(state, pc);
                    }
                }
            });
        if ui
            .small_button("−")
            .on_hover_text("Transpose down a semitone")
            .clicked()
        {
            state.transpose_tonic(-1);
        }
        if ui
            .small_button("+")
            .on_hover_text("Transpose up a semitone")
            .clicked()
        {
            state.transpose_tonic(1);
        }

        ui.separator();
        ui.label("Scale / chord template");
        let template_label = if state.template_name.is_empty() {
            "Custom".to_string()
        } else {
            state.template_name.clone()
        };
        ComboBox::from_id_salt("template")
            .selected_text(template_label)
            .width(230.0)
            .show_ui(ui, |ui| {
                egui::ScrollArea::vertical()
                    .max_height(360.0)
                    .show(ui, |ui| {
                        for entry in &es.library.template_menu {
                            match entry {
                                MenuEntry::Group(name) => {
                                    ui.label(RichText::new(name).small().strong());
                                }
                                MenuEntry::Separator => {
                                    ui.separator();
                                }
                                MenuEntry::Item(template_index) => {
                                    let template = &es.library.templates[*template_index];
                                    if ui
                                        .selectable_label(
                                            state.template_name == template.name,
                                            &template.name,
                                        )
                                        .clicked()
                                    {
                                        state.apply_template(template);
                                    }
                                }
                            }
                        }
                    });
            });

        if ui
            .button("Suggest triads")
            .on_hover_text(
                "Replace the chord triggers with diatonic triads built from the current scale",
            )
            .clicked()
        {
            apply_suggested_triads(es, state);
        }
    });

    ui.add_space(6.0);
    draw_keyboard(ui, state, params);
    ui.add_space(2.0);
    ui.label(
        RichText::new(
            "Click a key to preview its chord. Amber keys trigger chords; grey keys are outside the scale.",
        )
        .small()
        .weak(),
    );
    ui.separator();

    ui.columns(2, |columns| {
        columns[0].heading("Chord triggers");
        egui::ScrollArea::vertical()
            .id_salt("triggers-scroll")
            .max_height(300.0)
            .show(&mut columns[0], |ui| {
                let count = state.triggers.len();
                for index in 0..count {
                    let root = state.triggers[index].root;
                    let chord_id = state.triggers[index].chord_id.clone();
                    let enabled = state.triggers[index].enabled;
                    ui.horizontal(|ui| {
                        ui.checkbox(&mut state.triggers[index].enabled, "")
                            .on_hover_text(
                                "Unused triggers keep their key position but play nothing",
                            );
                        ui.label(
                            RichText::new(format!("{:>2}. {}", index + 1, note_name(root)))
                                .monospace(),
                        );
                        chord_selector(
                            ui,
                            ("trigger-chord", index),
                            &chord_id,
                            false,
                            index,
                            state,
                            &es.library,
                        );
                        if ui
                            .add_enabled(enabled, egui::Button::new("▶").small())
                            .on_hover_text("Preview")
                            .clicked()
                        {
                            params.request_preview(false, index);
                        }
                    });
                }

                if state.variations.iter().any(|trigger| trigger.enabled) {
                    ui.add_space(6.0);
                    ui.label(RichText::new("Black key variations").strong());
                    let count = state.variations.len();
                    for index in 0..count {
                        let root = state.variations[index].root;
                        let chord_id = state.variations[index].chord_id.clone();
                        let enabled = state.variations[index].enabled;
                        ui.horizontal(|ui| {
                            ui.checkbox(&mut state.variations[index].enabled, "")
                                .on_hover_text(
                                    "Unused variations keep their key position but play nothing",
                                );
                            ui.label(
                                RichText::new(format!("{:>2}. {}", index + 1, note_name(root)))
                                    .monospace(),
                            );
                            chord_selector(
                                ui,
                                ("variation-chord", index),
                                &chord_id,
                                true,
                                index,
                                state,
                                &es.library,
                            );
                            if ui
                                .add_enabled(enabled, egui::Button::new("▶").small())
                                .on_hover_text("Preview")
                                .clicked()
                            {
                                params.request_preview(true, index);
                            }
                        });
                    }
                }
            });

        columns[1].heading("Setup");
        egui::ScrollArea::vertical()
            .id_salt("setup-scroll")
            .max_height(300.0)
            .show(&mut columns[1], |ui| {
                ui.checkbox(&mut state.easy_mode, "Easy mode")
                    .on_hover_text(
                        "The seven white keys play the first seven scale degrees; extra degrees \
                         and black-key variations move to C#, D#, F#, G#, A#.",
                    );
                ui.checkbox(&mut state.follow_trigger_notes, "Follow trigger notes")
                    .on_hover_text("Notes outside the chord zone are snapped to the scale.");
                ui.checkbox(&mut state.relative_timing, "Relative timing")
                    .on_hover_text("Start and end delays accumulate across the chord notes.");
                ui.checkbox(&mut state.same_random_velocity, "Same random velocity")
                    .on_hover_text("All notes in a chord share one random velocity draw.");
                ui.checkbox(&mut state.random_note_assignment, "Random note assignment")
                    .on_hover_text("Assign each chord note to a random timing slot.");

                ui.add_space(4.0);
                ui.separator();
                ui.label(RichText::new("Keyboard zones").strong());
                ui.horizontal(|ui| {
                    ui.label("Chords from octave");
                    ui.add(
                        egui::DragValue::new(&mut state.octave_from)
                            .range(-2..=8)
                            .speed(0.1),
                    );
                    ui.label("to");
                    ui.add(
                        egui::DragValue::new(&mut state.octave_to)
                            .range(-2..=8)
                            .speed(0.1),
                    );
                });
                ui.horizontal(|ui| {
                    ui.label("Transpose chords");
                    ui.add(
                        egui::DragValue::new(&mut state.transpose_octave)
                            .range(-4..=4)
                            .speed(0.1),
                    )
                    .on_hover_text("Octaves added to every generated chord note");
                    ui.label("octave(s)");
                });
                ui.horizontal(|ui| {
                    ui.label("Below zone");
                    ui.add(
                        egui::DragValue::new(&mut state.transpose_below)
                            .range(-4..=4)
                            .speed(0.1),
                    );
                    ui.label("Above zone");
                    ui.add(
                        egui::DragValue::new(&mut state.transpose_above)
                            .range(-4..=4)
                            .speed(0.1),
                    );
                });
                ui.horizontal(|ui| {
                    ui.label("MIDI note 0 is octave");
                    ui.add(egui::DragValue::new(&mut state.lowest_octave).range(-2..=0));
                });

                ui.add_space(4.0);
                ui.separator();
                ui.horizontal(|ui| {
                    if ui.button("Reset to Major").clicked() {
                        *state = ChordzState::default();
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
                    }
                });
            });
    });
}

fn chord_selector(
    ui: &mut egui::Ui,
    id: impl std::hash::Hash,
    current: &str,
    variation: bool,
    index: usize,
    state: &mut ChordzState,
    library: &Library,
) {
    ComboBox::from_id_salt(id)
        .selected_text(if current.is_empty() { "—" } else { current })
        .width(140.0)
        .show_ui(ui, |ui| {
            egui::ScrollArea::vertical()
                .max_height(320.0)
                .show(ui, |ui| {
                    let mut last_was_group = false;
                    for entry in &library.chord_menu {
                        match entry {
                            MenuEntry::Group(name) => {
                                if !last_was_group {
                                    ui.add_space(2.0);
                                }
                                ui.label(RichText::new(name).small().weak());
                                last_was_group = true;
                            }
                            MenuEntry::Separator => {
                                ui.separator();
                                last_was_group = false;
                            }
                            MenuEntry::Item(chord_index) => {
                                let chord = &library.chords[*chord_index];
                                if ui
                                    .selectable_label(false, &chord.name)
                                    .on_hover_text(describe(library, *chord_index))
                                    .clicked()
                                {
                                    let target = if variation {
                                        state.variations.get_mut(index)
                                    } else {
                                        state.triggers.get_mut(index)
                                    };
                                    if let Some(target) = target {
                                        target.notes = chord.notes.clone();
                                        target.set_chord_id(&chord.chord_id);
                                    }
                                }
                                last_was_group = false;
                            }
                        }
                    }
                });
        });
}

fn describe(library: &Library, chord_index: usize) -> String {
    let chord = &library.chords[chord_index];
    if chord.notes.is_empty() {
        return chord.name.clone();
    }
    let mut parts = Vec::new();
    for note in &chord.notes {
        parts.push(format!("{:+}", note.semitone));
    }
    parts.join(" ")
}

fn timing_tab(ui: &mut egui::Ui, setter: &ParamSetter, params: &ChordzParams) {
    ui.heading("Note timing and velocity");
    ui.label(
        RichText::new(
            "Each chord note is assigned to one of five slots (the original numbers the notes of \
             a chord 1 to 5, and the /n suffix in Chords.txt overrides that). Start and End are \
             delays in milliseconds; velocity is scaled in percent.",
        )
        .small()
        .weak(),
    );
    ui.add_space(6.0);

    egui::ScrollArea::vertical().show(ui, |ui| {
        ui.columns(NUM_NOTE_SLOTS, |columns| {
            for (slot, column) in columns.iter_mut().enumerate() {
                let slot_params = &params.slots[slot];
                column.heading(format!("Note {}", slot + 1));
                column.add(ParamSlider::for_param(&slot_params.start_ms, setter));
                column.add(ParamSlider::for_param(&slot_params.start_random_ms, setter));
                column.add(ParamSlider::for_param(&slot_params.end_ms, setter));
                column.add(ParamSlider::for_param(&slot_params.end_random_ms, setter));
                column.add(ParamSlider::for_param(&slot_params.velocity_scale, setter));
                column.add(ParamSlider::for_param(&slot_params.velocity_random, setter));
            }
        });

        ui.add_space(10.0);
        ui.separator();
        ui.label(
            RichText::new(
                "Tip: negative velocity scale makes notes softer, positive makes them louder. \
                 Velocity random adds up to the given percentage on top.",
            )
            .small()
            .weak(),
        );
    });
}

fn presets_tab(
    ui: &mut egui::Ui,
    setter: &ParamSetter,
    es: &mut EditorState,
    state: &mut ChordzState,
    params: &ChordzParams,
) {
    ui.heading("Presets");
    ui.label(
        RichText::new(format!(
            "Presets are stored in {}. Both original Chordz (.codefn42p) and \
             ChordzMac (.json) files are listed.",
            preset_dir().display()
        ))
        .small()
        .weak(),
    );
    ui.add_space(4.0);

    ui.horizontal(|ui| {
        ui.label("Name");
        ui.text_edit_singleline(&mut es.preset_name);
        ComboBox::from_id_salt("preset-format")
            .selected_text(match es.preset_format {
                PresetFormat::Codefn42p => "Chordz (.codefn42p)",
                PresetFormat::Json => "ChordzMac (.json)",
            })
            .show_ui(ui, |ui| {
                ui.selectable_value(
                    &mut es.preset_format,
                    PresetFormat::Codefn42p,
                    "Chordz (.codefn42p)",
                );
                ui.selectable_value(
                    &mut es.preset_format,
                    PresetFormat::Json,
                    "ChordzMac (.json)",
                );
            });
        if ui.button("Save preset").clicked() {
            let mut snapshot = state.clone();
            let setups: [NoteSetup; NUM_NOTE_SLOTS] = params.setups();
            snapshot.note_setup = setups;
            let preset = Preset::new(es.preset_name.clone(), snapshot, setups);
            match save_preset_as(&preset, &preset_dir(), es.preset_format) {
                Ok(path) => {
                    es.message = format!("Saved {}", path.display());
                    es.presets = list_presets(&preset_dir());
                }
                Err(err) => es.message = err,
            }
        }
    });

    ui.separator();
    let paths = es.presets.clone();
    if paths.is_empty() {
        ui.label("No presets saved yet.");
    } else {
        egui::ScrollArea::vertical().show(ui, |ui| {
            for path in paths {
                ui.horizontal(|ui| {
                    let name = path
                        .file_stem()
                        .map(|stem| stem.to_string_lossy().to_string())
                        .unwrap_or_else(|| path.display().to_string());
                    let extension = path
                        .extension()
                        .map(|ext| ext.to_string_lossy().to_string())
                        .unwrap_or_default();
                    ui.label(name);
                    ui.label(RichText::new(format!(".{extension}")).small().weak());
                    if ui.button("Load").clicked() {
                        match load_preset_file(&path) {
                            Ok(preset) => {
                                *state = preset.state;
                                es.preset_name = preset.name.clone();
                                es.preset_format =
                                    match path.extension().and_then(|ext| ext.to_str()) {
                                        Some("json") => PresetFormat::Json,
                                        _ => PresetFormat::Codefn42p,
                                    };
                                params.apply_setups(&preset.note_setup, setter);
                                es.message = format!("Loaded {}", path.display());
                            }
                            Err(err) => es.message = err,
                        }
                    }
                });
            }
        });
    }

    ui.add_space(8.0);
    ui.separator();
    ui.label(RichText::new("Data files").strong());
    ui.label(
        RichText::new(format!(
            "Chords.txt and Scale Templates.txt live in {}. Edit them and press \
             \"Reload library\" on the Chords page to pick up the changes.",
            es.data_dir.display()
        ))
        .small(),
    );
}

fn help_tab(ui: &mut egui::Ui) {
    egui::ScrollArea::vertical().show(ui, |ui| {
        ui.heading("How ChordzMac works");
        ui.label(
            "ChordzMac is a MIDI processor. Put it before an instrument and play notes in the \
             chord trigger zone: every note is replaced by the chord assigned to its scale degree. \
             Outside the zone the note is passed through, optionally transposed or snapped to the \
             scale.",
        );
        ui.add_space(6.0);
        ui.label(RichText::new("Playing").strong());
        ui.label(
            "• With Easy mode off, the pressed note's pitch class must match a chord root.\n\
             • With Easy mode on, the white keys play the first seven scale degrees.\n\
             • Octave From/To choose which octaves trigger chords.\n\
             • Transpose Chords shifts generated notes by whole octaves.\n\
             • Notes below/above the zone are transposed by the Below/Above zone values.",
        );
        ui.add_space(6.0);
        ui.label(RichText::new("Timing").strong());
        ui.label(
            "Start/End delays and velocity scaling are per note slot. Enable Relative timing to \
             make the delays accumulate across the chord, which produces strummed chords.",
        );
        ui.add_space(6.0);
        ui.label(RichText::new("Host routing").strong());
        ui.label(
            "In Bitwig Studio and REAPER, add the plugin as a note effect and route its MIDI out \
             to an instrument track. In Ableton Live, use the CLAP version on a MIDI track (Live's \
             VST3 implementation does not expose MIDI output from instruments).",
        );
        ui.add_space(6.0);
        ui.label(
            RichText::new(
                "ChordzMac is an independent reimplementation of CodeFN42 Chordz 1.14's behaviour, \
                 built from the plugin's observable metadata, embedded chord/scale data and the \
                 official user guide. It shares no code with the original.",
            )
            .small()
            .weak(),
        );
    });
}

// ---------------------------------------------------------------------------
// Trigger keyboard
// ---------------------------------------------------------------------------

const BLACK_KEYS: [bool; 12] = [
    false, true, false, true, false, false, true, false, true, false, true, false,
];

fn draw_keyboard(ui: &mut egui::Ui, state: &ChordzState, params: &ChordzParams) {
    let white_notes: Vec<u8> = (KEYS_START..=KEYS_END)
        .filter(|note| !BLACK_KEYS[(note % 12) as usize])
        .collect();
    let width = white_notes.len() as f32 * WHITE_KEY_WIDTH;
    let (area, _) = ui.allocate_exact_size(Vec2::new(width, KEYBOARD_HEIGHT), Sense::hover());
    let painter = ui.painter_at(area);

    let mut preview: Option<(bool, usize)> = None;

    // White keys first so the black keys can overlap them.
    for (index, note) in white_notes.iter().enumerate() {
        let rect = egui::Rect::from_min_size(
            egui::pos2(area.left() + index as f32 * WHITE_KEY_WIDTH, area.top()),
            Vec2::new(WHITE_KEY_WIDTH - 1.0, KEYBOARD_HEIGHT),
        );
        let response = ui.interact(rect, ui.id().with(("white-key", *note)), Sense::click());
        let resolution = chordz_core::resolve(state, *note);
        let hovered = response.hovered() && resolution.is_some();
        let fill = match (resolution.is_some(), hovered) {
            (true, true) => TRIGGER_FILL_HOVER,
            (true, false) => TRIGGER_FILL,
            (false, _) => WHITE_FILL,
        };
        painter.rect_filled(rect, CornerRadius::same(2), fill);

        painter.text(
            egui::pos2(rect.center().x, rect.bottom() - 4.0),
            Align2::CENTER_BOTTOM,
            note_name_with_octave(*note, state.lowest_octave),
            FontId::proportional(8.0),
            Color32::from_rgb(90, 90, 90),
        );

        if let Some(resolution) = resolution {
            let trigger = if resolution.variation {
                state.variations.get(resolution.index)
            } else {
                state.triggers.get(resolution.index)
            };
            if let Some(trigger) = trigger {
                if !trigger.chord_id.is_empty() {
                    painter.text(
                        egui::pos2(rect.center().x, rect.top() + 6.0),
                        Align2::CENTER_TOP,
                        &trigger.chord_id,
                        FontId::proportional(9.0),
                        Color32::from_rgb(90, 62, 10),
                    );
                }
            }
            if response.clicked() {
                preview = Some((resolution.variation, resolution.index));
            }
            if hovered {
                ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
            }
        }
    }

    // Black keys.
    for note in KEYS_START..=KEYS_END {
        if !BLACK_KEYS[(note % 12) as usize] {
            continue;
        }
        let Some(next_white) = white_notes.iter().position(|white| *white > note) else {
            continue;
        };
        let x = area.left() + next_white as f32 * WHITE_KEY_WIDTH - WHITE_KEY_WIDTH * 0.30;
        let rect = egui::Rect::from_min_size(
            egui::pos2(x, area.top()),
            Vec2::new(WHITE_KEY_WIDTH * 0.60, KEYBOARD_HEIGHT * 0.62),
        );
        let response = ui.interact(rect, ui.id().with(("black-key", note)), Sense::click());
        let resolution = chordz_core::resolve(state, note);
        let hovered = response.hovered() && resolution.is_some();
        let fill = match (resolution.is_some(), hovered) {
            (true, true) => TRIGGER_BLACK_HOVER,
            (true, false) => TRIGGER_BLACK_FILL,
            (false, _) => BLACK_FILL,
        };
        painter.rect_filled(rect, CornerRadius::same(2), fill);
        if let Some(resolution) = resolution {
            if response.clicked() {
                preview = Some((resolution.variation, resolution.index));
            }
        }
        if hovered {
            ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
        }
    }

    if let Some((variation, index)) = preview {
        params.request_preview(variation, index);
    }
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn set_tonic(state: &mut ChordzState, tonic: u8) {
    let tonic = tonic % 12;
    let delta = (tonic as i16 - state.tonic as i16).rem_euclid(12) as u8;
    state.tonic = tonic;
    for trigger in state.triggers.iter_mut().chain(state.variations.iter_mut()) {
        trigger.root = (trigger.root + delta) % 12;
    }
}

fn apply_suggested_triads(es: &mut EditorState, state: &mut ChordzState) {
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
