//! Tests against the original Chordz `.codefn42p` presets shipped with the
//! Windows plugin (copied from the user's Presets folder).

use chordz_core::preset::{
    load_preset_file, save_preset_as, Preset, PresetFormat, CODEFN42P_EXTENSION,
};
use chordz_core::{ChordzState, Engine};

const ORION: &[u8] = include_bytes!("fixtures/A - Orion strm.codefn42p");
const ALL_MOST: &[u8] = include_bytes!("fixtures/Bb - All Most.codefn42p");
const LIFETIME: &[u8] = include_bytes!("fixtures/D - Lifetime - crumbles.codefn42p");
const GOSPEL: &[u8] = include_bytes!("fixtures/Gospel RB - No Key.codefn42p");

fn decode(bytes: &[u8]) -> Preset {
    chordz_core::preset::codefn42p::decode(bytes).expect("preset should decode")
}

fn roots(state: &ChordzState) -> Vec<u8> {
    state.triggers.iter().map(|t| t.root).collect()
}

#[test]
fn orion_setup_matches_the_original_file() {
    let preset = decode(ORION);
    assert_eq!(preset.name, "A - Orion strm");
    let state = &preset.state;
    assert_eq!(state.template_name, "Minor");
    assert_eq!(state.tonic, 9);
    assert!(state.easy_mode);
    assert!(state.follow_trigger_notes);
    assert_eq!(state.octave_from, -2);
    assert_eq!(state.octave_to, 8);
    assert!(!state.relative_timing);
    assert!(!state.random_note_assignment);
    assert!(state.same_random_velocity);

    // White keys carry the seven A-minor degrees with custom chord voicings.
    assert_eq!(roots(state), vec![9, 11, 0, 2, 4, 5, 7]);
    let ids: Vec<&str> = state.triggers.iter().map(|t| t.chord_id.as_str()).collect();
    assert_eq!(
        ids,
        vec!["F#m9", "F#m7", "F#m7^", "Bm7", "DMaj7", "C#m7", "C#m7/E"]
    );
    assert!(state.triggers.iter().all(|t| t.enabled));
    // All five black keys are unassigned in this preset.
    assert_eq!(state.variations.len(), 5);
    assert!(state.variations.iter().all(|t| !t.enabled));

    // The stored chord notes are absolute semitones from C; the model keeps
    // them relative to the trigger root (9 = A).
    let first = &state.triggers[0];
    let semitones: Vec<i16> = first.notes.iter().map(|n| n.semitone).collect();
    let slots: Vec<u8> = first.notes.iter().map(|n| n.note_no).collect();
    assert_eq!(semitones, vec![-27, -15, -12, -8, -5, -1]);
    assert_eq!(slots, vec![2, 1, 3, 4, 5, 5]);
}

#[test]
fn orion_timing_values_match_the_original_file() {
    let preset = decode(ORION);
    let setups = preset.note_setup;
    assert_eq!(setups[0].start_ms, 20.0);
    assert_eq!(setups[1].start_ms, 52.0);
    assert_eq!(setups[2].start_ms, 35.0);
    assert_eq!(setups[3].start_ms, 85.0);
    assert_eq!(setups[4].start_ms, 129.0);
    assert!((setups[0].velocity_scale - (-5.3333282)).abs() < 1e-4);
    assert!((setups[0].velocity_random - 44.66661).abs() < 1e-3);
    assert!((setups[3].velocity_scale - 9.3333244).abs() < 1e-4);
    assert!(setups
        .iter()
        .all(|s| s.end_ms == 0.0 && s.start_random_ms == 0.0));
    // The plugin keeps the same values in the state and in the preset copy.
    assert_eq!(preset.state.note_setup, setups);
}

#[test]
fn all_most_and_lifetime_tonics_and_roots() {
    let bb = decode(ALL_MOST);
    assert_eq!(bb.name, "Bb - All Most");
    assert_eq!(bb.state.tonic, 10);
    assert_eq!(roots(&bb.state), vec![10, 0, 1, 3, 5, 6, 8]);
    assert!(!bb.state.same_random_velocity);
    assert!(bb.note_setup.iter().all(|s| *s == Default::default()));

    let d = decode(LIFETIME);
    assert_eq!(d.name, "D - Lifetime - crumbles");
    assert_eq!(d.state.tonic, 2);
    assert_eq!(roots(&d.state), vec![2, 4, 5, 7, 9, 10, 0]);
    assert!(d.state.same_random_velocity);
    assert_eq!(d.note_setup[1].start_ms, 20.0);
    assert_eq!(d.note_setup[2].start_ms, 52.0);
    assert_eq!(d.note_setup[3].start_ms, 20.0);
    assert_eq!(d.note_setup[4].start_ms, 50.0);
    assert!((d.note_setup[4].velocity_random - 23.333315).abs() < 1e-3);
}

#[test]
fn gospel_has_no_template_but_keeps_custom_voicings() {
    let preset = decode(GOSPEL);
    assert_eq!(preset.name, "Gospel RB - No Key");
    assert_eq!(preset.state.template_name, "");
    assert_eq!(preset.state.tonic, 0);
    assert_eq!(roots(&preset.state), vec![0, 2, 4, 5, 7, 9, 11]);
    assert!(preset.state.triggers.iter().all(|t| t.chord_id.is_empty()));
    // Empty chord IDs must not discard the notes.
    let first = &preset.state.triggers[0];
    let semitones: Vec<i16> = first.notes.iter().map(|n| n.semitone).collect();
    assert_eq!(semitones, vec![-7, 0, 3, 8, 10]);
}

#[test]
fn every_sample_round_trips_through_encode_and_decode() {
    for bytes in [ORION, ALL_MOST, LIFETIME, GOSPEL] {
        let original = decode(bytes);
        let encoded = chordz_core::preset::codefn42p::encode(&original);
        let restored = decode(&encoded);
        assert_eq!(restored.name, original.name);
        assert_eq!(restored.state, original.state);
        assert_eq!(restored.note_setup, original.note_setup);
    }
}

#[test]
fn saved_codefn42p_file_loads_through_the_preset_browser_api() {
    let dir = std::env::temp_dir().join(format!("crabchordz-codefn42p-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);

    let original = decode(ORION);
    let path = save_preset_as(&original, &dir, PresetFormat::Codefn42p).unwrap();
    assert_eq!(
        path.extension().unwrap().to_str().unwrap(),
        CODEFN42P_EXTENSION
    );

    let loaded = load_preset_file(&path).unwrap();
    assert_eq!(loaded.name, original.name);
    assert_eq!(loaded.state, original.state);
    assert_eq!(loaded.note_setup, original.note_setup);

    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn decoded_orion_preset_plays_the_stored_voicing() {
    // Easy mode, C4 pressed: degree I is A, and the stored absolute offsets
    // (-18, -6, -3, 1, 4, 8 from C) must sound relative to the pressed octave.
    let preset = decode(ORION);
    let state = &preset.state;
    let setups = preset.note_setup;
    let mut engine = Engine::new(48_000.0);
    engine.set_seed(1);

    engine.note_on(state, &setups, 0, 60, 100, 0);
    // The preset delays each note by 20..129 ms; flush past the last delay.
    let mut notes = Vec::new();
    engine.flush_until(48_000, |event| {
        if let chordz_core::engine::OutputKind::NoteOn { note, .. } = event.kind {
            notes.push(note);
        }
    });
    notes.sort();
    assert_eq!(notes, vec![42, 54, 57, 61, 64, 68]);
}

#[test]
fn garbage_and_truncated_files_are_rejected() {
    assert!(chordz_core::preset::codefn42p::decode(b"not a preset").is_err());
    assert!(chordz_core::preset::codefn42p::decode(&ORION[..40]).is_err());
    // A compressed preset (no TPF0 signature) gets a helpful error.
    let mut compressed = ORION.to_vec();
    compressed[8] = b'X';
    let err = chordz_core::preset::codefn42p::decode(&compressed).unwrap_err();
    assert!(err.contains("compression"), "unexpected error: {err}");
}
