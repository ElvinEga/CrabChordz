# ChordzMac

A native macOS **VST3 + CLAP** MIDI chord generator, built in Rust as an independent
reimplementation of [CodeFN42 Chordz](https://www.codefn42.com/chordz/) 1.14.

Chordz turns single notes into chords. You pick a scale tonic and a scale/chord
template, and every note inside the chord zone plays the chord assigned to that
scale degree, transposed to the octave you played. Notes outside the zone can be
passed through, transposed by octave, and optionally snapped to the scale.

This project is not a wrapper around the original Windows DLL. It re-derives the
behaviour from the shipped plugin's metadata, its embedded chord/scale data, and
its official user guide, then implements that behaviour from scratch.

---

## What was reverse engineered, and how

The original artifacts are in the parent folder:

| File | Size | SHA-256 |
| --- | --- | --- |
| `Chordz.dll` | 13,633,536 bytes | `5d9e725f470535492860900a0b4628e80ea75039c18dce81c480cd5accbeafa4` |
| `Setup.exe` | 9,583,483 bytes | `1a31961d9556519ae18d0f03c7d098993ef22db98a80218655e33cf1a01a24eb` |
| `Chordz User Guide.pdf` | 1,295,028 bytes | `8916c627d88e1e88a8e498e6be917a7a66a84e3e040a7e08e51c88bfab63aae8` |

All findings below come from inspecting the shipped artifacts with REA
(headers, exports/imports, embedded resources, RTTI, and the manual text). No
decompiler was available in this environment, so there is no reconstructed source
code in this repo. The port is a behavioural reimplementation.

### The binary

- **PE32+ x86-64 DLL**, not .NET. There is no CLI metadata directory, so this is a
  native Delphi/C++Builder plugin built with the DAV (Delphi ASIO/VST) library.
- Exports include `VSTPluginMain` and `main`, plus the Delphi runtime export
  table. Imports include `winmm.dll`, `user32.dll`, `gdi32.dll` and `comctl32.dll`,
  which is consistent with a native VST2 plugin that owns a Win32 window and
  talks to MIDI devices.
- `Setup.exe` is an **Inno Setup 6.0.0** installer (identified from its embedded
  setup engine strings and version resource).

### Plugin metadata (from the embedded Delphi form/resource data)

- Effect/product name: **Chordz**
- Version: **1.14**
- Vendor: **CodeFN42**
- VST2 unique ID: **`fn09`**
- Category: **`vpcSynth`** (instrument)
- Flags: `effFlagsHasEditor`, `CanReplacing`, `ProgramChunks`, `IsSynth`
- `canDo`: `sendVstEvents`, `sendVstMidiEvent`, `receiveVstMidiEvent`, `2in2out`

The `ProgramChunks` flag matters: the chord setup is not stored as VST presets,
it is a binary chunk. ChordzMac stores the equivalent data as JSON in a persisted
state field instead (see “State model” below).

### The 30 host parameters

The original exposes exactly 30 automatable VST parameters — six values for each
of five chord-note slots:

| Slot | Parameters |
| --- | --- |
| Notes 1–5 | Start, Start Random, End, End Random, Velocity Scale, Velocity Random |

ChordzMac reproduces this parameter set and ordering. A unit test asserts that the
plugin's parameter map contains 30 entries
(`exposes_the_thirty_original_parameters`).

### Setup fields (from `TChordzSetup` RTTI)

REA recovered the field names of the setup class:

`Tonic`, `Triggers`, `NoteSetup`, `EasyMode`, `FollowTriggerNotes`,
`OctaveRangeFrom`, `OctaveRangeTo`, `TransposeOctave`,
`TransposeOctaveAbove`, `TransposeOctaveBelow`, `ScaleTemplateName`,
`ScaleTemplateNo`, `RelativeTiming`, `SameRandomVelocity`,
`RandomNoteAssignment`, `ActivePanel`.

These map one-to-one onto `ChordzState` in
[`crates/chordz-core/src/model.rs`](crates/chordz-core/src/model.rs). The only
deliberate difference is `lowest_octave`, which the manual describes as a display
setting for octave numbering (-2..7, -1..8, or 0..9).

### Embedded data files

Two text resources are compiled into the DLL and written to the user's Documents
folder on first run. Both were recovered byte-for-byte from the DLL's
length-prefixed Delphi strings and are checked into this repo:

- [`crates/chordz-core/data/Chords.txt`](crates/chordz-core/data/Chords.txt) —
  66 lines: 58 chord types, 8 submenu headers (`>Dyads`, `>Triads`, ...),
  separated by `-` lines.
- [`crates/chordz-core/data/Scale Templates.txt`](crates/chordz-core/data/Scale%20Templates.txt) —
  51 lines: scale-only templates, scale+chord templates, and black-key variation
  sets after `|`.

The parser accepts the same syntax as the originals, including the optional
`/slot` suffix that assigns a chord note to one of the five timing slots
(`0,4,7` versus `-12/1,0/1,4,7`).

### The `.codefn42p` preset format

Recovered from the four factory presets in [`Presets/`](../Presets) and the
`LoadPresetFromStream` / `SavePresetToStream` / `TTriggerItem` strings in the DLL:

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

ChordzMac reads and writes this format
([`crates/chordz-core/src/preset/codefn42p.rs`](crates/chordz-core/src/preset/codefn42p.rs)),
converting absolute chord notes to root-relative offsets and back. All four
factory presets are checked in as test fixtures and verified field-by-field,
including a playback test that sounds the stored Orion voicing through the
engine. Compressed presets (`FUsePresetCompression` in the DLL) are detected
and rejected with a hint to re-save uncompressed.

### Behaviour, from the official manual

Key statements the implementation follows:

- Chords are built relative to the trigger note's octave, so the same trigger in
  a higher octave produces a higher chord.
- **Easy mode**: white keys play the seven scale degrees with C always as the
  tonic; black keys play chord variations when the template provides them.
- With easy mode off, trigger notes are not remapped to other keys, and notes
  that are not in the scale are silent.
- Three keyboard zones: the chord zone and one zone below and above it. The outer
  zones play single notes and can each be transposed by whole octaves.
- “Follow Trigger Notes” forces single notes outside the chord zone to stay in
  the selected scale; in easy mode it follows the trigger mapping.
- Start/end delay and velocity scaling/randomisation are per note slot.
- Start/end values can include a random range (Alt-drag in the original).
- Relative timing makes delays accumulate across the chord, producing strummed
  chords.
- “Same random velocity” shares one random draw across all notes of a chord.
- “Random note assignment” assigns chord notes to random slots.
- If a trigger has no chord notes, the root note is generated.
- Chord IDs are limited to 12 characters.
- The chord-note keyboard spans five octaves, and the middle octave matches the
  trigger octave.

---

## Implementation

### Workspace

```
chordz-mac/
├── crates/chordz-core/     # data model, parsers, MIDI engine, presets (no plugin deps)
├── crates/chordz-plugin/   # nih-plug VST3/CLAP wrapper + egui editor
├── xtask/                  # nih-plug bundler
└── bundler.toml            # package name -> "ChordzMac" display name
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

- Data files are read from `~/Documents/ChordzMac/` (`Chords.txt`,
  `Scale Templates.txt`), created from the embedded originals on first use.
- Presets live in `~/Documents/ChordzMac/Presets/`. Both the original Chordz
  format (`.codefn42p`, read **and** write) and ChordzMac's diffable JSON format
  (`.json`) are supported; the Presets tab lists both and lets you pick the
  save format (`.codefn42p` is the default, so presets can go back to the
  Windows plugin).
- Copy the original Windows presets into that folder (or save new ones from the
  editor) and they appear in the list.
- The editor has a “Reload library” button so edits to the text files are picked
  up without restarting the host.
- A preset stores both the setup and the five note setups; loading applies the
  note setups through the host parameter API so automation stays consistent.

### Editor

The egui editor has four pages:

1. **Chords** — tonic selector, transpose buttons, scale/chord template menu,
   clickable trigger keyboard with chord IDs and hover preview, per-degree chord
   assignment from the factory library, black-key variations, easy mode, follow
   trigger notes, relative timing, same random velocity, random note assignment,
   octave zones and transposes, “Suggest triads”, and “Reset to Major”.
2. **Note Timing** — sliders for all 30 original parameters.
3. **Presets** — save/load `.codefn42p` (original Chordz) and `.json` presets,
   shows the data folder.
4. **Help** — behaviour notes and host routing instructions.

### Tests

```shell
cargo test
```

- 31 tests in `chordz-core` (parsers, model, engine, JSON preset persistence,
  and 8 `.codefn42p` tests against the four factory presets: field-level
  decoding, encode/decode round-trips, file-API round-trip, playback of a
  decoded voicing, and rejection of corrupt/compressed files).
- 6 tests in `chordz-plugin` that drive the real `Plugin::process` through a mock
  host context: parameter count, chord expansion, easy-mode mapping, out-of-zone
  pass-through/transposition, editor preview, and host state round-tripping.

---

## Building

Requirements: macOS, a recent stable Rust toolchain, and Xcode command line
tools.

```shell
cd chordz-mac
cargo xtask bundle chordz-mac --release
```

The bundles are written to `target/bundled/`:

- `ChordzMac.vst3`
- `ChordzMac.clap`

The bundler creates the required `Contents/Info.plist` and `Contents/PkgInfo` and
ad-hoc codesigns both bundles on macOS.

### Installing

```shell
cp -R target/bundled/ChordzMac.vst3 ~/Library/Audio/Plug-Ins/VST3/
cp -R target/bundled/ChordzMac.clap ~/Library/Audio/Plug-Ins/CLAP/
```

Restart the DAW (or rescan plugins). Both bundles are Apple Silicon native; build
with `--target x86_64-apple-darwin` if you need an Intel build.

### Verifying the bundle

```shell
codesign --verify --deep --strict target/bundled/ChordzMac.vst3
plutil -lint target/bundled/ChordzMac.vst3/Contents/Info.plist
```

The produced bundles were loaded in-process to check the plugin registry:

- CLAP: `clap_entry` reports CLAP 1.2.2 and one plugin,
  `com.chordzmac.chordz` / `ChordzMac` / `ChordzMac` 0.1.0, features
  `note-effect` and `utility`.
- VST3: `GetPluginFactory` reports one `Audio Module Class` named `ChordzMac`
  with CID `436872647a4d61633030303030303031` (`ChrdzMac00000001`), which matches
  the declared `VST3_CLASS_ID`.

Both bundles are ad-hoc signed and `codesign --verify --deep --strict` passes.

---

## Using it in a DAW

ChordzMac is a MIDI processor, so the plugin must sit *before* an instrument:

- **Bitwig Studio** — insert ChordzMac as a note FX before the instrument, or use
  the CLAP version's MIDI out routed to an instrument track.
- **REAPER** — put ChordzMac on a track, enable MIDI output to the instrument
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
- **Simplified chord editing.** The original has a five-octave chord-note
  keyboard where individual notes are toggled and assigned slot numbers. ChordzMac
  v1 assigns whole chords from `Chords.txt`/templates and previews them; per-note
  editing and copy/paste between triggers are not exposed in the UI.
- **No drag-and-drop preset browser or `.fxp` import.** The original's binary
  VST program chunks are not compatible. `.codefn42p` preset files are fully
  supported (read/write); compressed presets must be re-saved uncompressed in
  the Windows plugin first.
- The **visual design is new**. Only behaviour and parameter layout are modelled on
  the original.
- The original's “suggest chords” list is not a separate resource in the DLL; the
  implementation harmonises the selected scale in thirds, which matches the
  manual's description of the feature.

---

## License

MIT for this reimplementation. The original Chordz, its name, its user guide and
its embedded data files belong to CodeFN42. The chord/scale text data is included
here because it is required for behavioural compatibility; keep that in mind if
you redistribute this project commercially.
