//! Chordz-compatible chord generator core.
//!
//! The crate is deliberately free of any plugin-API or GUI dependency so the
//! engine can be unit tested and reused by VST3, CLAP or a standalone host.

pub mod data;
pub mod engine;
pub mod midi;
pub mod model;
pub mod preset;

pub use data::{builtin_library, load_library, Library, MenuEntry};
pub use engine::{describe_chord, resolve, snap_to_scale, Engine, OutputEvent, OutputKind};
pub use midi::{analyze_midi, apply_analysis, MidiAnalysis};
pub use model::{ChordNote, ChordzState, NoteSetup, Trigger, NUM_NOTE_SLOTS};
pub use preset::{
    list_presets, load_preset, load_preset_file, save_preset, save_preset_as, Preset, PresetFormat,
};
