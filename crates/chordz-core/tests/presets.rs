//! Preset persistence tests.

use chordz_core::model::{ChordzState, NoteSetup, NUM_NOTE_SLOTS};
use chordz_core::preset::{load_preset, save_preset, Preset};

fn temp_dir(name: &str) -> std::path::PathBuf {
    std::env::temp_dir().join(format!("chordzmac-{name}-{}", std::process::id()))
}

#[test]
fn preset_round_trip_keeps_setup_and_timing() {
    let dir = temp_dir("presets");
    let _ = std::fs::remove_dir_all(&dir);

    let mut state = ChordzState::default();
    state.easy_mode = true;
    state.tonic = 5;
    state.transpose_octave = 1;
    state.apply_template_by_name("Minor");

    let mut setups = [NoteSetup::default(); NUM_NOTE_SLOTS];
    setups[0].start_ms = 120.0;
    setups[1].end_random_ms = 250.0;
    setups[2].velocity_scale = -30.0;
    setups[4].velocity_random = 20.0;

    let preset = Preset::new("My Test Chords", state.clone(), setups);
    let path = save_preset(&preset, &dir).expect("the preset should save");
    assert!(path.ends_with("My Test Chords.json"));

    let loaded = load_preset(&path).expect("the preset should load");
    assert_eq!(loaded.name, "My Test Chords");
    assert_eq!(loaded.state, state);
    assert_eq!(loaded.note_setup, setups);

    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn foreign_json_is_rejected() {
    let dir = temp_dir("bad-preset");
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();

    let path = dir.join("other.json");
    std::fs::write(&path, r#"{"format":"something-else","version":1}"#).unwrap();
    assert!(load_preset(&path).is_err());

    std::fs::write(&path, "not json at all").unwrap();
    assert!(load_preset(&path).is_err());

    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn state_survives_a_json_round_trip() {
    // The persisted plugin chunk uses the same serde representation.
    let mut state = ChordzState::default();
    state.triggers[0].set_chord_id("Maj7");
    state.relative_timing = true;
    state.octave_from = -1;

    let json = serde_json::to_string(&state).expect("state should serialize");
    let restored: ChordzState = serde_json::from_str(&json).expect("state should deserialize");
    assert_eq!(restored, state);
}
