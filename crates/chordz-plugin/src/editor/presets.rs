//! Preset handling: the load dialog (three alphabetical columns like the
//! original), the save dialog, delete confirmation, and the Presets tab.

use std::path::{Path, PathBuf};

use chordz_core::midi::{
    analyze_midi, apply_analysis, estimate_tonic_for_mode, import_midi_file, list_midi_files,
    midi_dir, parse_midi,
};
use chordz_core::model::{note_name, ChordzState, NoteSetup, NUM_NOTE_SLOTS};
use chordz_core::preset::{
    list_presets, load_preset_file, preset_dir, save_preset_as, Preset, PresetFormat,
};
use nih_plug::prelude::ParamSetter;
use nih_plug_egui::egui::{self, ComboBox, RichText};

use super::{EditorState, Selection};
use crate::params::ChordzParams;

pub fn load_path(
    es: &mut EditorState,
    state: &mut ChordzState,
    params: &ChordzParams,
    setter: &ParamSetter,
    path: &Path,
) {
    match load_preset_file(path) {
        Ok(preset) => {
            *state = preset.state;
            params.apply_setups(&preset.note_setup, setter);
            es.preset_name = preset.name.clone();
            es.preset_format = PresetFormat::from_path(path).unwrap_or(PresetFormat::Codefn42p);
            es.message = format!("Loaded {}", path.display());
            es.selection = None;
        }
        Err(err) => es.message = err,
    }
}

pub fn init(
    es: &mut EditorState,
    state: &mut ChordzState,
    params: &ChordzParams,
    setter: &ParamSetter,
) {
    let init_preset = es.presets.iter().find(|path| {
        path.file_stem()
            .is_some_and(|stem| stem.to_string_lossy().eq_ignore_ascii_case("init"))
    });
    match init_preset.cloned() {
        Some(path) => load_path(es, state, params, setter, &path),
        None => {
            *state = ChordzState::default();
            params.apply_setups(&[NoteSetup::default(); NUM_NOTE_SLOTS], setter);
            es.preset_name = "Init".to_string();
            es.selection = None;
            es.message = "Initialised to C major".to_string();
        }
    }
}

fn show_in_folder(es: &mut EditorState, path: &Path) {
    let folder = path.parent().unwrap_or(path);
    match std::process::Command::new("open").arg(folder).spawn() {
        Ok(_) => es.message = format!("Opened {}", folder.display()),
        Err(err) => es.message = format!("Could not open folder: {err}"),
    }
}

fn preset_name(path: &Path) -> String {
    path.file_stem()
        .map(|stem| stem.to_string_lossy().to_string())
        .unwrap_or_else(|| path.display().to_string())
}

fn sorted_presets(es: &EditorState) -> Vec<PathBuf> {
    let mut paths = es.presets.clone();
    paths.sort_by(|a, b| {
        preset_name(a)
            .to_lowercase()
            .cmp(&preset_name(b).to_lowercase())
    });
    paths
}

fn row_context_menu(response: &egui::Response, es: &mut EditorState, path: &Path) {
    response.context_menu(|ui| {
        if ui.button("Delete…").clicked() {
            es.delete_confirm = Some(path.to_path_buf());
            ui.close_menu();
        }
        if ui.button("Show in Folder").clicked() {
            show_in_folder(es, path);
            ui.close_menu();
        }
    });
}

/// The modal load dialog: three column-major alphabetical columns, OK/Cancel,
/// double-click to load, right-click for Delete / Show in Folder.
pub fn load_dialog(
    ctx: &egui::Context,
    es: &mut EditorState,
    params: &ChordzParams,
    setter: &ParamSetter,
) {
    let paths = sorted_presets(es);
    let mut load_now: Option<PathBuf> = None;
    let mut close = false;

    let modal = egui::Modal::new(egui::Id::new("load-preset")).show(ctx, |ui| {
        ui.set_width(640.0);
        ui.heading("Load Preset");
        ui.add_space(4.0);
        if paths.is_empty() {
            ui.label("No presets found.");
            ui.label(
                RichText::new(format!("Save presets to {}", preset_dir().display()))
                    .small()
                    .weak(),
            );
        } else {
            let rows = paths.len().div_ceil(3);
            egui::ScrollArea::vertical()
                .max_height(420.0)
                .show(ui, |ui| {
                    ui.horizontal_top(|ui| {
                        for column in 0..3 {
                            ui.allocate_ui_with_layout(
                                egui::Vec2::new(200.0, rows as f32 * 22.0),
                                egui::Layout::top_down(egui::Align::LEFT),
                                |ui| {
                                    for row in 0..rows {
                                        let index = column * rows + row;
                                        let Some(path) = paths.get(index) else {
                                            continue;
                                        };
                                        let name = preset_name(path);
                                        let response = ui.add(
                                            egui::Button::new(RichText::new(&name).size(12.0))
                                                .fill(if es.load_selected == Some(index) {
                                                    super::theme::TEAL_DIM
                                                } else {
                                                    egui::Color32::TRANSPARENT
                                                })
                                                .frame(false)
                                                .min_size(egui::Vec2::new(196.0, 20.0)),
                                        );
                                        if response.clicked() {
                                            es.load_selected = Some(index);
                                        }
                                        if response.double_clicked() {
                                            load_now = Some(path.clone());
                                        }
                                        row_context_menu(&response, es, path);
                                    }
                                },
                            );
                        }
                    });
                });
        }
        ui.add_space(8.0);
        ui.horizontal(|ui| {
            let ok_enabled = es.load_selected.is_some_and(|index| index < paths.len());
            if ui
                .add_enabled(ok_enabled, egui::Button::new("OK"))
                .clicked()
            {
                if let Some(index) = es.load_selected {
                    load_now = paths.get(index).cloned();
                }
            }
            if ui.button("Cancel").clicked() {
                close = true;
            }
        });
    });

    if let Some(path) = load_now {
        let mut state = params.chordz_state.write();
        load_path(es, &mut state, params, setter, &path);
        es.load_dialog = false;
    } else if close || modal.should_close() {
        es.load_dialog = false;
    }
}

pub fn save_dialog(ctx: &egui::Context, es: &mut EditorState, params: &ChordzParams) {
    let mut save = false;
    let mut close = false;

    let modal = egui::Modal::new(egui::Id::new("save-preset")).show(ctx, |ui| {
        ui.set_width(360.0);
        ui.heading("Save Preset");
        ui.add_space(4.0);
        ui.horizontal(|ui| {
            ui.label("Name");
            ui.text_edit_singleline(&mut es.save_name);
        });
        ui.horizontal(|ui| {
            ui.label("Format");
            ComboBox::from_id_salt("save-format")
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
        });
        ui.add_space(8.0);
        ui.horizontal(|ui| {
            let name_ok = !es.save_name.trim().is_empty();
            if ui.add_enabled(name_ok, egui::Button::new("Save")).clicked() {
                save = true;
            }
            if ui.button("Cancel").clicked() {
                close = true;
            }
        });
    });

    if save {
        let mut snapshot = params.chordz_state.read().clone();
        let setups: [NoteSetup; NUM_NOTE_SLOTS] = params.setups();
        snapshot.note_setup = setups;
        let name = es.save_name.trim().to_string();
        let preset = Preset::new(name.clone(), snapshot, setups);
        match save_preset_as(&preset, &preset_dir(), es.preset_format) {
            Ok(path) => {
                es.message = format!("Saved {}", path.display());
                es.presets = list_presets(&preset_dir());
                es.preset_name = name;
                es.save_dialog = false;
            }
            Err(err) => es.message = err,
        }
    } else if close || modal.should_close() {
        es.save_dialog = false;
    }
}

pub fn delete_confirm(ctx: &egui::Context, es: &mut EditorState) {
    let Some(path) = es.delete_confirm.clone() else {
        return;
    };
    let mut delete = false;
    let mut close = false;
    let modal = egui::Modal::new(egui::Id::new("delete-preset")).show(ctx, |ui| {
        ui.set_width(320.0);
        ui.heading("Delete Preset");
        ui.add_space(4.0);
        ui.label(format!("Delete \"{}\"?", preset_name(&path)));
        ui.label(
            RichText::new("This removes the file from disk.")
                .small()
                .weak(),
        );
        ui.add_space(8.0);
        ui.horizontal(|ui| {
            if ui.button("Delete").clicked() {
                delete = true;
            }
            if ui.button("Cancel").clicked() {
                close = true;
            }
        });
    });
    if delete {
        match std::fs::remove_file(&path) {
            Ok(()) => {
                es.message = format!("Deleted {}", path.display());
                es.presets = list_presets(&preset_dir());
                es.midi_files = list_midi_files(&midi_dir());
                es.load_selected = None;
                es.midi_selected = None;
                es.midi_analysis = None;
            }
            Err(err) => es.message = format!("Could not delete: {err}"),
        }
        es.delete_confirm = None;
    } else if close || modal.should_close() {
        es.delete_confirm = None;
    }
}

/// The slim Presets tab: save button, preset list, data-folder info.
pub fn tab(
    ui: &mut egui::Ui,
    es: &mut EditorState,
    state: &mut ChordzState,
    params: &ChordzParams,
    setter: &ParamSetter,
) {
    egui::ScrollArea::vertical().show(ui, |ui| {
        ui.horizontal(|ui| {
            if ui.button("Save Preset…").clicked() {
                es.save_name = es.preset_name.clone();
                es.save_dialog = true;
            }
            if ui.button("Load Preset…").clicked() {
                es.presets = list_presets(&preset_dir());
                es.load_selected = None;
                es.load_dialog = true;
            }
            if ui
                .button("Refresh")
                .on_hover_text("Re-read the preset and MIDI folders")
                .clicked()
            {
                es.presets = list_presets(&preset_dir());
                es.midi_files = list_midi_files(&midi_dir());
            }
        });
        ui.label(
            RichText::new(format!("Folder: {}", preset_dir().display()))
                .small()
                .weak(),
        );
        ui.separator();

        let paths = sorted_presets(es);
        if paths.is_empty() {
            ui.label("No presets saved yet.");
        } else {
            egui::ScrollArea::vertical()
                .id_salt("preset-list")
                .max_height(200.0)
                .show(ui, |ui| {
                    for path in &paths {
                        ui.horizontal(|ui| {
                            let name = preset_name(path);
                            let response = ui.label(&name);
                            let extension = path
                                .extension()
                                .map(|ext| ext.to_string_lossy().to_string())
                                .unwrap_or_default();
                            ui.label(RichText::new(format!(".{extension}")).small().weak());
                            if ui.button("Load").clicked() {
                                load_path(es, state, params, setter, path);
                            }
                            row_context_menu(&response, es, path);
                        });
                    }
                });
        }

        ui.add_space(8.0);
        midi_section(ui, es, state);

        ui.add_space(8.0);
        ui.separator();
        ui.label(
            RichText::new(format!(
                "Chords.txt and Scale Templates.txt live in {}. Use \"Reload library\" \
             in the menu after editing them.",
                es.data_dir.display()
            ))
            .small()
            .weak(),
        );
    });
}

// ---------------------------------------------------------------------------
// MIDI progressions
// ---------------------------------------------------------------------------

fn select_midi(es: &mut EditorState, index: usize) {
    let Some(path) = es.midi_files.get(index).cloned() else {
        return;
    };
    es.midi_selected = Some(index);
    let name = preset_name(&path);
    match std::fs::read(&path) {
        Ok(bytes) => match analyze_midi(
            &bytes,
            &path.file_name().unwrap_or_default().to_string_lossy(),
            &es.library,
        ) {
            Ok(analysis) => {
                let (tonic, minor) = analysis.key();
                es.midi_tonic = tonic;
                es.midi_minor = minor;
                es.midi_analysis = Some(analysis);
            }
            Err(err) => {
                es.midi_analysis = None;
                es.message = format!("{name}: {err}");
            }
        },
        Err(err) => {
            es.midi_analysis = None;
            es.message = format!("{name}: {err}");
        }
    }
}

fn load_midi(es: &mut EditorState, state: &mut ChordzState) {
    let Some(analysis) = es.midi_analysis.clone() else {
        return;
    };
    apply_analysis(&analysis, es.midi_tonic, state);
    es.message = format!("Loaded MIDI progression \"{}\"", analysis.name);
    es.selection = Some(Selection::Degree(0));
    es.template_choice = None;
}

fn midi_section(ui: &mut egui::Ui, es: &mut EditorState, state: &mut ChordzState) {
    ui.separator();
    ui.heading("MIDI Progressions");

    // Files dropped onto the editor window are imported into the Midi folder.
    let dropped: Vec<PathBuf> = ui.ctx().input(|i| {
        i.raw
            .dropped_files
            .iter()
            .filter_map(|file| file.path.clone())
            .collect()
    });
    if !dropped.is_empty() {
        for path in dropped {
            match import_midi_file(&path, &midi_dir()) {
                Ok(target) => es.message = format!("Imported {}", target.display()),
                Err(err) => es.message = err,
            }
        }
        es.midi_files = list_midi_files(&midi_dir());
    }

    ui.horizontal(|ui| {
        ui.label("Import");
        ui.add(
            egui::TextEdit::singleline(&mut es.midi_import_path)
                .desired_width(280.0)
                .hint_text("/path/to/progression.mid"),
        );
        if ui.button("Import").clicked() {
            let path = PathBuf::from(es.midi_import_path.trim());
            match import_midi_file(&path, &midi_dir()) {
                Ok(target) => {
                    es.message = format!("Imported {}", target.display());
                    es.midi_import_path.clear();
                    es.midi_files = list_midi_files(&midi_dir());
                }
                Err(err) => es.message = err,
            }
        }
        ui.label(
            RichText::new("or drop .mid files onto this window")
                .small()
                .weak(),
        );
    });
    ui.label(
        RichText::new(format!("Folder: {}", midi_dir().display()))
            .small()
            .weak(),
    );
    ui.add_space(4.0);

    ui.horizontal_top(|ui| {
        ui.allocate_ui_with_layout(
            egui::Vec2::new(300.0, 240.0),
            egui::Layout::top_down(egui::Align::LEFT),
            |ui| {
                egui::ScrollArea::vertical()
                    .id_salt("midi-list")
                    .max_height(240.0)
                    .show(ui, |ui| {
                        if es.midi_files.is_empty() {
                            ui.label("No MIDI files yet.");
                        }
                        let files = es.midi_files.clone();
                        for (index, path) in files.iter().enumerate() {
                            let name = preset_name(path);
                            let selected = es.midi_selected == Some(index);
                            let response = ui.add(
                                egui::Button::new(RichText::new(&name).size(12.0))
                                    .fill(if selected {
                                        super::theme::TEAL_DIM
                                    } else {
                                        egui::Color32::TRANSPARENT
                                    })
                                    .frame(false)
                                    .min_size(egui::Vec2::new(296.0, 20.0)),
                            );
                            if response.clicked() {
                                select_midi(es, index);
                            }
                            if response.double_clicked() {
                                select_midi(es, index);
                                load_midi(es, state);
                            }
                            row_context_menu(&response, es, path);
                        }
                    });
            },
        );

        ui.allocate_ui_with_layout(
            egui::Vec2::new(ui.available_width().max(200.0), 240.0),
            egui::Layout::top_down(egui::Align::LEFT),
            |ui| {
                let Some(analysis) = es.midi_analysis.clone() else {
                    ui.label(
                        RichText::new(
                            "Select a MIDI file to preview its chords, then load them \
                             into the chord setup. Double-click loads immediately.",
                        )
                        .weak(),
                    );
                    return;
                };
                let source = if analysis.filename_key.is_some() {
                    "from filename"
                } else {
                    "estimated"
                };
                ui.label(RichText::new(format!(
                    "{} · {} · key {} {} ({source})",
                    analysis.name,
                    analysis
                        .tempo_bpm
                        .map(|bpm| format!("{bpm:.0} BPM"))
                        .unwrap_or_else(|| "no tempo".to_string()),
                    note_name(es.midi_tonic),
                    if es.midi_minor { "minor" } else { "major" },
                )));
                ui.horizontal(|ui| {
                    ui.label("Tonic");
                    ComboBox::from_id_salt("midi-tonic")
                        .selected_text(note_name(es.midi_tonic))
                        .width(60.0)
                        .show_ui(ui, |ui| {
                            for pc in 0u8..12 {
                                if ui
                                    .selectable_label(es.midi_tonic == pc, note_name(pc))
                                    .clicked()
                                {
                                    es.midi_tonic = pc;
                                }
                            }
                        });
                    ui.label("Mode");
                    let mut minor = es.midi_minor;
                    ComboBox::from_id_salt("midi-mode")
                        .selected_text(if minor { "Minor" } else { "Major" })
                        .width(80.0)
                        .show_ui(ui, |ui| {
                            ui.selectable_value(&mut minor, false, "Major");
                            ui.selectable_value(&mut minor, true, "Minor");
                        });
                    if minor != es.midi_minor {
                        es.midi_minor = minor;
                        // Re-estimate the tonic assuming the chosen mode.
                        if let Some(path) =
                            es.midi_selected.and_then(|i| es.midi_files.get(i)).cloned()
                        {
                            if let Ok(bytes) = std::fs::read(&path) {
                                if let Ok(midi) = parse_midi(&bytes) {
                                    es.midi_tonic = estimate_tonic_for_mode(&midi.note_ons, minor);
                                }
                            }
                        }
                    }
                });
                ui.add_space(4.0);
                egui::ScrollArea::vertical()
                    .id_salt("midi-chords")
                    .max_height(140.0)
                    .show(ui, |ui| {
                        for (index, chord) in analysis.chords.iter().enumerate() {
                            let offsets: Vec<String> = chord
                                .offsets
                                .iter()
                                .map(|offset| format!("{offset:+}"))
                                .collect();
                            ui.label(
                                RichText::new(format!(
                                    "{}. {}  {}  {}",
                                    index + 1,
                                    note_name(chord.root),
                                    chord.chord_id,
                                    offsets.join(" ")
                                ))
                                .monospace()
                                .size(11.0),
                            );
                        }
                    });
                if analysis.truncated {
                    ui.label(
                        RichText::new("More than 12 distinct chords; the tail was cut.")
                            .small()
                            .weak(),
                    );
                }
                if ui.button("Load into Chord Setup").clicked() {
                    load_midi(es, state);
                }
            },
        );
    });
}
