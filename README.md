# CrabChordz

A native macOS **VST3 + CLAP** MIDI chord generator, built in Rust as an independent
reimplementation of [CodeFN42 Chordz](https://www.codefn42.com/chordz/) 1.14.

Chordz turns single notes into chords. You pick a scale tonic and a scale/chord
template, and every note inside the chord zone plays the chord assigned to that
scale degree, transposed to the octave you played. Notes outside the zone can be
passed through, transposed by octave, and optionally snapped to the scale.

This project is an independent, from-scratch reimplementation. It is based on the
publicly available Chordz user guide, the documented behaviour of the original
plugin, and the file formats that the original produces. It is not a wrapper
around, or a derivative of, the Windows binary.

> **Renamed from ChordzMac.** The CLAP ID and VST3 class ID changed with the
> rename, so DAW projects saved with ChordzMac will not recall the plugin.
> The data folder `~/Documents/ChordzMac` is moved to `~/Documents/CrabChordz`
> automatically on first launch, and all existing `.codefn42p` and `.json`
> presets keep loading.

---

## Compatibility reference

CrabChordz aims to match the behaviour and data formats of Chordz 1.14 so that
existing presets and workflows carry over. The information below describes what
CrabChordz reproduces.

### Plugin metadata

- Product name: **Chordz**
- Version: **1.14**
- Vendor: **CodeFN42**

The original stores its chord setup as a binary chunk rather than as VST
programs. CrabChordz instead stores the equivalent setup as JSON in a persisted
state field, which keeps the data diffable and portable (see "State model").

### The 30 host parameters

The original exposes 30 automatable parameters — six values for each of five
chord-note slots:

| Slot | Parameters |
| --- | --- |
| Notes 1–5 | Start, Start Random, End, End Random, Velocity Scale, Velocity Random |

CrabChordz reproduces this parameter set and ordering. A unit test asserts that
the plugin's parameter map contains 30 entries
(`exposes_the_thirty_original_parameters`).

### Setup fields

The setup the original's editor manages covers:

`Tonic`, `Triggers`, `NoteSetup`, `EasyMode`, `FollowTriggerNotes`,
`OctaveRangeFrom`, `OctaveRangeTo`, `TransposeOctave`,
`TransposeOctaveAbove`, `TransposeOctaveBelow`, `ScaleTemplateName`,
`ScaleTemplateNo`, `RelativeTiming`, `SameRandomVelocity`,
`RandomNoteAssignment`, `ActivePanel`.

These map one-to-one onto `ChordzState` in
[`crates/chordz-core/src/model.rs`](crates/chordz-core/src/model.rs). The only
deliberate difference is `lowest_octave`, which the manual describes as a display
setting for octave numbering (-2..7, -1..8, or 0..9).

### Chord and scale template data

Two text resources accompany the original: a chord list (`Chords.txt`) and a
scale/scale-chord template list (`Scale Templates.txt`). CrabChordz requires both
to be present at runtime and seeds compatible starter files on first run.

- `Chords.txt` — 66 lines: 58 chord types, 8 submenu headers (`>Dyads`,
  `>Triads`, ...), separated by `-` lines.
- `Scale Templates.txt` — 51 lines: scale-only templates, scale+chord
  templates, and black-key variation sets after `|`.

The parser accepts the same syntax as the originals, including the optional
`/slot` suffix that assigns a chord note to one of the five timing slots
(`0,4,7` versus `-12/1,0/1,4,7`).

For full fidelity, users may replace the seeded starter files with their own
copies from an original Chordz installation by placing them in
`~/Documents/CrabChordz/`. Because these files belong to CodeFN42, redistributors
who need strict licensing hygiene should consider shipping only the parser and
letting users supply their own copies — see "License".

### The `.codefn42p` preset format

The `.codefn42p` container is documented below so that CrabChordz can read and
write the same presets. Publicly distributed Chordz presets were used as test
fixtures to validate the parser.

```text
"mmst"  u32 payload length (LE)  "TPF0"  <Delphi TWriter token stream>
```

The body is a Delphi `TWriter` value stream (token 1 = list terminated by 0,
2/3/4/17 = signed ints, 5 = 80-bit `Extended` float, 6 = ANSI string,
8/9 = false/true, 16 = wide string, 18 = UTF-8 string). Top level:

1. `["1.03", preset name, "", ""]` — format version, name, two reserved strings.
2. The setup, seven values:
   - `[ActivePanel]`
   - `[OctaveRangeFrom, OctaveRangeTop, TransposeOctave, TransposeOctaveAbove,
     TransposeOctaveBelow, FollowTriggerNotes]` (octaves stored relative to the
     lowest-octave setting)
   - `[ScaleTemplateName, ScaleTemplateNo, Tonic, EasyMode]` — the number is a
     menu index that counts duplicate names (e.g. scale-only "Minor" is 29);
     the name is authoritative
   - `[active trigger index]`
   - twelve trigger entries in C-major key order (white keys = scale degrees,
     black keys = variations/extra degrees): `[[RootNote, ChordID],
     [[semitone, slot], ...]]`. `RootNote = -1` marks an unassigned key. Chord
     notes are absolute semitones from C and may be negative; the slot is
     0-based.
   - five note setups × six `Extended` floats: Start, Start Random, End,
     End Random, Velocity Scale, Velocity Random
   - `[RelativeTiming, RandomNoteAssignment, SameRandomVelocity]`
3. An empty trailing list.

CrabChordz reads and writes this format
([`crates/chordz-core/src/preset/codefn42p.rs`](crates/chordz-core/src/preset/codefn42p.rs)),
converting absolute chord notes to root-relative offsets and back. Preset
fixtures are verified field-by-field, including a playback test that sounds the
stored Orion voicing through the engine. Compressed presets (saved with the
original's compression option enabled) are detected and rejected with a hint to
re-save uncompressed.

### Behaviour

Key statements the implementation follows:

- Chords are built relative to the trigger note's octave, so the same trigger in
  a higher octave produces a higher chord.
- **Easy mode**: white keys play the seven scale degrees with C always as the
  tonic; black keys play chord variations when the template provides them.
- With easy mode off, trigger notes are not remapped to other keys, and notes
  that are not in the scale are silent.
- Three keyboard zones: the chord zone and one zone below and above it. The outer
  zones play single notes and can each be transposed by whole octaves.
- "Follow Trigger Notes" forces single notes outside the chord zone to stay in
  the selected scale; in easy mode it follows the trigger mapping.
- Start/end delay and velocity scaling/randomisation are per note slot.
- Start/end values can include a random range (Alt-drag in the original).
- Relative timing makes delays accumulate across the chord, producing strummed
  chords.
- "Same random velocity" shares one random draw across all notes of a chord.
- "Random note assignment" assigns chord notes to random slots.
- If a trigger has no chord notes, the root note is generated.
- Chord IDs are limited to 12 characters.
- The chord-note keyboard spans five octaves, and the middle octave matches the
  trigger octave.

---

## Implementation

### Workspace

```
crabchordz/
├── crates/chordz-core/     # data model, parsers, MIDI engine, presets (no plugin deps)
├── crates/chordz-plugin/   # nih-plug VST3/CLAP wrapper + egui editor
├── xtask/                  # nih-plug bundler
└── bundler.toml            # package name -> "CrabChordz" display name
```

`chordz-core` has no dependency on nih-plug, egui, or any plugin API. That keeps
the musical behaviour testable and reusable.

### Audio and MIDI path

- 2-in/2-out pass-through audio (several hosts refuse to load plugins with zero
  audio buses; the buffers are left untouched).
- MIDI in and MIDI out, both `MidiConfig::Basic` (notes only).
- The engine keeps its own sample clock, so scheduled note-ons/offs are emitted at
  the correct sample offset within the host block.
- A voice tracks each trigger note's chord notes and their planned start/end
  times. Note-off adds the end delay per note; notes whose delay extends past the
  block boundary are queued for the next block.
- Retriggered notes emit a note-off before the new note-on to avoid stuck notes.

### State model

| Data | Storage |
| --- | --- |
| Start/start-random/end/end-random/velocity scale/velocity random for 5 slots | 30 host parameters (`slots_1_start`, ... `slots_5_vel_rand`) |
| Tonic, triggers, variations, easy mode, zones, transposes, timing flags, template name | `#[persist = "chordz-state"]` JSON chunk |
| Editor size | `#[persist = "editor-state"]` |

The editor writes timing values into the persisted setup as well, so saved state
and presets stay coherent even though the timing values are host parameters.

### Presets and data files

- Data files are read from `~/Documents/CrabChordz/` (`Chords.txt`,
  `Scale Templates.txt`) and seeded with starter files on first use. Users can
  replace them with their own copies from an original Chordz installation.
- Presets live in `~/Documents/CrabChordz/Presets/`. Both the original Chordz
  format (`.codefn42p`, read **and** write) and CrabChordz's diffable JSON format
  (`.json`) are supported; the Presets tab lists both and lets you pick the
  save format (`.codefn42p` is the default, so presets can go back to the
  Windows plugin).
- Copy your existing Windows presets into that folder (or save new ones from the
  editor) and they appear in the list.
- The editor has a "Reload library" button so edits to the text files are picked
  up without restarting the host.
- A preset stores both the setup and the five note setups; loading applies the
  note setups through the host parameter API so automation stays consistent.

### MIDI progressions

The Presets tab also loads chord progressions from `.mid` files
([`crates/chordz-core/src/midi.rs`](crates/chordz-core/src/midi.rs)):

- Files live in `~/Documents/CrabChordz/Midi/`. Seven example progressions are
  embedded in the binary and seeded on first run; **Import** copies any other
  `.mid` in (auto-renaming collisions), and files can be dropped onto the
  editor window.
- The analyser parses format 0/1 MIDI, clusters strummed note-ons (~40 ticks at
  96 ticks/beat), takes the lowest note as the root and skips single-note
  melody clusters. Distinct chords become one trigger each in order of
  appearance, with voicings kept exactly as played (bass octaves included) and
  chord IDs matched against `Chords.txt` where possible.
- The key comes from the filename ("F Minor"); files without one get a
  pitch-class histogram estimate. The preview shows the detected key with
  tonic/mode overrides before you load.
- Loading replaces the chord content only — zones, transposes, easy mode and
  all timing values survive. The result is ordinary state, so it can be saved
  as a `.codefn42p` or `.json` preset like any other setup.

### Editor

The editor is modelled on the original Chordz window: charcoal panels with teal
accents, a fixed 1160×700 design canvas that is proportionally zoomed to fit the
resizable host window, and a header with a hamburger menu, the preset name
(click it to open the load dialog) and an info panel that describes the hovered
control or the selected trigger.

- **Menu** — Init (loads an `init` preset if one exists), Load/Save Preset,
  Lowest Octave (-2/-1/0), Suggest triads, Reset to Major, Reload library, About.
- **Chord Setup** — tonic selector and ± transpose, template dropdown with an
  explicit Apply, Easy Mode; a one-octave trigger keyboard (click selects,
  double-click renames the chord ID, right-click edits root/chord/copy/paste/
  clear, Ctrl+click and hover preview); octave range and transpose controls;
  and a five-octave chord-note keyboard where clicking toggles notes (auto slot
  assignment), right-click sets a note's slot, and Ctrl+click previews a single
  note.
- **Timing / Velocity** — five slot columns with delay slider boxes (hold Alt
  to edit the amber random range) and velocity knobs. Drag to edit, Shift for
  fine control, double-click or right-click to enter a value, Ctrl+click resets
  to the default. The relative timing / same random velocity / random note
  assignment flags live here.
- **Presets** — a slim list tab; the main load dialog has **Presets / MIDI**
  tabs and shows files in three alphabetical columns like the original, with
  OK/Cancel, double-click to load, and right-click Delete (with confirmation) /
  Show in Folder. Both `.codefn42p` and `.json` formats can be saved. A MIDI
  Progressions section imports `.mid` files, previews the extracted chords and
  key, and loads them into the setup.

### Tests

```shell
cargo test
```

- 40 tests in `chordz-core` (parsers, model, engine, JSON preset persistence,
  8 `.codefn42p` tests against preset fixtures, and 9 MIDI tests against the
  seven embedded progressions: parsing/tempo, filename keys, exact-voicing
  extraction, melody skipping, dedupe, key estimation, state-application scope,
  engine playback of an extracted chord, and seeding/import).
- 6 tests in `chordz-plugin` that drive the real `Plugin::process` through a mock
  host context: parameter count, chord expansion, easy-mode mapping, out-of-zone
  pass-through/transposition, editor preview, and host state round-tripping.

---

## Building

Requirements: macOS, a recent stable Rust toolchain, and Xcode command line
tools.

```shell
cd crabchordz
cargo xtask bundle crabchordz --release
```

The bundles are written to `target/bundled/`:

- `CrabChordz.vst3`
- `CrabChordz.clap`

The bundler creates the required `Contents/Info.plist` and `Contents/PkgInfo` and
ad-hoc codesigns both bundles on macOS.

### Installing

```shell
cp -R target/bundled/CrabChordz.vst3 ~/Library/Audio/Plug-Ins/VST3/
cp -R target/bundled/CrabChordz.clap ~/Library/Audio/Plug-Ins/CLAP/
```

Restart the DAW (or rescan plugins). Both bundles are Apple Silicon native; build
with `--target x86_64-apple-darwin` if you need an Intel build.

### Verifying the bundle

```shell
codesign --verify --deep --strict target/bundled/CrabChordz.vst3
plutil -lint target/bundled/CrabChordz.vst3/Contents/Info.plist
```

The produced bundles were loaded in-process to check the plugin registry:

- CLAP: `clap_entry` reports CLAP 1.2.2 and one plugin,
  `com.crabchordz.crabchordz` / `CrabChordz` / `CrabChordz` 0.1.0, features
  `note-effect` and `utility`.
- VST3: `GetPluginFactory` reports one `Audio Module Class` named `CrabChordz`
  with CID `4372616243686f72647a303030303031` (`CrabChordz000001`), which matches
  the declared `VST3_CLASS_ID`.

Both bundles are ad-hoc signed and `codesign --verify --deep --strict` passes.

---

## Using it in a DAW

CrabChordz is a MIDI processor, so the plugin must sit *before* an instrument:

- **Bitwig Studio** — insert CrabChordz as a note FX before the instrument, or use
  the CLAP version's MIDI out routed to an instrument track.
- **REAPER** — put CrabChordz on a track, enable MIDI output to the instrument
  track (or use the track's MIDI routing).
- **Ableton Live** — use the **CLAP** build on a MIDI track. Live's VST3
  implementation does not expose MIDI output from instrument plugins, so the VST3
  build will load but its generated notes cannot be routed to another track.
- **Logic Pro / GarageBand** — these hosts do not load VST3 or CLAP MIDI effects;
  there is no AU build (see Limitations).

After inserting the plugin, play notes in the chord zone. With the default state
(C major) and easy mode off, playing C4 outputs C4–E4–G4.

---

## Differences and limitations

- **No VST2 or AU build.** nih-plug exports VST3 and CLAP only. VST2 requires a
  licensed SDK; AU would need a separate wrapper and host validation.
- **No MIDI CC learn.** The original can learn parameters from MIDI CC. Supporting
  that in VST3 requires exposing 16×130 CC parameters, so v1 relies on host
  automation. `MidiConfig::Basic` is used for this reason.
- **No drag-and-drop preset browser or `.fxp` import.** The original's binary
  VST program chunks are not compatible. `.codefn42p` preset files are fully
  supported (read/write); compressed presets must be re-saved uncompressed in
  the Windows plugin first.
- The original's "suggest chords" list is not exposed as a standalone resource;
  the implementation harmonises the selected scale in thirds, which matches the
  manual's description of the feature.

---

## License

MIT for this reimplementation. "Chordz", its name, its user guide and its
chord/scale text data belong to CodeFN42. The chord/scale data files are included
only so that the plugin has usable starter data out of the box; if you
redistribute this project commercially, consider shipping the code without those
data files and having users supply their own copies from an original Chordz
installation. No original binaries, installers, or documentation are
redistributed with this project.