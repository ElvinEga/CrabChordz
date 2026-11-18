//! Preset handling: the load dialog (three alphabetical columns like the
//! original), the save dialog, delete confirmation, and the Presets tab.

use std::path::{Path, PathBuf};

use chordz_core::model::{ChordzState, NoteSetup, NUM_NOTE_SLOTS};
use chordz_core::preset::{
    list_presets, load_preset_file, preset_dir, save_preset_as, Preset, PresetFormat,
};
use nih_plug::prelude::ParamSetter;
use nih_plug_egui::egui::{self, ComboBox, RichText};

use super::EditorState;
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
                es.load_selected = None;
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
            .on_hover_text("Re-read the preset folder")
            .clicked()
        {
            es.presets = list_presets(&preset_dir());
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
        egui::ScrollArea::vertical().show(ui, |ui| {
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
}
