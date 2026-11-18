//! Reader and writer for the original Chordz `.codefn42p` preset files.
//!
//! The file is a small wrapper around a Delphi `TWriter` token stream:
//!
//! ```text
//! "mmst"  u32 payload-length  "TPF0"  <Delphi TWriter tokens>
//! ```
//!
//! REA recovered the token layout from the four example presets shipped with
//! this project and from the `TChordzSetup` RTTI in `Chordz.dll`. The setup is
//! written as seven consecutive values:
//!
//! 0. active editor panel (ignored here)
//! 1. octave range/timing flags
//! 2. scale template name, template number, tonic, easy mode
//! 3. active trigger index (ignored here)
//! 4. twelve trigger entries, keyed by the C-major white/black key layout
//! 5. five note setups, six Delphi `Extended` floats each
//! 6. relative timing, random note assignment, same random velocity
//!
//! Chord notes are stored as semitone offsets from C, while the trigger's
//! `RootNote` is the pitch class that triggers the chord when easy mode is off.
//! [`ChordzState`] stores notes relative to the trigger root instead, so the
//! reader subtracts the root and the writer adds it back.

use std::collections::HashSet;

use crate::data::builtin_library;
use crate::model::{ChordNote, ChordzState, NoteSetup, Trigger, NUM_NOTE_SLOTS};

use super::{Preset, PRESET_FORMAT, PRESET_VERSION};

/// Extension used by the original plugin.
pub const EXTENSION: &str = "codefn42p";

const MAGIC: &[u8; 4] = b"mmst";
const FILER_SIGNATURE: &[u8; 4] = b"TPF0";
const STREAM_VERSION: &str = "1.03";

/// Pitch classes of the white keys in C-major order. In easy mode they address
/// the first seven scale degrees.
const WHITE_KEYS: [usize; 7] = [0, 2, 4, 5, 7, 9, 11];
/// Pitch classes of the black keys. They address chord variations, or scale
/// degrees eight through twelve for scales with more than seven notes.
const BLACK_KEYS: [usize; 5] = [1, 3, 6, 8, 10];

/// Is this byte slice a Chordz preset?
pub fn is_codefn42p(bytes: &[u8]) -> bool {
    bytes.len() >= 12 && &bytes[0..4] == MAGIC
}

/// Decode a `.codefn42p` file into the shared [`Preset`] model.
pub fn decode(bytes: &[u8]) -> Result<Preset, String> {
    if !is_codefn42p(bytes) {
        return Err("not a Chordz preset (missing mmst header)".to_string());
    }
    let payload_length = u32::from_le_bytes([bytes[4], bytes[5], bytes[6], bytes[7]]) as usize;
    let payload = &bytes[8..];
    if payload.len() < 4 {
        return Err("truncated Chordz preset header".to_string());
    }
    if &payload[0..4] != FILER_SIGNATURE {
        return Err(
            "compressed .codefn42p presets are not supported; disable preset compression in \
             Chordz and save the preset again"
                .to_string(),
        );
    }
    if payload_length > payload.len() {
        return Err(format!(
            "truncated Chordz preset: header says {payload_length} bytes, file has {}",
            payload.len()
        ));
    }

    let mut reader = Reader::new(&payload[4..]);
    let mut values = Vec::new();
    while !reader.is_empty() {
        values.push(reader.read_value(0)?);
    }
    if values.len() < 2 {
        return Err("Chordz preset has no setup data".to_string());
    }

    let meta = values[0].as_list("preset metadata")?;
    let name = meta
        .get(1)
        .map(|value| value.as_str("preset name"))
        .transpose()?
        .unwrap_or_default();

    let setup = values[1].as_list("preset setup")?;
    let mut state = ChordzState::default();
    state.note_setup = [NoteSetup::default(); NUM_NOTE_SLOTS];

    // 0. Active panel. Purely an editor detail.
    let _active_panel = setup
        .first()
        .map(|value| value.as_list("active panel"))
        .transpose()?;

    // 1. Octave range, transposition and follow-trigger-notes.
    if let Some(values) = setup.get(1) {
        let values = values.as_list("octave setup")?;
        let raw_from = int_at(values, 0).unwrap_or(0);
        let raw_top = int_at(values, 1).unwrap_or(10);
        state.octave_from = (raw_from + state.lowest_octave as i64).clamp(-2, 8) as i8;
        state.octave_to = (raw_top + state.lowest_octave as i64).clamp(-2, 8) as i8;
        if let Some(value) = int_at(values, 2) {
            state.transpose_octave = value.clamp(-8, 8) as i8;
        }
        if let Some(value) = int_at(values, 3) {
            state.transpose_above = value.clamp(-8, 8) as i8;
        }
        if let Some(value) = int_at(values, 4) {
            state.transpose_below = value.clamp(-8, 8) as i8;
        }
        if let Some(value) = values.get(5) {
            state.follow_trigger_notes = value.as_bool("follow trigger notes")?;
        }
    }

    // 2. Scale template and tonic.
    if let Some(values) = setup.get(2) {
        let values = values.as_list("scale setup")?;
        if let Some(value) = values.first() {
            state.template_name = value.as_str("scale template name")?;
        }
        // The stored template number does not match our parsed template index
        // (Delphi stores a menu index from a different version), so the name is
        // the authoritative value. The number is accepted but not retained.
        let _template_no = int_at(values, 1).unwrap_or(-1);
        if let Some(tonic) = int_at(values, 2) {
            state.tonic = tonic.rem_euclid(12) as u8;
        }
        if let Some(value) = values.get(3) {
            state.easy_mode = value.as_bool("easy mode")?;
        }
    }

    // 4. Twelve trigger entries. Index 0..=11 is the pitch class of the key in
    // the C-major layout, not the order in the list.
    let mut entries: Vec<Option<Trigger>> = vec![None; 12];
    if let Some(values) = setup.get(4) {
        for (index, value) in values.as_list("triggers")?.iter().enumerate().take(12) {
            entries[index] = parse_trigger(value)?;
        }
    }
    let mut triggers: Vec<Option<Trigger>> = WHITE_KEYS
        .iter()
        .map(|index| entries[*index].clone())
        .collect();
    let mut variations: Vec<Option<Trigger>> = BLACK_KEYS
        .iter()
        .map(|index| entries[*index].clone())
        .collect();

    // Scales with more than seven notes put their extra degrees on the black
    // keys. A variation always reuses a root that is already present on a white
    // key, so a black-key root outside the white-key set marks an extra degree.
    let white_roots: HashSet<u8> = triggers
        .iter()
        .flatten()
        .map(|trigger| trigger.root)
        .collect();
    let has_extra_degrees = variations
        .iter()
        .flatten()
        .any(|trigger| !white_roots.contains(&trigger.root));
    if has_extra_degrees {
        triggers.extend(variations);
        variations = Vec::new();
    }

    state.triggers = triggers
        .into_iter()
        .map(|trigger| trigger.unwrap_or_else(disabled_trigger))
        .collect();
    state.variations = variations
        .into_iter()
        .map(|trigger| trigger.unwrap_or_else(disabled_trigger))
        .collect();

    // 5. Five note setups.
    let mut note_setup = [NoteSetup::default(); NUM_NOTE_SLOTS];
    if let Some(values) = setup.get(5) {
        for (index, value) in values
            .as_list("note setups")?
            .iter()
            .enumerate()
            .take(NUM_NOTE_SLOTS)
        {
            note_setup[index] = parse_note_setup(value)?;
        }
    }
    state.note_setup = note_setup;

    // 6. Three global switches. The original writes them in published property
    // order: RelativeTiming, RandomNoteAssignment, SameRandomVelocity.
    if let Some(values) = setup.get(6) {
        let values = values.as_list("global switches")?;
        if let Some(value) = values.first() {
            state.relative_timing = value.as_bool("relative timing")?;
        }
        if let Some(value) = values.get(1) {
            state.random_note_assignment = value.as_bool("random note assignment")?;
        }
        if let Some(value) = values.get(2) {
            state.same_random_velocity = value.as_bool("same random velocity")?;
        }
    }

    Ok(Preset {
        format: PRESET_FORMAT.to_string(),
        version: PRESET_VERSION,
        name,
        state,
        note_setup,
    })
}

/// Encode a [`Preset`] as a Chordz `.codefn42p` file.
pub fn encode(preset: &Preset) -> Vec<u8> {
    let state = &preset.state;
    let mut writer = Writer::default();

    // Top-level value 0: stream version, preset name and two strings the
    // original keeps free for author/comment metadata.
    writer.list(|writer| {
        writer.string(STREAM_VERSION);
        writer.string(&preset.name);
        writer.string("");
        writer.string("");
    });

    // Top-level value 1: the setup.
    writer.list(|writer| {
        // Active panel.
        writer.list(|writer| writer.int(0));

        // Octave range and transposition.
        writer.list(|writer| {
            writer.int((state.octave_from as i64 - state.lowest_octave as i64).clamp(0, 127));
            writer.int((state.octave_to as i64 - state.lowest_octave as i64).clamp(0, 127));
            writer.int(state.transpose_octave as i64);
            writer.int(state.transpose_above as i64);
            writer.int(state.transpose_below as i64);
            writer.bool(state.follow_trigger_notes);
        });

        // Scale template.
        writer.list(|writer| {
            writer.string(&state.template_name);
            writer.int(template_number(&state.template_name));
            writer.int(state.tonic as i64);
            writer.bool(state.easy_mode);
        });

        // Active trigger. The Mac port has no separate active trigger.
        writer.list(|writer| writer.int(0));

        // Twelve trigger entries in C-major key order.
        writer.list(|writer| {
            for pc in 0..12usize {
                let trigger = trigger_for_key(state, pc);
                write_trigger(writer, trigger);
            }
        });

        // Five note setups.
        writer.list(|writer| {
            for setup in &preset.note_setup {
                writer.list(|writer| {
                    writer.float(setup.start_ms as f64);
                    writer.float(setup.start_random_ms as f64);
                    writer.float(setup.end_ms as f64);
                    writer.float(setup.end_random_ms as f64);
                    writer.float(setup.velocity_scale as f64);
                    writer.float(setup.velocity_random as f64);
                });
            }
        });

        // Relative timing, random note assignment, same random velocity.
        writer.list(|writer| {
            writer.bool(state.relative_timing);
            writer.bool(state.random_note_assignment);
            writer.bool(state.same_random_velocity);
        });
    });

    // Top-level value 2: the empty trailing list written by the original.
    writer.list(|_| {});

    let tokens = writer.out;
    let mut file = Vec::with_capacity(tokens.len() + 12);
    file.extend_from_slice(MAGIC);
    file.extend_from_slice(&((tokens.len() + FILER_SIGNATURE.len()) as u32).to_le_bytes());
    file.extend_from_slice(FILER_SIGNATURE);
    file.extend_from_slice(&tokens);
    file
}

fn trigger_for_key(state: &ChordzState, pc: usize) -> Option<&Trigger> {
    if let Some(index) = WHITE_KEYS.iter().position(|key| *key == pc) {
        return state.triggers.get(index).filter(|trigger| trigger.enabled);
    }
    let black_index = BLACK_KEYS.iter().position(|key| *key == pc)?;
    if let Some(trigger) = state
        .triggers
        .get(7 + black_index)
        .filter(|trigger| trigger.enabled)
    {
        return Some(trigger);
    }
    state
        .variations
        .get(black_index)
        .filter(|trigger| trigger.enabled)
}

fn write_trigger(writer: &mut Writer, trigger: Option<&Trigger>) {
    let Some(trigger) = trigger else {
        writer.list(|writer| {
            writer.list(|writer| {
                writer.int(-1);
                writer.string("");
            });
            writer.list(|_| {});
        });
        return;
    };

    writer.list(|writer| {
        writer.list(|writer| {
            writer.int(trigger.root as i64);
            writer.string(&trigger.chord_id);
        });
        writer.list(|writer| {
            for note in &trigger.notes {
                writer.list(|writer| {
                    writer.int(note.semitone as i64 + trigger.root as i64);
                    writer.int(note.note_no as i64 - 1);
                });
            }
        });
    });
}

fn template_number(name: &str) -> i64 {
    if name.is_empty() {
        return -1;
    }
    builtin_library()
        .template_index(name)
        .map(|index| index as i64)
        .unwrap_or(-1)
}

fn disabled_trigger() -> Trigger {
    Trigger {
        root: 0,
        chord_id: String::new(),
        notes: Vec::new(),
        enabled: false,
    }
}

fn parse_trigger(value: &Value) -> Result<Option<Trigger>, String> {
    let item = value.as_list("trigger")?;
    let head = item
        .first()
        .ok_or_else(|| "trigger is missing its header".to_string())?
        .as_list("trigger header")?;
    let root = int_at(head, 0).unwrap_or(-1);
    if root < 0 {
        return Ok(None);
    }
    let chord_id = head
        .get(1)
        .map(|value| value.as_str("chord id"))
        .transpose()?
        .unwrap_or_default();

    let mut notes = Vec::new();
    if let Some(list) = item.get(1) {
        for note in list.as_list("chord notes")? {
            let pair = note.as_list("chord note")?;
            let semitone = int_at(pair, 0).unwrap_or(0);
            let slot = int_at(pair, 1).unwrap_or(0);
            notes.push(ChordNote::new(
                (semitone - root).clamp(i16::MIN as i64, i16::MAX as i64) as i16,
                (slot + 1).clamp(1, NUM_NOTE_SLOTS as i64) as u8,
            ));
        }
    }

    Ok(Some(Trigger {
        root: root.rem_euclid(12) as u8,
        chord_id: chord_id.chars().take(crate::model::MAX_CHORD_ID).collect(),
        notes,
        enabled: true,
    }))
}

fn parse_note_setup(value: &Value) -> Result<NoteSetup, String> {
    let values = value.as_list("note setup")?;
    let read = |index: usize| -> f32 {
        values
            .get(index)
            .and_then(|value| value.as_f64())
            .unwrap_or(0.0) as f32
    };
    Ok(NoteSetup {
        start_ms: read(0),
        start_random_ms: read(1),
        end_ms: read(2),
        end_random_ms: read(3),
        velocity_scale: read(4),
        velocity_random: read(5),
    }
    .sanitized())
}

fn int_at(values: &[Value], index: usize) -> Option<i64> {
    values.get(index).and_then(|value| value.as_i64())
}

// ---------------------------------------------------------------------------
// Delphi TWriter token stream
// ---------------------------------------------------------------------------

#[derive(Clone, Debug, PartialEq)]
enum Value {
    Null,
    List(Vec<Value>),
    Int(i64),
    Bool(bool),
    Str(String),
    Float(f64),
    Binary(Vec<u8>),
}

impl Value {
    fn as_list(&self, what: &str) -> Result<&[Value], String> {
        match self {
            Value::List(values) => Ok(values),
            _ => Err(format!("{what} is not a list")),
        }
    }

    fn as_i64(&self) -> Option<i64> {
        match self {
            Value::Int(value) => Some(*value),
            Value::Float(value) => Some(*value as i64),
            _ => None,
        }
    }

    fn as_f64(&self) -> Option<f64> {
        match self {
            Value::Float(value) => Some(*value),
            Value::Int(value) => Some(*value as f64),
            _ => None,
        }
    }

    fn as_bool(&self, what: &str) -> Result<bool, String> {
        match self {
            Value::Bool(value) => Ok(*value),
            _ => Err(format!("{what} is not a boolean")),
        }
    }

    fn as_str(&self, what: &str) -> Result<String, String> {
        match self {
            Value::Str(value) => Ok(value.clone()),
            _ => Err(format!("{what} is not a string")),
        }
    }
}

struct Reader<'a> {
    data: &'a [u8],
    position: usize,
}

impl<'a> Reader<'a> {
    fn new(data: &'a [u8]) -> Self {
        Self { data, position: 0 }
    }

    fn is_empty(&self) -> bool {
        self.position >= self.data.len()
    }

    fn read_u8(&mut self) -> Result<u8, String> {
        let byte = *self
            .data
            .get(self.position)
            .ok_or_else(|| "unexpected end of Chordz preset".to_string())?;
        self.position += 1;
        Ok(byte)
    }

    fn read_exact(&mut self, length: usize) -> Result<&'a [u8], String> {
        let end = self
            .position
            .checked_add(length)
            .filter(|end| *end <= self.data.len())
            .ok_or_else(|| "unexpected end of Chordz preset".to_string())?;
        let slice = &self.data[self.position..end];
        self.position = end;
        Ok(slice)
    }

    fn read_u32(&mut self) -> Result<u32, String> {
        let bytes = self.read_exact(4)?;
        Ok(u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]))
    }

    fn read_value(&mut self, depth: usize) -> Result<Value, String> {
        if depth > 64 {
            return Err("Chordz preset is nested too deeply".to_string());
        }
        let token = self.read_u8()?;
        match token {
            0 => Ok(Value::Null),
            1 => {
                let mut values = Vec::new();
                loop {
                    if self.is_empty() {
                        return Err("unterminated list in Chordz preset".to_string());
                    }
                    if self.data[self.position] == 0 {
                        self.position += 1;
                        break;
                    }
                    values.push(self.read_value(depth + 1)?);
                    if values.len() > 1_000_000 {
                        return Err("Chordz preset list is too long".to_string());
                    }
                }
                Ok(Value::List(values))
            }
            2 => Ok(Value::Int(self.read_u8()? as i8 as i64)),
            3 => {
                let bytes = self.read_exact(2)?;
                Ok(Value::Int(i16::from_le_bytes([bytes[0], bytes[1]]) as i64))
            }
            4 => {
                let bytes = self.read_exact(4)?;
                Ok(Value::Int(
                    i32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]) as i64,
                ))
            }
            5 => {
                let bytes = self.read_exact(10)?;
                let mut raw = [0u8; 10];
                raw.copy_from_slice(bytes);
                Ok(Value::Float(decode_extended(raw)))
            }
            6 | 7 => {
                let length = self.read_u8()? as usize;
                Ok(Value::Str(decode_ansi(self.read_exact(length)?)))
            }
            8 => Ok(Value::Bool(false)),
            9 => Ok(Value::Bool(true)),
            10 => {
                let length = self.read_u32()? as usize;
                Ok(Value::Binary(self.read_exact(length)?.to_vec()))
            }
            11 => {
                let length = self.read_u8()? as usize;
                self.read_exact(length)?;
                Ok(Value::Binary(Vec::new()))
            }
            12 => {
                // Collections behave like a list terminated by vaNull.
                let mut values = Vec::new();
                loop {
                    if self.is_empty() {
                        return Err("unterminated collection in Chordz preset".to_string());
                    }
                    if self.data[self.position] == 0 {
                        self.position += 1;
                        break;
                    }
                    values.push(self.read_value(depth + 1)?);
                }
                Ok(Value::List(values))
            }
            13 => {
                let bytes = self.read_exact(4)?;
                Ok(Value::Float(
                    f32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]) as f64,
                ))
            }
            14 => {
                let bytes = self.read_exact(8)?;
                let raw = i64::from_le_bytes([
                    bytes[0], bytes[1], bytes[2], bytes[3], bytes[4], bytes[5], bytes[6], bytes[7],
                ]);
                Ok(Value::Float(raw as f64 / 10_000.0))
            }
            15 | 19 => {
                let bytes = self.read_exact(8)?;
                Ok(Value::Float(f64::from_le_bytes([
                    bytes[0], bytes[1], bytes[2], bytes[3], bytes[4], bytes[5], bytes[6], bytes[7],
                ])))
            }
            16 => {
                let length = self.read_u32()? as usize;
                let bytes = self.read_exact(length * 2)?;
                let units: Vec<u16> = bytes
                    .chunks_exact(2)
                    .map(|pair| u16::from_le_bytes([pair[0], pair[1]]))
                    .collect();
                Ok(Value::Str(String::from_utf16_lossy(&units)))
            }
            17 => {
                let bytes = self.read_exact(8)?;
                Ok(Value::Int(i64::from_le_bytes([
                    bytes[0], bytes[1], bytes[2], bytes[3], bytes[4], bytes[5], bytes[6], bytes[7],
                ])))
            }
            18 => {
                let length = self.read_u32()? as usize;
                let bytes = self.read_exact(length)?;
                Ok(Value::Str(String::from_utf8_lossy(bytes).into_owned()))
            }
            other => Err(format!(
                "unsupported Delphi token {other} at offset {}",
                self.position - 1
            )),
        }
    }
}

fn decode_ansi(bytes: &[u8]) -> String {
    match std::str::from_utf8(bytes) {
        Ok(text) => text.to_string(),
        Err(_) => bytes.iter().map(|byte| *byte as char).collect(),
    }
}

/// Decode a Delphi 80-bit `Extended` float.
fn decode_extended(bytes: [u8; 10]) -> f64 {
    let fraction = u64::from_le_bytes([
        bytes[0], bytes[1], bytes[2], bytes[3], bytes[4], bytes[5], bytes[6], bytes[7],
    ]);
    let sign_exponent = u16::from_le_bytes([bytes[8], bytes[9]]);
    let negative = sign_exponent & 0x8000 != 0;
    let exponent = (sign_exponent & 0x7fff) as i32;

    if exponent == 0x7fff {
        return if fraction == 0 {
            if negative {
                f64::NEG_INFINITY
            } else {
                f64::INFINITY
            }
        } else {
            f64::NAN
        };
    }
    if fraction == 0 {
        return if negative { -0.0 } else { 0.0 };
    }

    // The original never writes denormal note values, but decode them anyway.
    let (mantissa, exponent) = if exponent == 0 {
        let shift = fraction.leading_zeros() as i32;
        ((fraction << shift) as f64 / 2f64.powi(63), -16382 - shift)
    } else {
        (fraction as f64 / 2f64.powi(63), exponent - 16383)
    };
    let value = mantissa * 2f64.powi(exponent);
    if negative {
        -value
    } else {
        value
    }
}

/// Encode an `f64` as a Delphi 80-bit `Extended` float.
fn encode_extended(value: f64) -> [u8; 10] {
    let mut out = [0u8; 10];
    if !value.is_finite() {
        return out;
    }
    if value == 0.0 {
        out[9] = if value.is_sign_negative() { 0x80 } else { 0x00 };
        return out;
    }

    let negative = value.is_sign_negative();
    let absolute = value.abs();

    // Normalise `absolute` to [1, 2) with an exponent. Note values are always
    // ordinary numbers, so this is simpler and exact enough for f32 inputs.
    let mut exponent = absolute.log2().floor() as i32;
    let mut mantissa = absolute / 2f64.powi(exponent);
    if mantissa >= 2.0 {
        mantissa /= 2.0;
        exponent += 1;
    } else if mantissa < 1.0 {
        mantissa *= 2.0;
        exponent -= 1;
    }

    let fraction = (mantissa * 2f64.powi(63)).round() as u64;
    let biased = (exponent + 16383).clamp(0, 0x7ffe) as u16;
    out[0..8].copy_from_slice(&fraction.to_le_bytes());
    let sign_exponent = biased | if negative { 0x8000 } else { 0 };
    out[8..10].copy_from_slice(&sign_exponent.to_le_bytes());
    out
}

#[derive(Default)]
struct Writer {
    out: Vec<u8>,
}

impl Writer {
    fn int(&mut self, value: i64) {
        if let Ok(value) = i8::try_from(value) {
            self.out.push(2);
            self.out.push(value as u8);
        } else if let Ok(value) = i16::try_from(value) {
            self.out.push(3);
            self.out.extend_from_slice(&value.to_le_bytes());
        } else if let Ok(value) = i32::try_from(value) {
            self.out.push(4);
            self.out.extend_from_slice(&value.to_le_bytes());
        } else {
            self.out.push(17);
            self.out.extend_from_slice(&value.to_le_bytes());
        }
    }

    fn bool(&mut self, value: bool) {
        self.out.push(if value { 9 } else { 8 });
    }

    fn string(&mut self, value: &str) {
        if value.len() <= u8::MAX as usize && value.is_ascii() {
            self.out.push(6);
            self.out.push(value.len() as u8);
            self.out.extend_from_slice(value.as_bytes());
        } else {
            self.out.push(18);
            self.out
                .extend_from_slice(&(value.len() as u32).to_le_bytes());
            self.out.extend_from_slice(value.as_bytes());
        }
    }

    fn float(&mut self, value: f64) {
        self.out.push(5);
        self.out.extend_from_slice(&encode_extended(value));
    }

    fn list(&mut self, write: impl FnOnce(&mut Self)) {
        self.out.push(1);
        write(self);
        self.out.push(0);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extended_round_trip() {
        for value in [0.0, 1.0, -1.0, 20.0, -5.3333, 0.125, 129.0, 1000.0] {
            let encoded = encode_extended(value);
            let decoded = decode_extended(encoded);
            assert!(
                (decoded - value).abs() < 1e-9,
                "{value} decoded as {decoded}"
            );
        }
    }
}
