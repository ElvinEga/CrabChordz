//! The MIDI processing engine.
//!
//! This is a sample-accurate reimplementation of Chordz's note transformation:
//! chord triggers, easy mode, keyboard zones, per-note start/end delays with
//! randomisation, velocity scaling and relative timing.

use crate::model::{ChordzState, NoteSetup, NUM_NOTE_SLOTS};

/// Maximum number of simultaneously tracked trigger voices.
const MAX_VOICES: usize = 64;
/// Maximum number of queued events. Chordz itself is not polyphonic enough to
/// get anywhere near this, but it keeps the audio thread allocation-free.
const MAX_PENDING: usize = 512;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct OutputEvent {
    /// Absolute engine time in samples.
    pub time: u64,
    pub channel: u8,
    pub kind: OutputKind,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum OutputKind {
    NoteOn { note: u8, velocity: u8 },
    NoteOff { note: u8 },
}

/// A resolved trigger for a pressed note.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Resolution {
    pub variation: bool,
    pub index: usize,
    /// Root note (0..=127) that the chord is built on, in the octave of the
    /// pressed note.
    pub root: i16,
}

#[derive(Clone, Copy, Debug)]
struct PlannedNote {
    note: u8,
    slot: u8,
    on_time: u64,
    off_time: Option<u64>,
}

#[derive(Clone, Copy, Debug)]
struct Voice {
    channel: u8,
    trigger_note: u8,
    released: bool,
    note_count: usize,
    notes: [PlannedNote; crate::model::MAX_CHORD_NOTES],
}

impl Voice {
    fn empty() -> Self {
        Self {
            channel: 0,
            trigger_note: 0,
            released: false,
            note_count: 0,
            notes: [PlannedNote {
                note: 0,
                slot: 0,
                on_time: 0,
                off_time: None,
            }; crate::model::MAX_CHORD_NOTES],
        }
    }
}

#[derive(Clone, Copy, Debug)]
struct Pending {
    time: u64,
    seq: u64,
    channel: u8,
    kind: OutputKind,
}

/// A tiny deterministic PRNG so tests can reproduce randomisation.
#[derive(Clone, Debug)]
pub struct Rng(u64);

impl Rng {
    pub fn new(seed: u64) -> Self {
        Self(seed.max(1))
    }

    pub fn seeded_now() -> Self {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos() as u64)
            .unwrap_or(0x9e37_79b9_7f4a_7c15);
        Self::new(nanos ^ 0x9e37_79b9_7f4a_7c15)
    }

    fn next_u64(&mut self) -> u64 {
        let mut x = self.0;
        x ^= x >> 12;
        x ^= x << 25;
        x ^= x >> 27;
        self.0 = x;
        x.wrapping_mul(0x2545_f491_4f6c_dd1d)
    }

    /// Uniform float in `[0, max]`.
    pub fn range(&mut self, max: f32) -> f32 {
        if max <= 0.0 {
            return 0.0;
        }
        let unit = (self.next_u64() >> 11) as f32 / (1u64 << 53) as f32;
        unit * max
    }

    fn unit(&mut self) -> f32 {
        (self.next_u64() >> 11) as f32 / (1u64 << 53) as f32
    }

    fn range_u8(&mut self, max_inclusive: u8) -> u8 {
        if max_inclusive == 0 {
            return 0;
        }
        (self.next_u64() % (max_inclusive as u64 + 1)) as u8
    }
}

pub struct Engine {
    sample_rate: f32,
    clock: u64,
    seq: u64,
    rng: Rng,
    voices: [Voice; MAX_VOICES],
    voice_len: usize,
    pending: Vec<Pending>,
    active_notes: [u16; 128],
    steal_cursor: usize,
}

impl Default for Engine {
    fn default() -> Self {
        Self::new(48_000.0)
    }
}

impl Engine {
    pub fn new(sample_rate: f32) -> Self {
        Self {
            sample_rate,
            clock: 0,
            seq: 0,
            rng: Rng::seeded_now(),
            voices: [Voice::empty(); MAX_VOICES],
            voice_len: 0,
            pending: Vec::with_capacity(MAX_PENDING),
            active_notes: [0; 128],
            steal_cursor: 0,
        }
    }

    pub fn set_seed(&mut self, seed: u64) {
        self.rng = Rng::new(seed);
    }

    pub fn set_sample_rate(&mut self, sample_rate: f32) {
        self.sample_rate = sample_rate.max(1.0);
    }

    pub fn reset(&mut self) {
        self.voice_len = 0;
        self.pending.clear();
        self.active_notes = [0; 128];
        self.clock = 0;
    }

    /// Current absolute engine time in samples.
    pub fn clock(&self) -> u64 {
        self.clock
    }

    fn ms_to_samples(&self, ms: f32) -> u64 {
        ((ms.max(0.0) as f64) * self.sample_rate as f64 / 1000.0).round() as u64
    }

    fn push_pending(&mut self, event: Pending) {
        if self.pending.len() >= MAX_PENDING {
            // Should never happen with sensible chords; drop the oldest queued
            // event so the audio thread never allocates or panics.
            self.pending.remove(0);
        }
        let position = self
            .pending
            .partition_point(|p| (p.time, p.seq) <= (event.time, event.seq));
        self.pending.insert(position, event);
    }

    fn schedule_on(&mut self, time: u64, channel: u8, note: u8, velocity: u8) {
        self.seq += 1;
        self.push_pending(Pending {
            time,
            seq: self.seq,
            channel,
            kind: OutputKind::NoteOn { note, velocity },
        });
    }

    fn schedule_off(&mut self, time: u64, channel: u8, note: u8) {
        self.seq += 1;
        self.push_pending(Pending {
            time,
            seq: self.seq,
            channel,
            kind: OutputKind::NoteOff { note },
        });
    }

    /// Emit every queued event whose time is `<= limit`, in time order.
    pub fn flush_until(&mut self, limit: u64, mut emit: impl FnMut(OutputEvent)) {
        while let Some(event) = self.pending.first().copied() {
            if event.time > limit {
                break;
            }
            self.pending.remove(0);
            match event.kind {
                OutputKind::NoteOn { note, velocity } => {
                    // A retriggered note must not leave the previous instance
                    // hanging. Most synths ignore a second note-on otherwise.
                    if self.active_notes[note as usize] > 0 {
                        emit(OutputEvent {
                            time: event.time,
                            channel: event.channel,
                            kind: OutputKind::NoteOff { note },
                        });
                    }
                    self.active_notes[note as usize] =
                        self.active_notes[note as usize].saturating_add(1);
                    emit(OutputEvent {
                        time: event.time,
                        channel: event.channel,
                        kind: OutputKind::NoteOn { note, velocity },
                    });
                }
                OutputKind::NoteOff { note } => {
                    let count = self.active_notes[note as usize];
                    if count > 0 {
                        self.active_notes[note as usize] = count - 1;
                    }
                    if count <= 1 {
                        emit(OutputEvent {
                            time: event.time,
                            channel: event.channel,
                            kind: OutputKind::NoteOff { note },
                        });
                    }
                }
            }
        }

        self.clock = self.clock.max(limit);
        self.retire_finished_voices();
    }

    fn retire_finished_voices(&mut self) {
        let now = self.clock;
        let mut write = 0;
        for read in 0..self.voice_len {
            let voice = self.voices[read];
            let finished = voice.released
                && voice.notes[..voice.note_count]
                    .iter()
                    .all(|n| n.off_time.map(|t| t <= now).unwrap_or(false));
            if !finished {
                self.voices[write] = voice;
                write += 1;
            }
        }
        self.voice_len = write;
    }

    fn allocate_voice(&mut self) -> usize {
        if self.voice_len < MAX_VOICES {
            let index = self.voice_len;
            self.voice_len += 1;
            return index;
        }
        // Steal a released voice first, otherwise rotate through the pool. The
        // old voice's queued events remain in the queue so the host still gets
        // the matching note-offs.
        if let Some(index) = (0..MAX_VOICES).find(|i| self.voices[*i].released) {
            return index;
        }
        let index = self.steal_cursor % MAX_VOICES;
        self.steal_cursor = self.steal_cursor.wrapping_add(1);
        index
    }

    /// Handle a note-on from the host. `at` is an absolute sample time.
    pub fn note_on(
        &mut self,
        state: &ChordzState,
        setups: &[NoteSetup; NUM_NOTE_SLOTS],
        channel: u8,
        note: u8,
        velocity: u8,
        at: u64,
    ) {
        self.clock = self.clock.max(at);

        let octave = state.octave_number(note);
        if octave < state.octave_from as i32 || octave > state.octave_to as i32 {
            let transpose = if octave < state.octave_from as i32 {
                state.transpose_below as i32
            } else {
                state.transpose_above as i32
            };
            if let Some(out) = resolve_single(state, note, transpose) {
                self.schedule_on(at, channel, out, velocity);
            }
            return;
        }

        let Some(resolution) = resolve(state, note) else {
            return;
        };
        let trigger = if resolution.variation {
            &state.variations[resolution.index]
        } else {
            &state.triggers[resolution.index]
        };

        let voice_index = self.allocate_voice();
        let mut voice = Voice {
            channel,
            trigger_note: note,
            released: false,
            note_count: 0,
            ..Voice::empty()
        };

        let mut planned = [PlannedNote {
            note: 0,
            slot: 0,
            on_time: 0,
            off_time: None,
        }; crate::model::MAX_CHORD_NOTES];

        let intervals: Vec<i16> = if trigger.notes.is_empty() {
            vec![0]
        } else {
            trigger.notes.iter().map(|n| n.semitone).collect()
        };

        let shared_random = if state.same_random_velocity {
            Some(self.rng.unit())
        } else {
            None
        };

        let mut count = 0usize;
        let mut start_accum = 0u64;
        let mut seen = [false; 128];

        for (i, semitone) in intervals.iter().enumerate() {
            if count >= crate::model::MAX_CHORD_NOTES {
                break;
            }
            let out =
                resolution.root as i32 + *semitone as i32 + 12 * state.transpose_octave as i32;
            if !(0..=127).contains(&out) {
                continue;
            }
            let out = out as u8;
            if seen[out as usize] {
                continue;
            }
            seen[out as usize] = true;

            let explicit_slot = trigger
                .notes
                .get(i)
                .map(|n| n.note_no)
                .unwrap_or(((i % NUM_NOTE_SLOTS) + 1) as u8);
            let slot = if state.random_note_assignment {
                self.rng.range_u8(NUM_NOTE_SLOTS as u8 - 1) + 1
            } else {
                explicit_slot.clamp(1, NUM_NOTE_SLOTS as u8)
            };
            let setup = setups[(slot - 1) as usize];

            let start_random = self.rng.range(setup.start_random_ms);
            let delay = self.ms_to_samples(setup.start_ms) + self.ms_to_samples(start_random);
            let start = if state.relative_timing {
                start_accum += delay;
                at + start_accum
            } else {
                at + delay
            };

            let velocity_draw = shared_random.unwrap_or_else(|| self.rng.unit());
            let out_velocity = scaled_velocity(velocity, &setup, velocity_draw);

            planned[count] = PlannedNote {
                note: out,
                slot,
                on_time: start,
                off_time: None,
            };
            count += 1;
            self.schedule_on(start, channel, out, out_velocity);
        }

        voice.note_count = count;
        voice.notes = planned;
        self.voices[voice_index] = voice;
    }

    /// Handle a note-off from the host.
    pub fn note_off(
        &mut self,
        state: &ChordzState,
        setups: &[NoteSetup; NUM_NOTE_SLOTS],
        channel: u8,
        note: u8,
        at: u64,
    ) {
        self.clock = self.clock.max(at);

        let octave = state.octave_number(note);
        if octave < state.octave_from as i32 || octave > state.octave_to as i32 {
            // Single notes outside the chord zone are passed through (and end
            // immediately, like the original's non-chord zones).
            let transpose = if octave < state.octave_from as i32 {
                state.transpose_below as i32
            } else {
                state.transpose_above as i32
            };
            if let Some(out) = resolve_single(state, note, transpose) {
                self.schedule_off(at, channel, out);
            }
            return;
        }

        let mut found = None;
        for index in (0..self.voice_len).rev() {
            let voice = &self.voices[index];
            if voice.channel == channel && voice.trigger_note == note && !voice.released {
                found = Some(index);
                break;
            }
        }
        let Some(index) = found else {
            return;
        };

        let mut voice = self.voices[index];
        voice.released = true;
        let mut end_accum = 0u64;
        for note_index in 0..voice.note_count {
            let mut note = voice.notes[note_index];
            let setup = setups[(note.slot.clamp(1, NUM_NOTE_SLOTS as u8) - 1) as usize];
            let end_random = self.rng.range(setup.end_random_ms);
            let delay = self.ms_to_samples(setup.end_ms) + self.ms_to_samples(end_random);
            let end = if state.relative_timing {
                end_accum += delay;
                at + end_accum
            } else {
                at + delay
            };
            let off = end.max(note.on_time);
            note.off_time = Some(off);
            voice.notes[note_index] = note;
            self.schedule_off(off, channel, note.note);
        }
        self.voices[index] = voice;
    }

    /// Send an immediate chord preview for a trigger index.
    pub fn preview(
        &mut self,
        state: &ChordzState,
        setups: &[NoteSetup; NUM_NOTE_SLOTS],
        variation: bool,
        index: usize,
        channel: u8,
        at: u64,
    ) {
        let trigger = if variation {
            state.variations.get(index)
        } else {
            state.triggers.get(index)
        };
        let Some(trigger) = trigger else {
            return;
        };
        if !trigger.enabled {
            return;
        }

        // The preview keyboard in the original plays the chord where it was
        // edited, with middle C as the reference.
        let root = 60 + trigger.root as i32 + 12 * state.transpose_octave as i32;
        let intervals: Vec<i16> = if trigger.notes.is_empty() {
            vec![0]
        } else {
            trigger.notes.iter().map(|n| n.semitone).collect()
        };
        let release_at = at + self.ms_to_samples(600.0);
        let mut seen = [false; 128];
        for (i, semitone) in intervals.iter().enumerate() {
            let out = root + *semitone as i32;
            if !(0..=127).contains(&out) || seen[out as usize] {
                continue;
            }
            let out = out as u8;
            seen[out as usize] = true;
            let slot = trigger
                .notes
                .get(i)
                .map(|n| n.note_no)
                .unwrap_or(((i % NUM_NOTE_SLOTS) + 1) as u8)
                .clamp(1, NUM_NOTE_SLOTS as u8);
            let setup = setups[(slot - 1) as usize];
            let velocity = scaled_velocity(100, &setup, self.rng.unit());
            self.schedule_on(at, channel, out, velocity);
            self.schedule_off(release_at, channel, out);
        }
    }
}

/// Resolve a note to a chord trigger.
///
/// With easy mode disabled the pressed pitch class must match a trigger root.
/// With easy mode enabled the seven white keys address the first seven scale
/// degrees (C is always the tonic) and any additional degrees or the template's
/// black-key variations live on C#, D#, F#, G#, A#.
pub fn resolve(state: &ChordzState, note: u8) -> Option<Resolution> {
    if state.easy_mode {
        resolve_easy(state, note)
    } else {
        let pc = note % 12;
        state
            .triggers
            .iter()
            .position(|t| t.enabled && t.root % 12 == pc)
            .map(|index| Resolution {
                variation: false,
                index,
                root: note as i16,
            })
    }
}

fn resolve_easy(state: &ChordzState, note: u8) -> Option<Resolution> {
    const WHITE: [u8; 7] = [0, 2, 4, 5, 7, 9, 11];
    const BLACK: [u8; 5] = [1, 3, 6, 8, 10];

    let pc = note % 12;
    let base = (note - pc) as i16;
    if let Some(index) = WHITE.iter().position(|w| *w == pc) {
        if let Some(trigger) = state.triggers.get(index).filter(|t| t.enabled) {
            return Some(Resolution {
                variation: false,
                index,
                root: base + trigger.root as i16,
            });
        }
        return None;
    }
    let black_index = BLACK.iter().position(|b| *b == pc)?;
    let degree = 7 + black_index;
    if let Some(trigger) = state.triggers.get(degree).filter(|t| t.enabled) {
        return Some(Resolution {
            variation: false,
            index: degree,
            root: base + trigger.root as i16,
        });
    }
    if let Some(trigger) = state.variations.get(black_index).filter(|t| t.enabled) {
        return Some(Resolution {
            variation: true,
            index: black_index,
            root: base + trigger.root as i16,
        });
    }
    None
}

/// Resolve a single note outside the trigger zone, applying the zone
/// transposition and optionally keeping the note in the selected scale.
pub fn resolve_single(state: &ChordzState, note: u8, transpose: i32) -> Option<u8> {
    let transposed = note as i32 + 12 * transpose;
    if !(0..=127).contains(&transposed) {
        return None;
    }
    let transposed = transposed as u8;
    if !state.follow_trigger_notes {
        return Some(transposed);
    }
    if state.easy_mode {
        if let Some(resolution) = resolve_easy(state, note) {
            let mapped = resolution.root + 12 * transpose as i16;
            return u8::try_from(mapped).ok().filter(|n| *n < 128);
        }
        return None;
    }
    Some(snap_to_scale(state, transposed))
}

/// Force a note into the scale by moving it to the nearest scale pitch class.
pub fn snap_to_scale(state: &ChordzState, note: u8) -> u8 {
    if state.is_in_scale_pc(note % 12) {
        return note;
    }
    for distance in 1..=6u8 {
        if note as i16 + distance as i16 <= 127 {
            let up = note + distance;
            if state.is_in_scale_pc(up % 12) {
                return up;
            }
        }
        if note as i16 - distance as i16 >= 0 {
            let down = note - distance;
            if state.is_in_scale_pc(down % 12) {
                return down;
            }
        }
    }
    note
}

fn scaled_velocity(input: u8, setup: &NoteSetup, unit_draw: f32) -> u8 {
    let scale = setup.velocity_scale + unit_draw.clamp(0.0, 1.0) * setup.velocity_random;
    let output = input as f32 * (1.0 + scale / 100.0);
    (output.round() as i32).clamp(1, 127) as u8
}

/// Human readable description of a stored chord, used by the UI.
pub fn describe_chord(state: &ChordzState, variation: bool, index: usize) -> String {
    let trigger = if variation {
        state.variations.get(index)
    } else {
        state.triggers.get(index)
    };
    let Some(trigger) = trigger else {
        return String::new();
    };
    let mut notes = Vec::new();
    if trigger.notes.is_empty() {
        notes.push(crate::model::note_name_with_octave(
            trigger.root,
            state.lowest_octave,
        ));
    } else {
        for note in &trigger.notes {
            let value = trigger.root as i32 + note.semitone as i32;
            if (0..=127).contains(&value) {
                notes.push(crate::model::note_name_with_octave(
                    value as u8,
                    state.lowest_octave,
                ));
            }
        }
    }
    notes.join(" ")
}
