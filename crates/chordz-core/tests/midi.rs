//! MIDI analysis tests against the seven embedded example progressions.

use chordz_core::data::builtin_library;
use chordz_core::engine::{Engine, OutputKind};
use chordz_core::midi::{
    analyze_midi, apply_analysis, estimate_key, estimate_tonic_for_mode, import_midi_file,
    key_from_filename, list_midi_files, parse_midi, seed_midi_examples, EMBEDDED_EXAMPLES,
};
use chordz_core::model::{ChordzState, NoteSetup, NUM_NOTE_SLOTS};

const WAVES: &[u8] = include_bytes!("../data/midi/Waves Midi Bpm82 A Minor.mid");
const WAVES_NAME: &str = "Waves Midi Bpm82 A Minor.mid";

#[test]
fn all_examples_parse_with_matching_tempo() {
    for (name, bytes) in EMBEDDED_EXAMPLES {
        let midi = parse_midi(bytes).unwrap_or_else(|err| panic!("{name}: {err}"));
        assert!(!midi.note_ons.is_empty(), "{name} has no notes");
        let bpm: f32 = name
            .split("Bpm")
            .nth(1)
            .and_then(|rest| rest.split(' ').next())
            .and_then(|value| value.parse().ok())
            .expect("example name carries a BPM");
        let tempo = midi.tempo_bpm.expect("tempo meta");
        assert!((tempo - bpm).abs() < 0.01, "{name}: {tempo} != {bpm}");
    }
}

#[test]
fn keys_come_from_the_filename() {
    assert_eq!(key_from_filename(WAVES_NAME), Some((9, true)));
    assert_eq!(
        key_from_filename("Roses Midi Bpm90 A Major.mid"),
        Some((9, false))
    );
    assert_eq!(
        key_from_filename("Sunrise Bpm142 D# Minor.mid"),
        Some((3, true))
    );
    assert_eq!(key_from_filename("Song in Db Major.mid"), Some((1, false)));
    assert_eq!(key_from_filename("no key here.mid"), None);
}

#[test]
fn waves_first_chord_is_the_exact_voicing() {
    let analysis = analyze_midi(WAVES, WAVES_NAME, builtin_library()).unwrap();
    assert_eq!(analysis.filename_key, Some((9, true)));
    let first = &analysis.chords[0];
    assert_eq!(first.root, 9); // A
                               // The strum window merges the tick-17 G into the opening voicing: Am7.
    assert_eq!(first.offsets, vec![0, 10, 15, 19, 22]);
    assert_eq!(first.chord_id, "Am7");
}

#[test]
fn melody_clusters_are_skipped_and_chords_deduped() {
    for (name, bytes) in EMBEDDED_EXAMPLES {
        let analysis = analyze_midi(bytes, name, builtin_library()).unwrap();
        assert!(analysis.chords.len() >= 2, "{name}: too few chords");
        assert!(analysis.chords.len() <= 12, "{name}: too many triggers");
        for chord in &analysis.chords {
            assert!(chord.offsets.len() >= 2, "{name}: single-note trigger");
            assert_eq!(chord.offsets[0], 0);
            assert!(!chord.chord_id.is_empty());
            assert!(chord.chord_id.chars().count() <= 12);
        }
        let mut seen = std::collections::HashSet::new();
        for chord in &analysis.chords {
            assert!(
                seen.insert((chord.root, chord.offsets.clone())),
                "{name}: duplicate chord"
            );
        }
    }
}

#[test]
fn histogram_fallback_on_a_clean_c_major_input() {
    // A C major scale plus a doubled C major triad: unambiguous C major.
    let mut ons: Vec<(u32, u8, u8)> = (0..8)
        .map(|i| (i as u32 * 96, [60, 62, 64, 65, 67, 69, 71, 72][i], 100))
        .collect();
    for tick in [800, 900, 1000] {
        for note in [60, 64, 67] {
            ons.push((tick, note, 100));
        }
    }
    assert_eq!(estimate_key(&ons), (0, false));
    assert_eq!(estimate_tonic_for_mode(&ons, false), 0);

    // The Waves pack is A dorian (it contains F#), so the histogram hears the
    // relative major key signature; the filename key must win over it.
    let analysis = analyze_midi(WAVES, "Untitled.mid", builtin_library()).unwrap();
    assert_eq!(analysis.filename_key, None);
    assert_eq!(analysis.key(), analysis.estimated_key);
    let named = analyze_midi(WAVES, WAVES_NAME, builtin_library()).unwrap();
    assert_eq!(named.key(), (9, true));
}

#[test]
fn apply_replaces_chord_content_only() {
    let analysis = analyze_midi(WAVES, WAVES_NAME, builtin_library()).unwrap();
    let mut state = ChordzState::default();
    state.octave_from = 0;
    state.octave_to = 4;
    state.transpose_octave = 1;
    state.easy_mode = true;
    state.relative_timing = true;
    state.note_setup[0].start_ms = 25.0;
    let before = state.clone();

    apply_analysis(&analysis, 5, &mut state);

    assert_eq!(state.tonic, 5);
    assert_eq!(state.triggers.len(), analysis.chords.len());
    assert!(state.variations.is_empty());
    assert_eq!(state.template_name, format!("MIDI: {}", analysis.name));
    assert_eq!(state.octave_from, before.octave_from);
    assert_eq!(state.octave_to, before.octave_to);
    assert_eq!(state.transpose_octave, before.transpose_octave);
    assert_eq!(state.easy_mode, before.easy_mode);
    assert_eq!(state.relative_timing, before.relative_timing);
    assert_eq!(state.note_setup, before.note_setup);
    for (trigger, chord) in state.triggers.iter().zip(&analysis.chords) {
        assert_eq!(trigger.root, chord.root);
        assert_eq!(trigger.chord_id, chord.chord_id);
        let offsets: Vec<i16> = trigger.notes.iter().map(|note| note.semitone).collect();
        assert_eq!(offsets, chord.offsets);
    }
}

#[test]
fn extracted_chords_play_through_the_engine() {
    let analysis = analyze_midi(WAVES, WAVES_NAME, builtin_library()).unwrap();
    let mut state = ChordzState::default();
    apply_analysis(&analysis, 9, &mut state);
    let setups = [NoteSetup::default(); NUM_NOTE_SLOTS];
    let mut engine = Engine::new(48_000.0);

    // A4 matches the first chord's root (A), so the exact voicing sounds.
    engine.note_on(&state, &setups, 0, 69, 100, 0);
    let mut notes = Vec::new();
    engine.flush_until(48_000, |out| {
        if let OutputKind::NoteOn { note, .. } = out.kind {
            notes.push(note);
        }
    });
    assert_eq!(notes, vec![69, 79, 84, 88, 91]);
}

#[test]
fn seeding_and_import() {
    let dir = std::env::temp_dir().join(format!("chordz-midi-test-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);

    assert_eq!(seed_midi_examples(&dir).unwrap(), 7);
    assert_eq!(seed_midi_examples(&dir).unwrap(), 0);
    assert_eq!(list_midi_files(&dir).len(), 7);

    let src = dir.join(WAVES_NAME);
    let imported = import_midi_file(&src, &dir).unwrap();
    assert_eq!(
        imported.file_name().unwrap().to_string_lossy(),
        "Waves Midi Bpm82 A Minor (2).mid"
    );
    assert_eq!(list_midi_files(&dir).len(), 8);

    let text = dir.join("notes.txt");
    std::fs::write(&text, "hello").unwrap();
    assert!(import_midi_file(&text, &dir).is_err());

    std::fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn corrupt_and_noteless_files_are_rejected() {
    assert!(parse_midi(b"not midi at all").is_err());
    // Valid header + one empty track (end-of-track meta only).
    let mut empty = Vec::new();
    empty.extend_from_slice(b"MThd");
    empty.extend_from_slice(&6u32.to_be_bytes());
    empty.extend_from_slice(&[0, 0, 0, 1, 0, 96]);
    empty.extend_from_slice(b"MTrk");
    empty.extend_from_slice(&4u32.to_be_bytes());
    empty.extend_from_slice(&[0x00, 0xff, 0x2f, 0x00]);
    assert!(analyze_midi(&empty, "Empty.mid", builtin_library()).is_err());
}
