//! Host-visible parameters.
//!
//! The original Chordz exposes exactly 30 VST parameters: for each of the five
//! chord-note slots a Start, Start Random, End, End Random, Velocity Scale and
//! Velocity Random value. That layout is reproduced here so the automation
//! surface matches the Windows plugin (see the README for the REA evidence).
//!
//! Everything that is setup data rather than automation (tonic, scale template,
//! octave zones, easy mode, ...) lives in a persisted [`ChordzState`] chunk,
//! just like the original stores it in its VST program chunk.

use std::sync::atomic::AtomicU64;
use std::sync::Arc;

use chordz_core::model::{ChordzState, NoteSetup, NUM_NOTE_SLOTS};
use nih_plug::prelude::*;
use nih_plug_egui::EguiState;
use parking_lot::RwLock;

/// Timing and velocity parameters for one of the five note slots.
#[derive(Params)]
pub struct NoteSlotParams {
    /// Start delay in milliseconds.
    #[id = "start"]
    pub start_ms: FloatParam,
    /// Extra random start delay in milliseconds.
    #[id = "start_rand"]
    pub start_random_ms: FloatParam,
    /// End (release) delay in milliseconds.
    #[id = "end"]
    pub end_ms: FloatParam,
    /// Extra random end delay in milliseconds.
    #[id = "end_rand"]
    pub end_random_ms: FloatParam,
    /// Velocity scaling in percent.
    #[id = "vel"]
    pub velocity_scale: FloatParam,
    /// Extra random velocity scaling in percent.
    #[id = "vel_rand"]
    pub velocity_random: FloatParam,
}

impl NoteSlotParams {
    fn new(slot: usize) -> Self {
        let slot = slot + 1;
        Self {
            start_ms: FloatParam::new(
                format!("Note {slot} Start"),
                0.0,
                FloatRange::Linear {
                    min: 0.0,
                    max: 1000.0,
                },
            )
            .with_step_size(1.0)
            .with_unit(" ms"),
            start_random_ms: FloatParam::new(
                format!("Note {slot} Start Random"),
                0.0,
                FloatRange::Linear {
                    min: 0.0,
                    max: 1000.0,
                },
            )
            .with_step_size(1.0)
            .with_unit(" ms"),
            end_ms: FloatParam::new(
                format!("Note {slot} End"),
                0.0,
                FloatRange::Linear {
                    min: 0.0,
                    max: 1000.0,
                },
            )
            .with_step_size(1.0)
            .with_unit(" ms"),
            end_random_ms: FloatParam::new(
                format!("Note {slot} End Random"),
                0.0,
                FloatRange::Linear {
                    min: 0.0,
                    max: 1000.0,
                },
            )
            .with_step_size(1.0)
            .with_unit(" ms"),
            velocity_scale: FloatParam::new(
                format!("Note {slot} Velocity Scale"),
                0.0,
                FloatRange::Linear {
                    min: -100.0,
                    max: 100.0,
                },
            )
            .with_step_size(1.0)
            .with_unit(" %"),
            velocity_random: FloatParam::new(
                format!("Note {slot} Velocity Random"),
                0.0,
                FloatRange::Linear {
                    min: 0.0,
                    max: 100.0,
                },
            )
            .with_step_size(1.0)
            .with_unit(" %"),
        }
    }

    pub fn setup(&self) -> NoteSetup {
        NoteSetup {
            start_ms: self.start_ms.value(),
            start_random_ms: self.start_random_ms.value(),
            end_ms: self.end_ms.value(),
            end_random_ms: self.end_random_ms.value(),
            velocity_scale: self.velocity_scale.value(),
            velocity_random: self.velocity_random.value(),
        }
        .sanitized()
    }

    /// Write a setup back into the host parameters. Used when loading presets.
    pub fn apply(&self, setup: &NoteSetup, setter: &ParamSetter) {
        let setup = setup.sanitized();
        set_param(setter, &self.start_ms, setup.start_ms);
        set_param(setter, &self.start_random_ms, setup.start_random_ms);
        set_param(setter, &self.end_ms, setup.end_ms);
        set_param(setter, &self.end_random_ms, setup.end_random_ms);
        set_param(setter, &self.velocity_scale, setup.velocity_scale);
        set_param(setter, &self.velocity_random, setup.velocity_random);
    }
}

fn set_param(setter: &ParamSetter, param: &FloatParam, value: f32) {
    setter.begin_set_parameter(param);
    setter.set_parameter(param, value);
    setter.end_set_parameter(param);
}

/// A preview request from the editor to the audio thread.
///
/// The top 32 bits are a generation counter, bits 16..32 hold the variation
/// flag, and the low 16 bits hold the trigger index. A plain atomic keeps the
/// hand-off allocation free and lock free.
pub const PREVIEW_VARIATION_BIT: u64 = 1 << 16;

#[derive(Params)]
pub struct ChordzParams {
    /// Editor window state, saved with the plugin state so the window size
    /// survives a reload.
    #[persist = "editor-state"]
    pub editor_state: Arc<EguiState>,

    /// The complete chord setup. This is the Mac port's equivalent of the
    /// original's `TChordzSetup` program chunk.
    #[persist = "chordz-state"]
    pub chordz_state: Arc<RwLock<ChordzState>>,

    /// The 30 automation parameters, in the same order as the original.
    #[nested(array, group = "Note Settings")]
    pub slots: [NoteSlotParams; NUM_NOTE_SLOTS],

    /// Set by the editor, read by the audio thread.
    pub preview: Arc<AtomicU64>,

    /// Single-note preview (Ctrl+click on the chord keyboard). Same encoding
    /// as `preview` but the low bits hold a MIDI note number.
    pub preview_note: Arc<AtomicU64>,
}

impl ChordzParams {
    pub fn new() -> Self {
        Self {
            editor_state: EguiState::from_size(1160, 700),
            chordz_state: Arc::new(RwLock::new(ChordzState::default())),
            slots: std::array::from_fn(NoteSlotParams::new),
            preview: Arc::new(AtomicU64::new(0)),
            preview_note: Arc::new(AtomicU64::new(0)),
        }
    }

    /// Snapshot the timing/velocity parameters for the audio thread.
    pub fn setups(&self) -> [NoteSetup; NUM_NOTE_SLOTS] {
        std::array::from_fn(|index| self.slots[index].setup())
    }

    /// Apply a whole preset's timing values through the host parameter API.
    pub fn apply_setups(&self, setups: &[NoteSetup; NUM_NOTE_SLOTS], setter: &ParamSetter) {
        for (slot, setup) in self.slots.iter().zip(setups.iter()) {
            slot.apply(setup, setter);
        }
    }

    pub fn request_preview(&self, variation: bool, index: usize) {
        let generation = (self.preview.load(std::sync::atomic::Ordering::Relaxed) >> 32) + 1;
        let mut value = (generation << 32) | (index as u64 & 0xFFFF);
        if variation {
            value |= PREVIEW_VARIATION_BIT;
        }
        self.preview
            .store(value, std::sync::atomic::Ordering::Release);
    }

    /// Read a pending preview request, returning `(variation, index)` and
    /// clearing it. Called from the audio thread.
    pub fn take_preview(&self) -> Option<(bool, usize)> {
        let value = self.preview.swap(0, std::sync::atomic::Ordering::Acquire);
        if value == 0 {
            None
        } else {
            Some((
                value & PREVIEW_VARIATION_BIT != 0,
                (value & 0xFFFF) as usize,
            ))
        }
    }

    pub fn request_note_preview(&self, note: u8) {
        let generation = (self.preview_note.load(std::sync::atomic::Ordering::Relaxed) >> 32) + 1;
        self.preview_note.store(
            (generation << 32) | note as u64,
            std::sync::atomic::Ordering::Release,
        );
    }

    pub fn take_note_preview(&self) -> Option<u8> {
        let value = self
            .preview_note
            .swap(0, std::sync::atomic::Ordering::Acquire);
        if value == 0 {
            None
        } else {
            Some((value & 0xFF) as u8)
        }
    }
}

impl Default for ChordzParams {
    fn default() -> Self {
        Self::new()
    }
}
