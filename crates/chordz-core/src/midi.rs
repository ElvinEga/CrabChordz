//! MIDI progression analysis: parse a `.mid` file, extract its chords and
//! detect its key, so a progression pack can become a Chordz setup.
//!
//! The example packs are strummed chord progressions with melody notes mixed
//! into the same track. Extraction clusters nearby note-ons (the strum
//! window), takes the lowest note as the chord root, and skips single-note
//! clusters (melody). Distinct chords become one trigger each, in order of
//! appearance, with the voicing kept exactly as played.

use std::fs;
use std::path::{Path, PathBuf};

use crate::data::{default_data_dir, Library};
use crate::model::{note_name, ChordNote, ChordzState, Trigger, MAX_CHORD_ID, NUM_NOTE_SLOTS};

/// The example progressions shipped with the plugin, seeded into the user's
/// Midi folder on first run.
pub const EMBEDDED_EXAMPLES: [(&str, &[u8]); 7] = [
    (
        "Aesthetic Midi Bpm96 F Minor.mid",
        include_bytes!("../data/midi/Aesthetic Midi Bpm96 F Minor.mid"),
    ),
    (
        "Arizona Bpm122 F Minor.mid",
        include_bytes!("../data/midi/Arizona Bpm122 F Minor.mid"),
    ),
    (
        "Dulcet Midi Bpm112 D Minor.mid",
        include_bytes!("../data/midi/Dulcet Midi Bpm112 D Minor.mid"),
    ),
    (
        "Roses Midi Bpm90 A Major.mid",
        include_bytes!("../data/midi/Roses Midi Bpm90 A Major.mid"),
    ),
    (
        "Sunrise Bpm142 D# Minor.mid",
        include_bytes!("../data/midi/Sunrise Bpm142 D# Minor.mid"),
    ),
    (
        "Sunset Midi Bpm140 F Minor.mid",
        include_bytes!("../data/midi/Sunset Midi Bpm140 F Minor.mid"),
    ),
    (
        "Waves Midi Bpm82 A Minor.mid",
        include_bytes!("../data/midi/Waves Midi Bpm82 A Minor.mid"),
    ),
];

/// Maximum number of triggers: seven white keys plus five black keys.
const MAX_TRIGGERS: usize = 12;

// ---------------------------------------------------------------------------
// Parser
// ---------------------------------------------------------------------------

/// A parsed MIDI file: merged note-ons from every track, sorted by tick.
pub struct MidiFile {
    pub division: u16,
    pub tempo_bpm: Option<f32>,
    /// `(tick, note, velocity)`.
    pub note_ons: Vec<(u32, u8, u8)>,
}

fn varlen(data: &[u8], i: &mut usize) -> Result<u32, String> {
    let mut value = 0u32;
    for _ in 0..4 {
        let byte = *data.get(*i).ok_or("Truncated variable-length value")?;
        *i += 1;
        value = (value << 7) | (byte & 0x7f) as u32;
        if byte & 0x80 == 0 {
            return Ok(value);
        }
    }
    Err("Variable-length value too long".to_string())
}

pub fn parse_midi(data: &[u8]) -> Result<MidiFile, String> {
    if data.len() < 14 || &data[0..4] != b"MThd" {
        return Err("Not a MIDI file (missing MThd header)".to_string());
    }
    let header_len = u32::from_be_bytes([data[4], data[5], data[6], data[7]]) as usize;
    let format = u16::from_be_bytes([data[8], data[9]]);
    let tracks = u16::from_be_bytes([data[10], data[11]]);
    let division = u16::from_be_bytes([data[12], data[13]]);
    if format > 1 {
        return Err(format!("Unsupported MIDI format {format}"));
    }
    if division & 0x8000 != 0 {
        return Err("SMPTE-timed MIDI files are not supported".to_string());
    }
    if division == 0 {
        return Err("Invalid MIDI division".to_string());
    }

    let mut note_ons = Vec::new();
    let mut tempo_bpm = None;
    let mut i = 8 + header_len;
    for _ in 0..tracks {
        if data.len() < i + 8 || &data[i..i + 4] != b"MTrk" {
            return Err("Missing MTrk chunk".to_string());
        }
        let track_len =
            u32::from_be_bytes([data[i + 4], data[i + 5], data[i + 6], data[i + 7]]) as usize;
        let end = i + 8 + track_len;
        if end > data.len() {
            return Err("Truncated track".to_string());
        }
        let mut j = i + 8;
        let mut tick = 0u32;
        let mut status = 0u8;
        while j < end {
            tick += varlen(data, &mut j)?;
            let byte = *data.get(j).ok_or("Truncated event")?;
            if byte & 0x80 != 0 {
                status = byte;
                j += 1;
            }
            match status {
                0xff => {
                    let meta = *data.get(j).ok_or("Truncated meta event")?;
                    j += 1;
                    let len = varlen(data, &mut j)? as usize;
                    let payload = data.get(j..j + len).ok_or("Truncated meta payload")?;
                    j += len;
                    if meta == 0x51 && payload.len() == 3 && tempo_bpm.is_none() {
                        let micros = u32::from_be_bytes([0, payload[0], payload[1], payload[2]]);
                        tempo_bpm = Some(60_000_000.0 / micros as f32);
                    }
                }
                0xf0 | 0xf7 => {
                    let len = varlen(data, &mut j)? as usize;
                    j += len;
                }
                0x80..=0xef => {
                    let kind = status & 0xf0;
                    let needed = if matches!(kind, 0xc0 | 0xd0) { 1 } else { 2 };
                    let payload = data.get(j..j + needed).ok_or("Truncated channel event")?;
                    j += needed;
                    if kind == 0x90 && payload[1] > 0 {
                        note_ons.push((tick, payload[0], payload[1]));
                    }
                }
                _ => return Err(format!("Unexpected MIDI status {status:#x}")),
            }
        }
        i = end;
    }
    note_ons.sort_by_key(|event| event.0);
    Ok(MidiFile {
        division,
        tempo_bpm,
        note_ons,
    })
}

// ---------------------------------------------------------------------------
// Key detection
// ---------------------------------------------------------------------------

fn parse_note_token(token: &str) -> Option<u8> {
    let mut chars = token.chars();
    let base = match chars.next()? {
        'C' => 0,
        'D' => 2,
        'E' => 4,
        'F' => 5,
        'G' => 7,
        'A' => 9,
        'B' => 11,
        _ => return None,
    };
    let pc = match chars.next() {
        None => base,
        Some('#') => base + 1,
        Some('b') => (base + 11) % 12,
        _ => return None,
    };
    if chars.next().is_some() {
        return None;
    }
    Some(pc % 12)
}

/// Parse a key like "F Minor" / "A Major" / "D# Minor" out of a file name.
/// Returns `(tonic, is_minor)`.
pub fn key_from_filename(name: &str) -> Option<(u8, bool)> {
    let tokens: Vec<&str> = name
        .split(|c: char| c.is_whitespace() || matches!(c, '-' | '_' | '.'))
        .collect();
    for pair in tokens.windows(2) {
        let Some(tonic) = parse_note_token(pair[0]) else {
            continue;
        };
        let mode = pair[1].to_ascii_lowercase();
        let is_minor = match mode.as_str() {
            "minor" | "min" | "m" => true,
            "major" | "maj" => false,
            _ => continue,
        };
        return Some((tonic, is_minor));
    }
    None
}

// Krumhansl-Kessler key profiles.
const MAJOR_PROFILE: [f32; 12] = [
    6.35, 2.23, 3.48, 2.33, 4.38, 4.09, 2.52, 5.19, 2.39, 3.66, 2.29, 2.88,
];
const MINOR_PROFILE: [f32; 12] = [
    6.33, 2.68, 3.52, 5.38, 2.60, 3.53, 2.54, 4.75, 3.98, 2.69, 3.34, 3.17,
];

fn correlation(counts: &[f32; 12], profile: &[f32; 12], tonic: usize) -> f32 {
    let rotated: [f32; 12] = std::array::from_fn(|pc| profile[(pc + 12 - tonic) % 12]);
    let mean_a = counts.iter().sum::<f32>() / 12.0;
    let mean_b = rotated.iter().sum::<f32>() / 12.0;
    let mut cov = 0.0;
    let mut var_a = 0.0;
    let mut var_b = 0.0;
    for pc in 0..12 {
        let a = counts[pc] - mean_a;
        let b = rotated[pc] - mean_b;
        cov += a * b;
        var_a += a * a;
        var_b += b * b;
    }
    if var_a == 0.0 || var_b == 0.0 {
        return 0.0;
    }
    cov / (var_a * var_b).sqrt()
}

fn pitch_counts(note_ons: &[(u32, u8, u8)]) -> [f32; 12] {
    let mut counts = [0.0f32; 12];
    for (_, note, velocity) in note_ons {
        counts[(note % 12) as usize] += *velocity as f32;
    }
    counts
}

/// Estimate `(tonic, is_minor)` from the pitch-class distribution.
pub fn estimate_key(note_ons: &[(u32, u8, u8)]) -> (u8, bool) {
    let counts = pitch_counts(note_ons);
    let mut best = (0u8, false);
    let mut best_score = f32::NEG_INFINITY;
    for tonic in 0..12 {
        for (minor, profile) in [(false, &MAJOR_PROFILE), (true, &MINOR_PROFILE)] {
            let score = correlation(&counts, profile, tonic);
            if score > best_score {
                best_score = score;
                best = (tonic as u8, minor);
            }
        }
    }
    best
}

/// Estimate the tonic assuming a fixed mode (used when the user overrides the
/// mode in the preview).
pub fn estimate_tonic_for_mode(note_ons: &[(u32, u8, u8)], minor: bool) -> u8 {
    let counts = pitch_counts(note_ons);
    let profile = if minor {
        &MINOR_PROFILE
    } else {
        &MAJOR_PROFILE
    };
    let mut best = 0u8;
    let mut best_score = f32::NEG_INFINITY;
    for tonic in 0..12 {
        let score = correlation(&counts, profile, tonic);
        if score > best_score {
            best_score = score;
            best = tonic as u8;
        }
    }
    best
}

// ---------------------------------------------------------------------------
// Chord extraction
// ---------------------------------------------------------------------------

/// One distinct chord of the progression.
#[derive(Clone, Debug, PartialEq)]
pub struct ExtractedChord {
    /// Root pitch class (0 = C).
    pub root: u8,
    /// Root-relative semitone offsets, exactly as played (bass included).
    pub offsets: Vec<i16>,
    pub chord_id: String,
}

#[derive(Clone)]
pub struct MidiAnalysis {
    /// File stem, used for the template name.
    pub name: String,
    /// Key parsed from the file name, if present.
    pub filename_key: Option<(u8, bool)>,
    /// Histogram estimate, used when the file name has no key.
    pub estimated_key: (u8, bool),
    pub chords: Vec<ExtractedChord>,
    pub tempo_bpm: Option<f32>,
    /// True when more than 12 distinct chords were found and the tail was cut.
    pub truncated: bool,
}

impl MidiAnalysis {
    /// The key shown in the preview: file name wins over the estimate.
    pub fn key(&self) -> (u8, bool) {
        self.filename_key.unwrap_or(self.estimated_key)
    }
}

fn generated_id(root: u8, offsets: &[i16]) -> String {
    let has = |semitone: i16| offsets.iter().any(|offset| offset % 12 == semitone);
    let mut suffix = String::new();
    if has(3) {
        suffix.push('m');
    }
    if has(6) {
        suffix.push_str("dim");
    } else if has(11) {
        suffix.push_str("maj7");
    } else if has(10) {
        suffix.push('7');
    }
    format!("{}{}", note_name(root), suffix)
        .chars()
        .take(MAX_CHORD_ID)
        .collect()
}

fn match_library_id(offsets: &[i16], library: &Library) -> Option<String> {
    library
        .chords
        .iter()
        .find(|chord| {
            let mut library_offsets: Vec<i16> = chord.notes.iter().map(|n| n.semitone).collect();
            library_offsets.sort_unstable();
            library_offsets == offsets
        })
        .map(|chord| chord.chord_id.clone())
}

pub fn analyze_midi(
    data: &[u8],
    file_name: &str,
    library: &Library,
) -> Result<MidiAnalysis, String> {
    let midi = parse_midi(data)?;
    if midi.note_ons.is_empty() {
        return Err("The file contains no notes".to_string());
    }

    // Strum window: ~40 ticks at 96 ticks/beat, scaled to the file's division.
    let window = ((midi.division as u32 * 40) / 96).max(1);
    let mut clusters: Vec<(u32, Vec<u8>)> = Vec::new();
    for (tick, note, _) in &midi.note_ons {
        if let Some((start, notes)) = clusters.last_mut() {
            if *tick - *start <= window {
                notes.push(*note);
                continue;
            }
        }
        clusters.push((*tick, vec![*note]));
    }

    let mut chords: Vec<ExtractedChord> = Vec::new();
    let mut truncated = false;
    for (_, mut notes) in clusters {
        if notes.len() < 2 {
            continue; // melody / passing note
        }
        notes.sort_unstable();
        notes.dedup();
        if notes.len() < 2 {
            continue;
        }
        let root = notes[0];
        let offsets: Vec<i16> = notes.iter().map(|note| (*note - root) as i16).collect();
        if chords
            .iter()
            .any(|chord| chord.root % 12 == root % 12 && chord.offsets == offsets)
        {
            continue;
        }
        if chords.len() >= MAX_TRIGGERS {
            truncated = true;
            break;
        }
        let chord_id =
            match_library_id(&offsets, library).unwrap_or_else(|| generated_id(root, &offsets));
        chords.push(ExtractedChord {
            root: root % 12,
            offsets,
            chord_id,
        });
    }

    if chords.is_empty() {
        return Err("No chords found (only single notes?)".to_string());
    }

    let name = Path::new(file_name)
        .file_stem()
        .map(|stem| stem.to_string_lossy().to_string())
        .unwrap_or_else(|| file_name.to_string());
    Ok(MidiAnalysis {
        name,
        filename_key: key_from_filename(file_name),
        estimated_key: estimate_key(&midi.note_ons),
        chords,
        tempo_bpm: midi.tempo_bpm,
        truncated,
    })
}

/// Apply an analysis to the state: replace the chord content only. Zones,
/// transposes, flags and timing values are left untouched.
pub fn apply_analysis(analysis: &MidiAnalysis, tonic: u8, state: &mut ChordzState) {
    state.tonic = tonic % 12;
    state.triggers = analysis
        .chords
        .iter()
        .map(|chord| Trigger {
            root: chord.root,
            chord_id: chord.chord_id.clone(),
            notes: chord
                .offsets
                .iter()
                .enumerate()
                .map(|(index, offset)| ChordNote::new(*offset, (index % NUM_NOTE_SLOTS + 1) as u8))
                .collect(),
            enabled: true,
        })
        .collect();
    state.variations.clear();
    state.template_name = format!("MIDI: {}", analysis.name);
}

// ---------------------------------------------------------------------------
// Folder management
// ---------------------------------------------------------------------------

pub fn midi_dir() -> PathBuf {
    default_data_dir().join("Midi")
}

/// Write the embedded examples into `dir`, skipping files that already exist.
/// Returns the number of files written.
pub fn seed_midi_examples(dir: &Path) -> std::io::Result<usize> {
    fs::create_dir_all(dir)?;
    let mut written = 0;
    for (name, bytes) in EMBEDDED_EXAMPLES {
        let path = dir.join(name);
        if !path.exists() {
            fs::write(&path, bytes)?;
            written += 1;
        }
    }
    Ok(written)
}

pub fn list_midi_files(dir: &Path) -> Vec<PathBuf> {
    let mut paths: Vec<PathBuf> = fs::read_dir(dir)
        .into_iter()
        .flatten()
        .filter_map(|entry| entry.ok().map(|entry| entry.path()))
        .filter(|path| {
            path.extension().is_some_and(|ext| {
                ext.eq_ignore_ascii_case("mid") || ext.eq_ignore_ascii_case("midi")
            })
        })
        .collect();
    paths.sort();
    paths
}

/// Copy a `.mid` file into `dir`, appending " (2)", " (3)", ... on collision.
pub fn import_midi_file(src: &Path, dir: &Path) -> Result<PathBuf, String> {
    let is_midi = src
        .extension()
        .is_some_and(|ext| ext.eq_ignore_ascii_case("mid") || ext.eq_ignore_ascii_case("midi"));
    if !is_midi {
        return Err("Only .mid / .midi files can be imported".to_string());
    }
    if !src.is_file() {
        return Err(format!("{} is not a file", src.display()));
    }
    fs::create_dir_all(dir).map_err(|err| format!("Could not create {}: {err}", dir.display()))?;
    let stem = src
        .file_stem()
        .map(|stem| stem.to_string_lossy().to_string())
        .unwrap_or_else(|| "import".to_string());
    let extension = src
        .extension()
        .map(|ext| ext.to_string_lossy().to_string())
        .unwrap_or_else(|| "mid".to_string());
    let mut target = dir.join(format!("{stem}.{extension}"));
    let mut counter = 2;
    while target.exists() {
        target = dir.join(format!("{stem} ({counter}).{extension}"));
        counter += 1;
    }
    fs::copy(src, &target).map_err(|err| format!("Could not copy: {err}"))?;
    Ok(target)
}
