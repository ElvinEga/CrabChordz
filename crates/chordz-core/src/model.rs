//! The serializable state of a Chordz instance.
//!
//! The model mirrors the fields of the original Delphi `TChordzSetup` class that
//! REA recovered from `Chordz.dll` (see the README for the evidence trail).

use serde::{Deserialize, Serialize};

pub const NUM_NOTE_SLOTS: usize = 5;
pub const MAX_CHORD_NOTES: usize = 16;
pub const MAX_CHORD_ID: usize = 12;

/// One note of a chord: a semitone offset relative to the chord root plus the
/// timing/velocity slot (1..=5) that controls it.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ChordNote {
    pub semitone: i16,
    pub note_no: u8,
}

impl ChordNote {
    pub fn new(semitone: i16, note_no: u8) -> Self {
        Self {
            semitone,
            note_no: note_no.clamp(1, NUM_NOTE_SLOTS as u8),
        }
    }
}

/// A scale degree that can trigger a chord.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Trigger {
    /// Root of the chord as an absolute pitch class (0 = C).
    pub root: u8,
    /// Short identifier shown on the trigger keyboard (max 12 chars).
    pub chord_id: String,
    pub notes: Vec<ChordNote>,
    /// Disabled triggers stay in the list so scale degrees keep their white-key
    /// positions. Chordz `.codefn42p` presets mark an unassigned trigger with
    /// root note `-1`.
    #[serde(default = "default_true")]
    pub enabled: bool,
}

fn default_true() -> bool {
    true
}

impl Default for Trigger {
    fn default() -> Self {
        Self {
            root: 0,
            chord_id: String::new(),
            notes: Vec::new(),
            enabled: true,
        }
    }
}

impl Trigger {
    pub fn set_root(&mut self, root: u8) {
        self.root = root % 12;
    }

    pub fn set_chord_id(&mut self, id: &str) {
        self.chord_id = id.chars().take(MAX_CHORD_ID).collect();
    }
}

/// Timing and velocity settings for one of the five note slots.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct NoteSetup {
    /// Start delay in milliseconds (0..1000).
    pub start_ms: f32,
    /// Additional random start delay in milliseconds (0..1000).
    pub start_random_ms: f32,
    /// End delay in milliseconds (0..1000).
    pub end_ms: f32,
    /// Additional random end delay in milliseconds (0..1000).
    pub end_random_ms: f32,
    /// Velocity scale in percent (-100..100).
    pub velocity_scale: f32,
    /// Additional random velocity scale in percent (0..100).
    pub velocity_random: f32,
}

impl Default for NoteSetup {
    fn default() -> Self {
        Self {
            start_ms: 0.0,
            start_random_ms: 0.0,
            end_ms: 0.0,
            end_random_ms: 0.0,
            velocity_scale: 0.0,
            velocity_random: 0.0,
        }
    }
}

impl NoteSetup {
    pub fn sanitized(mut self) -> Self {
        self.start_ms = self.start_ms.clamp(0.0, 1000.0);
        self.start_random_ms = self.start_random_ms.clamp(0.0, 1000.0);
        self.end_ms = self.end_ms.clamp(0.0, 1000.0);
        self.end_random_ms = self.end_random_ms.clamp(0.0, 1000.0);
        self.velocity_scale = self.velocity_scale.clamp(-100.0, 100.0);
        self.velocity_random = self.velocity_random.clamp(0.0, 100.0);
        self
    }
}

/// Complete plugin state. Serialized into the VST3/CLAP state chunk and into
/// JSON presets.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ChordzState {
    /// Scale tonic as a pitch class (0 = C).
    pub tonic: u8,
    /// One trigger per scale degree, in scale order. The first seven degrees
    /// sit on the white keys in easy mode, further degrees move onto the black
    /// keys (matching the original's documented behaviour).
    pub triggers: Vec<Trigger>,
    /// Optional black-key chord variations (C#, D#, F#, G#, A#), built from the
    /// part of a scale template after the `|` character.
    pub variations: Vec<Trigger>,
    pub easy_mode: bool,
    pub follow_trigger_notes: bool,
    /// First octave that triggers chords, using the user-facing octave numbering.
    pub octave_from: i8,
    /// Last octave that triggers chords.
    pub octave_to: i8,
    /// Chords are transposed by this many octaves.
    pub transpose_octave: i8,
    /// Notes below the trigger zone are transposed by this many octaves.
    pub transpose_below: i8,
    /// Notes above the trigger zone are transposed by this many octaves.
    pub transpose_above: i8,
    /// How octaves are numbered: -2 (MIDI note 0 = octave -2), -1 or 0.
    pub lowest_octave: i8,
    pub relative_timing: bool,
    pub same_random_velocity: bool,
    pub random_note_assignment: bool,
    pub note_setup: [NoteSetup; NUM_NOTE_SLOTS],
    /// Name of the last applied scale/chord template.
    pub template_name: String,
}

impl Default for ChordzState {
    fn default() -> Self {
        let mut state = Self {
            tonic: 0,
            triggers: Vec::new(),
            variations: Vec::new(),
            easy_mode: false,
            follow_trigger_notes: false,
            octave_from: -2,
            octave_to: 8,
            transpose_octave: 0,
            transpose_below: 0,
            transpose_above: 0,
            lowest_octave: -2,
            relative_timing: false,
            same_random_velocity: false,
            random_note_assignment: false,
            note_setup: [NoteSetup::default(); NUM_NOTE_SLOTS],
            template_name: String::new(),
        };
        // The manual describes the C major scale as the factory starting point.
        state.apply_template_by_name("Major");
        state
    }
}

impl ChordzState {
    /// Apply one of the factory scale/chord templates by name.
    pub fn apply_template_by_name(&mut self, name: &str) -> bool {
        let library = crate::data::builtin_library();
        match library.templates.iter().find(|t| t.name == name) {
            Some(template) => {
                self.apply_template(template);
                true
            }
            None => false,
        }
    }

    pub fn apply_template(&mut self, template: &crate::data::Template) {
        self.triggers = template
            .degrees
            .iter()
            .map(|degree| Trigger {
                root: (self.tonic + degree.offset) % 12,
                chord_id: degree.chord_id.clone(),
                notes: degree.notes.clone(),
                enabled: true,
            })
            .collect();

        self.variations = template
            .variations
            .iter()
            .map(|degree| Trigger {
                root: (self.tonic + degree.offset) % 12,
                chord_id: degree.chord_id.clone(),
                notes: degree.notes.clone(),
                enabled: true,
            })
            .collect();

        self.template_name = template.name.clone();
    }

    /// Transpose the scale (and every chord root) by the given number of
    /// semitones. This is the two transpose buttons below the trigger keyboard.
    pub fn transpose_tonic(&mut self, semitones: i16) {
        let shift = semitones.rem_euclid(12) as u8;
        self.tonic = (self.tonic + shift) % 12;
        for trigger in self.triggers.iter_mut().chain(self.variations.iter_mut()) {
            trigger.root = (trigger.root + shift) % 12;
        }
    }

    /// Scale degrees as semitone offsets from the tonic, in trigger order.
    pub fn scale_offsets(&self) -> Vec<u8> {
        self.triggers
            .iter()
            .map(|t| (t.root + 12 - self.tonic) % 12)
            .collect()
    }

    pub fn note_setup(&self, slot: u8) -> &NoteSetup {
        let index = (slot.clamp(1, NUM_NOTE_SLOTS as u8) - 1) as usize;
        &self.note_setup[index]
    }

    /// MIDI note number of the chord root in the octave of `note` (easy mode),
    /// or of the pressed note itself (easy mode disabled).
    pub fn octave_number(&self, note: u8) -> i32 {
        (note as i32) / 12 + self.lowest_octave as i32
    }

    /// Does the current setup contain a scale degree whose pitch class is `pc`?
    pub fn is_in_scale_pc(&self, pc: u8) -> bool {
        self.triggers
            .iter()
            .any(|t| t.enabled && t.root % 12 == pc % 12)
    }

    /// Names of every scale degree root, useful for the UI.
    pub fn degree_labels(&self) -> Vec<String> {
        self.triggers
            .iter()
            .map(|t| crate::model::note_name(t.root))
            .collect()
    }
}

pub const NOTE_NAMES_SHARP: [&str; 12] = [
    "C", "C#", "D", "D#", "E", "F", "F#", "G", "G#", "A", "A#", "B",
];

pub fn note_name(pc: u8) -> String {
    NOTE_NAMES_SHARP[(pc % 12) as usize].to_string()
}

pub fn note_name_with_octave(note: u8, lowest_octave: i8) -> String {
    format!(
        "{}{}",
        note_name(note % 12),
        (note as i32) / 12 + lowest_octave as i32
    )
}
