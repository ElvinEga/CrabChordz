//! The Chord Setup page: trigger keyboard, scale/template controls and the
//! five-octave chord-note keyboard, laid out like the original Chordz window.

use chordz_core::data::MenuEntry;
use chordz_core::model::{note_name, ChordNote, ChordzState, MAX_CHORD_NOTES, NUM_NOTE_SLOTS};
use nih_plug_egui::egui::{self, Align2, ComboBox, FontId, Pos2, RichText, Sense, Stroke, Vec2};

use super::theme;
use super::widgets::{self, KeyboardLayout};
use super::{EditorState, Selection};
use crate::params::ChordzParams;

pub fn page(
    ui: &mut egui::Ui,
    es: &mut EditorState,
    state: &mut ChordzState,
    params: &ChordzParams,
) {
    if es.selection.is_none() && !state.triggers.is_empty() {
        es.selection = Some(Selection::Degree(0));
    }

    ui.horizontal_top(|ui| {
        let right_width = 264.0;
        let main_width = ui.available_width() - right_width - 12.0;
        ui.allocate_ui_with_layout(
            Vec2::new(main_width, ui.available_height()),
            egui::Layout::top_down(egui::Align::LEFT),
            |ui| {
                controls_row(ui, es, state);
                ui.add_space(8.0);
                trigger_keyboard(ui, es, state, params);
                ui.add_space(4.0);
                ui.label(
                    RichText::new(
                        "Click: select · double-click: rename · right-click: edit · \
                         Ctrl+click: preview · hover: preview",
                    )
                    .small()
                    .weak(),
                );
                ui.add_space(8.0);
                chord_type_button(ui, es, state);
                ui.add_space(4.0);
                chord_keyboard(ui, es, state, params);
            },
        );
        ui.allocate_ui_with_layout(
            Vec2::new(right_width, ui.available_height()),
            egui::Layout::top_down(egui::Align::LEFT),
            |ui| right_column(ui, es, state, params),
        );
    });

    if let Some(selection) = es.selection {
        if let Some(trigger) = selection.get(state) {
            es.info = describe_trigger(trigger);
        }
    }
}

pub fn describe_trigger(trigger: &chordz_core::model::Trigger) -> String {
    let offsets: Vec<String> = trigger
        .notes
        .iter()
        .map(|note| format!("{:+}", note.semitone))
        .collect();
    let id = if trigger.chord_id.is_empty() {
        "(no chord)".to_string()
    } else {
        trigger.chord_id.clone()
    };
    format!("{} {}  {}", note_name(trigger.root), id, offsets.join(" "))
}

fn controls_row(ui: &mut egui::Ui, es: &mut EditorState, state: &mut ChordzState) {
    ui.horizontal_wrapped(|ui| {
        ui.label("Tonic");
        ComboBox::from_id_salt("tonic")
            .selected_text(RichText::new(note_name(state.tonic)).strong())
            .width(60.0)
            .show_ui(ui, |ui| {
                for pc in 0u8..12 {
                    if ui
                        .selectable_label(state.tonic == pc, note_name(pc))
                        .clicked()
                    {
                        super::set_tonic(state, pc);
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
        ui.label("Template");
        let staged = es
            .template_choice
            .and_then(|index| es.library.templates.get(index))
            .map(|template| template.name.clone());
        let label = staged.unwrap_or_else(|| {
            if state.template_name.is_empty() {
                "Custom".to_string()
            } else {
                state.template_name.clone()
            }
        });
        ComboBox::from_id_salt("template")
            .selected_text(label)
            .width(220.0)
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
                                MenuEntry::Item(index) => {
                                    let template = &es.library.templates[*index];
                                    if ui
                                        .selectable_label(
                                            es.template_choice == Some(*index),
                                            &template.name,
                                        )
                                        .clicked()
                                    {
                                        es.template_choice = Some(*index);
                                    }
                                }
                            }
                        }
                    });
            });
        let apply_enabled = es
            .template_choice
            .is_some_and(|index| index < es.library.templates.len());
        if ui
            .add_enabled(apply_enabled, egui::Button::new("Apply"))
            .on_hover_text("Replace the triggers with the selected template")
            .clicked()
        {
            if let Some(index) = es.template_choice {
                let template = es.library.templates[index].clone();
                state.apply_template(&template);
                es.template_choice = None;
                es.selection = Some(Selection::Degree(0));
                es.message = format!("Applied template \"{}\"", template.name);
            }
        }

        ui.separator();
        ui.checkbox(&mut state.easy_mode, "Easy Mode")
            .on_hover_text(
                "White keys play the first seven scale degrees; extra degrees and \
                 variations move to the black keys.",
            );
    });
}

fn right_column(
    ui: &mut egui::Ui,
    es: &mut EditorState,
    state: &mut ChordzState,
    params: &ChordzParams,
) {
    ui.group(|ui| {
        ui.label(RichText::new("Octave Range").strong());
        ui.horizontal(|ui| {
            ui.label("From");
            ui.add(egui::DragValue::new(&mut state.octave_from).range(-2..=8));
            ui.label("To");
            ui.add(egui::DragValue::new(&mut state.octave_to).range(-2..=8));
        });
        ui.add_space(4.0);
        ui.label(RichText::new("Transpose Octave").strong());
        ui.horizontal(|ui| {
            ui.label("Chords");
            ui.add(egui::DragValue::new(&mut state.transpose_octave).range(-4..=4))
                .on_hover_text("Octaves added to every generated chord note");
        });
        ui.horizontal(|ui| {
            ui.label("Below");
            ui.add(egui::DragValue::new(&mut state.transpose_below).range(-4..=4));
            ui.label("Above");
            ui.add(egui::DragValue::new(&mut state.transpose_above).range(-4..=4));
        });
        ui.add_space(4.0);
        ui.checkbox(&mut state.follow_trigger_notes, "Follow Trigger Notes")
            .on_hover_text("Notes outside the chord zone are snapped to the scale.");
    });
    ui.add_space(8.0);
    let can_preview = es
        .selection
        .and_then(|selection| selection.get(state))
        .is_some_and(|trigger| trigger.enabled);
    if ui
        .add_enabled(can_preview, egui::Button::new("Preview Chord"))
        .clicked()
    {
        if let Some(selection) = es.selection {
            params.request_preview(selection.variation(), selection.index());
        }
    }
    ui.add_space(8.0);
    ui.label(
        RichText::new(
            "Notes outside the octave range pass through as single notes, \
             transposed by the Below/Above values.",
        )
        .small()
        .weak(),
    );
}

// ---------------------------------------------------------------------------
// Trigger keyboard (one octave)
// ---------------------------------------------------------------------------

fn trigger_keyboard(
    ui: &mut egui::Ui,
    es: &mut EditorState,
    state: &mut ChordzState,
    params: &ChordzParams,
) {
    let width = ui.available_width();
    let (rect, _) = ui.allocate_exact_size(Vec2::new(width, 120.0), Sense::hover());
    let layout = KeyboardLayout::new(rect, 1);
    let painter = ui.painter_at(rect);
    let ctrl = ui.input(|i| i.modifiers.ctrl);

    let mut preview: Option<(bool, usize)> = None;
    let mut any_hover = false;
    let mut order: Vec<usize> = (0..12).filter(|key| !layout.is_black(*key)).collect();
    order.extend((0..12).filter(|key| layout.is_black(*key)));

    for key in order {
        let key_rect = layout.key_rect(key);
        let black = layout.is_black(key);
        let note = 60 + key as u8;
        let resolution = chordz_core::resolve(state, note);
        let trigger = resolution.and_then(|r| {
            if r.variation {
                state.variations.get(r.index)
            } else {
                state.triggers.get(r.index)
            }
        });
        let assigned = trigger.is_some_and(|t| t.enabled);
        let selected = resolution.is_some_and(|r| {
            es.selection
                == Some(if r.variation {
                    Selection::Variation(r.index)
                } else {
                    Selection::Degree(r.index)
                })
        });

        let response = ui.interact(key_rect, ui.id().with(("trigger-key", key)), Sense::click());
        let hovered = response.hovered();
        let fill = match (black, assigned, hovered) {
            (false, true, true) => theme::TEAL_BRIGHT,
            (false, true, false) => theme::TEAL,
            (false, false, true) => theme::WHITE_KEY_HOVER,
            (false, false, false) => theme::WHITE_KEY,
            (true, true, true) => theme::TEAL,
            (true, true, false) => theme::TEAL_DIM,
            (true, false, true) => theme::BLACK_KEY_HOVER,
            (true, false, false) => theme::BLACK_KEY,
        };
        widgets::paint_key(&painter, key_rect, fill, 2);
        if selected {
            painter.rect_stroke(
                key_rect.shrink(1.0),
                egui::CornerRadius::same(2),
                Stroke::new(2.0_f32, theme::TEAL_BRIGHT),
                egui::StrokeKind::Outside,
            );
        }

        if let Some(trigger) = trigger {
            let text_color = if assigned {
                if black {
                    theme::TEXT
                } else {
                    theme::TEXT_ON_TEAL
                }
            } else {
                theme::KEY_TEXT
            };
            if !trigger.chord_id.is_empty() {
                painter.text(
                    Pos2::new(
                        key_rect.center().x,
                        if black {
                            key_rect.center().y - 6.0
                        } else {
                            key_rect.top() + 8.0
                        },
                    ),
                    Align2::CENTER_TOP,
                    &trigger.chord_id,
                    FontId::proportional(if black { 8.0 } else { 11.0 }),
                    text_color,
                );
            }
            if !black {
                painter.text(
                    Pos2::new(key_rect.center().x, key_rect.bottom() - 5.0),
                    Align2::CENTER_BOTTOM,
                    note_name(trigger.root),
                    FontId::proportional(10.0),
                    text_color,
                );
            }
        }

        let Some(resolution) = resolution else {
            continue;
        };
        let selection = if resolution.variation {
            Selection::Variation(resolution.index)
        } else {
            Selection::Degree(resolution.index)
        };

        if hovered {
            any_hover = true;
            ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
            if es.hover_key != Some(key) {
                es.hover_key = Some(key);
                preview = Some((resolution.variation, resolution.index));
            }
        }
        if response.clicked() {
            if ctrl {
                preview = Some((resolution.variation, resolution.index));
            } else {
                es.selection = Some(selection);
            }
        }
        if response.double_clicked() && assigned {
            if let Some(trigger) = selection.get(state) {
                es.id_edit = Some((selection, trigger.chord_id.clone()));
            }
        }

        let mut root_change: Option<u8> = None;
        let mut paste = false;
        let mut clear = false;
        let mut edit_id = false;
        response.context_menu(|ui| {
            ui.menu_button("Root", |ui| {
                for pc in 0u8..12 {
                    if ui.button(note_name(pc)).clicked() {
                        root_change = Some(pc);
                        ui.close_menu();
                    }
                }
            });
            if ui.button("Chord ID…").clicked() {
                edit_id = true;
                ui.close_menu();
            }
            if ui.button("Copy").clicked() {
                es.clipboard = selection.get(state).cloned();
                ui.close_menu();
            }
            if ui
                .add_enabled(es.clipboard.is_some(), egui::Button::new("Paste"))
                .clicked()
            {
                paste = true;
                ui.close_menu();
            }
            if ui.button("Clear").clicked() {
                clear = true;
                ui.close_menu();
            }
        });
        if let Some(root) = root_change {
            if let Some(trigger) = selection.get_mut(state) {
                trigger.set_root(root);
            }
        }
        if edit_id {
            if let Some(trigger) = selection.get(state) {
                es.id_edit = Some((selection, trigger.chord_id.clone()));
            }
        }
        if paste {
            if let (Some(source), Some(target)) = (es.clipboard.clone(), selection.get_mut(state)) {
                target.notes = source.notes;
                target.set_chord_id(&source.chord_id);
            }
        }
        if clear {
            if let Some(trigger) = selection.get_mut(state) {
                trigger.notes.clear();
                trigger.chord_id.clear();
            }
        }
    }

    if !any_hover {
        es.hover_key = None;
    }
    if let Some((variation, index)) = preview {
        params.request_preview(variation, index);
    }
}

// ---------------------------------------------------------------------------
// Chord-note keyboard (five octaves)
// ---------------------------------------------------------------------------

fn chord_type_button(ui: &mut egui::Ui, es: &mut EditorState, state: &mut ChordzState) {
    let Some(selection) = es.selection else {
        return;
    };
    let current = selection
        .get(state)
        .map(|trigger| trigger.chord_id.clone())
        .unwrap_or_default();
    let label = if current.is_empty() {
        "Assign chord ▾".to_string()
    } else {
        format!("{current} ▾")
    };
    ui.menu_button(label, |ui| {
        egui::ScrollArea::vertical()
            .max_height(320.0)
            .show(ui, |ui| {
                for entry in &es.library.chord_menu {
                    match entry {
                        MenuEntry::Group(name) => {
                            ui.label(RichText::new(name).small().weak());
                        }
                        MenuEntry::Separator => {
                            ui.separator();
                        }
                        MenuEntry::Item(index) => {
                            let chord = &es.library.chords[*index];
                            if ui.selectable_label(false, &chord.name).clicked() {
                                if let Some(trigger) = selection.get_mut(state) {
                                    trigger.notes = chord.notes.clone();
                                    trigger.set_chord_id(&chord.chord_id);
                                }
                                ui.close_menu();
                            }
                        }
                    }
                }
            });
    });
}

fn chord_keyboard(
    ui: &mut egui::Ui,
    es: &mut EditorState,
    state: &mut ChordzState,
    params: &ChordzParams,
) {
    let Some(selection) = es.selection else {
        ui.label("Select a trigger key to edit its chord notes.");
        return;
    };
    let Some(trigger) = selection.get(state) else {
        return;
    };
    let root = trigger.root;

    let width = ui.available_width();
    let (area, _) = ui.allocate_exact_size(Vec2::new(width, 190.0), Sense::hover());
    let painter = ui.painter_at(area);
    for octave in 0..5 {
        let label = match octave {
            0 => "-2",
            1 => "-1",
            2 => "0",
            3 => "+1",
            _ => "+2",
        };
        let x = area.left() + octave as f32 * (area.width() / 5.0);
        painter.text(
            Pos2::new(x + 2.0, area.top()),
            Align2::LEFT_TOP,
            label,
            FontId::proportional(10.0),
            theme::TEXT_WEAK,
        );
    }
    let keys_rect = egui::Rect::from_min_size(
        Pos2::new(area.left(), area.top() + 16.0),
        Vec2::new(area.width(), area.height() - 16.0),
    );
    let layout = KeyboardLayout::new(keys_rect, 5);
    let ctrl = ui.input(|i| i.modifiers.ctrl);

    let mut order: Vec<usize> = (0..60).filter(|key| !layout.is_black(*key)).collect();
    order.extend((0..60).filter(|key| layout.is_black(*key)));

    for key in order {
        let key_rect = layout.key_rect(key);
        let black = layout.is_black(key);
        let absolute = layout.absolute(key);
        let semitone = absolute - root as i16;
        let note_index = selection
            .get(state)
            .and_then(|trigger| trigger.notes.iter().position(|n| n.semitone == semitone));

        let response = ui.interact(key_rect, ui.id().with(("chord-key", key)), Sense::click());
        let fill = match (note_index.is_some(), black, response.hovered()) {
            (true, false, true) => theme::TEAL_BRIGHT,
            (true, false, false) => theme::TEAL,
            (true, true, true) => theme::TEAL,
            (true, true, false) => theme::TEAL_DIM,
            (false, _, true) => {
                if black {
                    theme::BLACK_KEY_HOVER
                } else {
                    theme::WHITE_KEY_HOVER
                }
            }
            (false, _, false) => {
                if black {
                    theme::BLACK_KEY
                } else {
                    theme::WHITE_KEY
                }
            }
        };
        widgets::paint_key(&painter, key_rect, fill, 2);
        if semitone == 0 {
            painter.rect_stroke(
                key_rect.shrink(1.0),
                egui::CornerRadius::same(2),
                Stroke::new(2.0_f32, theme::TEAL_BRIGHT),
                egui::StrokeKind::Outside,
            );
        }

        if let Some(note_index) = note_index {
            let slot = selection
                .get(state)
                .map(|trigger| trigger.notes[note_index].note_no)
                .unwrap_or(1);
            let text_color = if black {
                theme::TEXT
            } else {
                theme::TEXT_ON_TEAL
            };
            painter.text(
                Pos2::new(key_rect.center().x, key_rect.top() + 4.0),
                Align2::CENTER_TOP,
                slot.to_string(),
                FontId::proportional(if black { 8.0 } else { 10.0 }),
                text_color,
            );
            if !black {
                let pc = (root as i16 + semitone).rem_euclid(12) as u8;
                painter.text(
                    Pos2::new(key_rect.center().x, key_rect.bottom() - 4.0),
                    Align2::CENTER_BOTTOM,
                    note_name(pc),
                    FontId::proportional(9.0),
                    text_color,
                );
            }
        }

        if response.clicked() {
            if ctrl {
                let midi = (60 + absolute).clamp(0, 127) as u8;
                params.request_note_preview(midi);
            } else if let Some(trigger) = selection.get_mut(state) {
                if let Some(index) = note_index {
                    trigger.notes.remove(index);
                } else if trigger.notes.len() < MAX_CHORD_NOTES {
                    let slot = (trigger.notes.len() % NUM_NOTE_SLOTS + 1) as u8;
                    trigger.notes.push(ChordNote::new(semitone, slot));
                    trigger.notes.sort_by_key(|note| note.semitone);
                }
            }
        }

        if note_index.is_some() {
            let mut new_slot: Option<u8> = None;
            let mut remove = false;
            response.context_menu(|ui| {
                ui.menu_button("Slot", |ui| {
                    for slot in 1..=NUM_NOTE_SLOTS as u8 {
                        if ui.button(format!("Note {slot}")).clicked() {
                            new_slot = Some(slot);
                            ui.close_menu();
                        }
                    }
                });
                if ui.button("Remove note").clicked() {
                    remove = true;
                    ui.close_menu();
                }
            });
            if let Some(trigger) = selection.get_mut(state) {
                if let Some(slot) = new_slot {
                    if let Some(note) = trigger.notes.get_mut(note_index.unwrap()) {
                        note.note_no = slot;
                    }
                }
                if remove {
                    if let Some(index) = note_index {
                        trigger.notes.remove(index);
                    }
                }
            }
        }
    }
}
