use chordz_core::engine::{OutputKind, Resolution};
use chordz_core::model::{ChordNote, NoteSetup, Trigger};
use chordz_core::{resolve, ChordzState, Engine};

fn setup_for(state: &ChordzState) -> [NoteSetup; 5] {
    state.note_setup
}

fn collect(engine: &mut Engine, until: u64) -> Vec<(u64, OutputKind)> {
    let mut events = Vec::new();
    engine.flush_until(until, |event| events.push((event.time, event.kind)));
    events
}

fn note_ons(events: &[(u64, OutputKind)]) -> Vec<u8> {
    events
        .iter()
        .filter_map(|(_, kind)| match kind {
            OutputKind::NoteOn { note, .. } => Some(*note),
            _ => None,
        })
        .collect()
}

fn offs(events: &[(u64, OutputKind)]) -> Vec<u8> {
    events
        .iter()
        .filter_map(|(_, kind)| match kind {
            OutputKind::NoteOff { note } => Some(*note),
            _ => None,
        })
        .collect()
}

#[test]
fn c_major_diatonic_chords_easy_mode_off() {
    let state = ChordzState::default();
    assert!(!state.easy_mode);
    let setups = setup_for(&state);
    let mut engine = Engine::new(48_000.0);
    engine.set_seed(1);

    engine.note_on(&state, &setups, 0, 60, 100, 0);
    let events = collect(&mut engine, 0);
    assert_eq!(note_ons(&events), vec![60, 64, 67]);

    engine.note_off(&state, &setups, 0, 60, 480);
    let events = collect(&mut engine, 480);
    assert_eq!(offs(&events), vec![60, 64, 67]);
}

#[test]
fn d_minor_and_b_diminished() {
    let state = ChordzState::default();
    let setups = setup_for(&state);
    let mut engine = Engine::new(48_000.0);
    engine.set_seed(1);

    engine.note_on(&state, &setups, 0, 62, 100, 0);
    assert_eq!(note_ons(&collect(&mut engine, 0)), vec![62, 65, 69]);

    engine.note_on(&state, &setups, 0, 71, 100, 10);
    assert_eq!(note_ons(&collect(&mut engine, 10)), vec![71, 74, 77]);
}

#[test]
fn easy_mode_maps_white_keys_to_e_major() {
    let mut state = ChordzState::default();
    state.tonic = 4;
    state.apply_template_by_name("Major");
    state.easy_mode = true;
    let setups = setup_for(&state);
    let mut engine = Engine::new(48_000.0);
    engine.set_seed(1);

    // C -> E major, D -> F# minor, B -> D# diminished (per the user guide).
    engine.note_on(&state, &setups, 0, 60, 100, 0);
    assert_eq!(note_ons(&collect(&mut engine, 0)), vec![64, 68, 71]);

    engine.note_on(&state, &setups, 0, 62, 100, 10);
    assert_eq!(note_ons(&collect(&mut engine, 10)), vec![66, 69, 73]);

    engine.note_on(&state, &setups, 0, 71, 100, 20);
    assert_eq!(note_ons(&collect(&mut engine, 20)), vec![63, 66, 69]);
}

#[test]
fn easy_mode_black_keys_play_template_variations() {
    let mut state = ChordzState::default();
    assert!(state.apply_template_by_name("Major - Sevenths"));
    state.easy_mode = true;
    let setups = setup_for(&state);
    let mut engine = Engine::new(48_000.0);
    engine.set_seed(1);

    // C# is the first variation: Cmaj7.
    engine.note_on(&state, &setups, 0, 61, 100, 0);
    assert_eq!(note_ons(&collect(&mut engine, 0)), vec![60, 64, 67, 71]);

    // G# is the fourth variation: G7.
    engine.note_on(&state, &setups, 0, 68, 100, 10);
    assert_eq!(note_ons(&collect(&mut engine, 10)), vec![67, 71, 74, 77]);
}

#[test]
fn easy_mode_without_variations_leaves_black_keys_silent() {
    let state = ChordzState::default(); // Major template has no variations
    assert!(state.easy_mode == false);
    let mut state_easy = state.clone();
    state_easy.easy_mode = true;
    let setups = setup_for(&state_easy);
    let mut engine = Engine::new(48_000.0);
    engine.set_seed(1);

    engine.note_on(&state_easy, &setups, 0, 61, 100, 0); // C#
    assert!(collect(&mut engine, 0).is_empty());
}

#[test]
fn notes_outside_the_trigger_zone_are_transposed_and_passed_through() {
    let mut state = ChordzState::default();
    state.octave_from = 3;
    state.octave_to = 4;
    state.transpose_below = -1;
    state.transpose_above = 1;
    let setups = setup_for(&state);
    let mut engine = Engine::new(48_000.0);
    engine.set_seed(1);

    // MIDI note 24 is C1 with the default -2 numbering, below the zone.
    engine.note_on(&state, &setups, 0, 24, 100, 0);
    assert_eq!(note_ons(&collect(&mut engine, 0)), vec![12]);

    // Note 96 is C7, above the zone.
    engine.note_on(&state, &setups, 0, 96, 100, 10);
    assert_eq!(note_ons(&collect(&mut engine, 10)), vec![108]);

    engine.note_off(&state, &setups, 0, 24, 20);
    assert_eq!(offs(&collect(&mut engine, 20)), vec![12]);
}

#[test]
fn follow_trigger_notes_snaps_outside_notes_to_the_scale() {
    let mut state = ChordzState::default();
    state.follow_trigger_notes = true;
    state.octave_from = 3;
    state.octave_to = 4;
    let setups = setup_for(&state);
    let mut engine = Engine::new(48_000.0);
    engine.set_seed(1);

    // C#1 is not in C major; the nearest scale notes are C and D.
    engine.note_on(&state, &setups, 0, 25, 100, 0);
    let ons = note_ons(&collect(&mut engine, 0));
    assert_eq!(ons.len(), 1);
    assert!(ons[0] == 24 || ons[0] == 26);
}

#[test]
fn follow_trigger_notes_in_easy_mode_plays_the_mapped_root() {
    let mut state = ChordzState::default();
    state.tonic = 5; // F
    state.apply_template_by_name("Minor");
    state.follow_trigger_notes = true;
    state.easy_mode = true;
    state.octave_from = 3;
    state.octave_to = 4;
    let setups = setup_for(&state);
    let mut engine = Engine::new(48_000.0);
    engine.set_seed(1);

    // C1 outside the trigger zone -> F, matching the manual's example.
    engine.note_on(&state, &setups, 0, 24, 100, 0);
    assert_eq!(note_ons(&collect(&mut engine, 0)), vec![29]); // F1
}

#[test]
fn start_delay_is_relative_when_enabled() {
    let mut state = ChordzState::default();
    state.relative_timing = true;
    state.note_setup[0].start_ms = 10.0;
    state.note_setup[1].start_ms = 20.0;
    state.note_setup[2].start_ms = 30.0;
    let setups = setup_for(&state);
    let mut engine = Engine::new(48_000.0);
    engine.set_seed(1);

    engine.note_on(&state, &setups, 0, 60, 100, 0);
    let events = collect(&mut engine, 48_000);
    let times: Vec<u64> = events
        .iter()
        .filter(|(_, kind)| matches!(kind, OutputKind::NoteOn { .. }))
        .map(|(time, _)| *time)
        .collect();
    // 10 ms, 30 ms and 60 ms at 48 kHz.
    assert_eq!(times, vec![480, 1_440, 2_880]);
}

#[test]
fn end_delay_delays_note_offs() {
    let mut state = ChordzState::default();
    state.note_setup[0].end_ms = 50.0;
    let setups = setup_for(&state);
    let mut engine = Engine::new(48_000.0);
    engine.set_seed(1);

    engine.note_on(&state, &setups, 0, 60, 100, 0);
    collect(&mut engine, 0);
    engine.note_off(&state, &setups, 0, 60, 1_000);
    let events = collect(&mut engine, 5_000);
    let first_off = events
        .iter()
        .find(|(_, kind)| matches!(kind, OutputKind::NoteOff { note: 60 }))
        .map(|(time, _)| *time)
        .unwrap();
    assert_eq!(first_off, 1_000 + 2_400); // 50 ms at 48 kHz
}

#[test]
fn velocity_scaling_matches_the_manual() {
    let mut state = ChordzState::default();
    state.note_setup[0].velocity_scale = -50.0;
    state.note_setup[1].velocity_scale = 100.0;
    state.note_setup[2].velocity_scale = 0.0;
    let setups = setup_for(&state);
    let mut engine = Engine::new(48_000.0);
    engine.set_seed(1);

    engine.note_on(&state, &setups, 0, 60, 100, 0);
    let events = collect(&mut engine, 0);
    let velocities: Vec<u8> = events
        .iter()
        .filter_map(|(_, kind)| match kind {
            OutputKind::NoteOn { velocity, .. } => Some(*velocity),
            _ => None,
        })
        .collect();
    assert_eq!(velocities, vec![50, 127, 100]);
}

#[test]
fn empty_chords_play_the_root_note_only() {
    let mut state = ChordzState::default();
    state.triggers = vec![Trigger {
        root: 0,
        chord_id: "Root".into(),
        notes: vec![],
        enabled: true,
    }];
    let setups = setup_for(&state);
    let mut engine = Engine::new(48_000.0);
    engine.set_seed(1);

    engine.note_on(&state, &setups, 0, 48, 100, 0);
    assert_eq!(note_ons(&collect(&mut engine, 0)), vec![48]);
}

#[test]
fn transposing_the_scale_moves_every_root() {
    let mut state = ChordzState::default();
    state.transpose_tonic(2);
    assert_eq!(state.tonic, 2);
    assert_eq!(state.triggers[0].root, 2);
    assert_eq!(state.triggers[1].root, 4);
}

#[test]
fn easy_mode_resolution_returns_the_expected_root() {
    let mut state = ChordzState::default();
    state.tonic = 4;
    state.apply_template_by_name("Major");
    state.easy_mode = true;
    assert_eq!(
        resolve(&state, 60),
        Some(Resolution {
            variation: false,
            index: 0,
            root: 64
        })
    );
}

#[test]
fn chord_note_slots_cycle_for_large_chords() {
    let mut state = ChordzState::default();
    state.triggers = vec![Trigger {
        root: 0,
        chord_id: "Lots".into(),
        notes: (0..8)
            .map(|i| ChordNote::new(i * 2, ((i as u8) % 5) + 1))
            .collect(),
        enabled: true,
    }];
    let setups = setup_for(&state);
    let mut engine = Engine::new(48_000.0);
    engine.set_seed(1);

    engine.note_on(&state, &setups, 0, 60, 100, 0);
    let ons = note_ons(&collect(&mut engine, 0));
    assert_eq!(ons.len(), 8);
}

#[test]
fn suggested_triads_for_c_major() {
    let library = chordz_core::builtin_library();
    let major = library
        .templates
        .iter()
        .find(|t| t.name == "Major")
        .unwrap();
    let suggestions = chordz_core::data::suggest_triads(major, 0);
    let ids: Vec<&str> = suggestions.iter().map(|(_, id, _)| id.as_str()).collect();
    assert_eq!(ids, vec!["Maj", "Min", "Min", "Maj", "Maj", "Min", "Dim"]);
}
