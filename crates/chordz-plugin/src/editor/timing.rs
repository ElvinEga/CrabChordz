//! The Timing / Velocity page: five slot columns with delay slider boxes and
//! velocity knobs, plus the global timing flags.

use chordz_core::model::{ChordzState, NUM_NOTE_SLOTS};
use nih_plug::prelude::ParamSetter;
use nih_plug_egui::egui::{self, RichText, Vec2};

use super::widgets::{self, ParamKind};
use super::EditorState;
use crate::params::ChordzParams;

pub fn page(
    ui: &mut egui::Ui,
    es: &mut EditorState,
    setter: &ParamSetter,
    params: &ChordzParams,
    state: &mut ChordzState,
) {
    ui.add_space(4.0);
    ui.horizontal(|ui| {
        for slot in 0..NUM_NOTE_SLOTS {
            ui.allocate_ui_with_layout(
                Vec2::new(212.0, 330.0),
                egui::Layout::top_down(egui::Align::LEFT),
                |ui| {
                    ui.label(RichText::new(format!("Note {}", slot + 1)).strong());
                    ui.add_space(6.0);
                    widgets::delay_slider(
                        ui,
                        setter,
                        params,
                        slot,
                        ParamKind::Start,
                        ParamKind::StartRand,
                        "Start",
                        &mut es.info,
                        &mut es.value_entry,
                    );
                    ui.add_space(4.0);
                    widgets::delay_slider(
                        ui,
                        setter,
                        params,
                        slot,
                        ParamKind::End,
                        ParamKind::EndRand,
                        "End",
                        &mut es.info,
                        &mut es.value_entry,
                    );
                    ui.add_space(8.0);
                    ui.horizontal(|ui| {
                        widgets::knob(
                            ui,
                            setter,
                            params,
                            slot,
                            ParamKind::Vel,
                            "Velocity",
                            &mut es.info,
                            &mut es.value_entry,
                        );
                        widgets::knob(
                            ui,
                            setter,
                            params,
                            slot,
                            ParamKind::VelRand,
                            "Vel Random",
                            &mut es.info,
                            &mut es.value_entry,
                        );
                    });
                },
            );
            if slot + 1 < NUM_NOTE_SLOTS {
                ui.separator();
            }
        }
    });

    ui.add_space(10.0);
    ui.separator();
    ui.horizontal(|ui| {
        ui.checkbox(&mut state.relative_timing, "Relative Timing")
            .on_hover_text("Start and end delays accumulate across the chord notes.");
        ui.checkbox(&mut state.same_random_velocity, "Same Random Velocity")
            .on_hover_text("All notes in a chord share one random velocity draw.");
        ui.checkbox(&mut state.random_note_assignment, "Random Note Assignment")
            .on_hover_text("Assign each chord note to a random timing slot.");
    });
    ui.add_space(6.0);
    ui.label(
        RichText::new(
            "Drag sliders and knobs to edit · hold Alt to edit the random range · \
             Shift for fine control · double-click or right-click to enter a value · \
             Ctrl+click resets to the default.",
        )
        .small()
        .weak(),
    );
}
