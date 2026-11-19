//! The factory chord library and scale/chord templates.
//!
//! These files were extracted verbatim from the `Chords.txt` and
//! `Scale Templates.txt` string resources embedded in `Chordz.dll`. They are
//! written to the user data folder on first run, exactly like the original
//! plugin does, and re-parsed when the user edits them.

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

use crate::model::{ChordNote, MAX_CHORD_NOTES};

pub const BUILTIN_CHORDS_TXT: &str = include_str!("../data/Chords.txt");
pub const BUILTIN_TEMPLATES_TXT: &str = include_str!("../data/Scale Templates.txt");

pub const CHORDS_FILE: &str = "Chords.txt";
pub const TEMPLATES_FILE: &str = "Scale Templates.txt";
pub const DATA_DIR_NAME: &str = "CrabChordz";
const LEGACY_DATA_DIR_NAME: &str = "ChordzMac";

#[derive(Clone, Debug, Default, PartialEq)]
pub struct ChordType {
    pub name: String,
    pub chord_id: String,
    pub notes: Vec<ChordNote>,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct TemplateDegree {
    /// Semitones above the tonic (0..=11).
    pub offset: u8,
    pub chord_id: String,
    pub notes: Vec<ChordNote>,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Template {
    pub name: String,
    pub degrees: Vec<TemplateDegree>,
    /// Chords for the five black keys, from the part after `|`.
    pub variations: Vec<TemplateDegree>,
}

/// One row in a drop-down menu.
#[derive(Clone, Debug, PartialEq)]
pub enum MenuEntry {
    Group(String),
    Separator,
    Item(usize),
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Library {
    pub chords: Vec<ChordType>,
    pub chord_menu: Vec<MenuEntry>,
    pub templates: Vec<Template>,
    pub template_menu: Vec<MenuEntry>,
}

impl Library {
    pub fn chord(&self, index: usize) -> Option<&ChordType> {
        self.chords.get(index)
    }

    pub fn template(&self, index: usize) -> Option<&Template> {
        self.templates.get(index)
    }

    pub fn template_index(&self, name: &str) -> Option<usize> {
        self.templates.iter().position(|t| t.name == name)
    }
}

/// Parse `Chords.txt`.
///
/// Format: `Name;ID,semitone[,semitone...]` where each semitone may carry an
/// optional `/slot` suffix. `>Group` starts a submenu and `-` is a separator.
pub fn parse_chords(text: &str) -> (Vec<ChordType>, Vec<MenuEntry>) {
    let mut chords = Vec::new();
    let mut menu = Vec::new();

    for raw in text.lines() {
        let line = raw.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        if line == "-" {
            menu.push(MenuEntry::Separator);
            continue;
        }
        if let Some(group) = line.strip_prefix('>') {
            menu.push(MenuEntry::Group(group.trim().to_string()));
            continue;
        }
        let Some((name, body)) = line.split_once(';') else {
            continue;
        };
        let mut parts = body.split(',');
        let chord_id = parts.next().unwrap_or("").trim();
        let mut notes = Vec::new();
        for (index, part) in parts.enumerate() {
            if let Some(note) = parse_chord_note(part, index) {
                if notes.len() < MAX_CHORD_NOTES {
                    notes.push(note);
                }
            }
        }
        menu.push(MenuEntry::Item(chords.len()));
        chords.push(ChordType {
            name: name.trim().to_string(),
            chord_id: chord_id.chars().take(crate::model::MAX_CHORD_ID).collect(),
            notes,
        });
    }

    (chords, menu)
}

fn parse_chord_note(token: &str, index: usize) -> Option<ChordNote> {
    let token = token.trim();
    if token.is_empty() {
        return None;
    }
    let (semi, slot) = match token.split_once('/') {
        Some((semi, slot)) => {
            let slot: u8 = slot.trim().parse().ok()?;
            (semi, Some(slot))
        }
        None => (token, None),
    };
    let semitone: i16 = semi.trim().parse().ok()?;
    let note_no = slot.unwrap_or_else(|| ((index % crate::model::NUM_NOTE_SLOTS) + 1) as u8);
    Some(ChordNote::new(
        semitone.clamp(-48, 48),
        note_no.clamp(1, crate::model::NUM_NOTE_SLOTS as u8),
    ))
}

/// Parse `Scale Templates.txt`.
///
/// Format: `Name;offset,ID,notes...;offset,ID,notes...` with an optional
/// `|offset,ID,notes...;...` section for black-key variations. A degree may
/// also be written scale-only, as just `offset`.
pub fn parse_templates(text: &str) -> (Vec<Template>, Vec<MenuEntry>) {
    let mut templates = Vec::new();
    let mut menu = Vec::new();

    for raw in text.lines() {
        let line = raw.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        if line == "-" {
            menu.push(MenuEntry::Separator);
            continue;
        }
        if let Some(group) = line.strip_prefix('>') {
            menu.push(MenuEntry::Group(group.trim().to_string()));
            continue;
        }
        let Some((name, body)) = line.split_once(';') else {
            continue;
        };
        let (main, variations) = match body.split_once('|') {
            Some((main, variations)) => (main, Some(variations)),
            None => (body, None),
        };
        let degrees = parse_degrees(main);
        if degrees.is_empty() {
            continue;
        }
        menu.push(MenuEntry::Item(templates.len()));
        templates.push(Template {
            name: name.trim().to_string(),
            degrees,
            variations: variations.map(parse_degrees).unwrap_or_default(),
        });
    }

    (templates, menu)
}

fn parse_degrees(body: &str) -> Vec<TemplateDegree> {
    let mut degrees = Vec::new();
    for entry in body.split(';') {
        let entry = entry.trim();
        if entry.is_empty() {
            continue;
        }
        let mut parts = entry.split(',');
        let Ok(offset) = parts.next().unwrap_or("").trim().parse::<i16>() else {
            continue;
        };
        let rest: Vec<&str> = parts.collect();
        let chord_id = rest.first().map(|s| s.trim()).unwrap_or("");
        let note_tokens: &[&str] = if rest.is_empty() {
            &rest[..0]
        } else {
            &rest[1..]
        };
        let mut notes = Vec::new();
        for (index, token) in note_tokens.iter().enumerate() {
            // Auto-numbering is positional, as described in the user guide:
            // the first note is slot 1, the second slot 2, and so on.
            if let Some(note) = parse_degree_note(token, index) {
                if notes.len() < MAX_CHORD_NOTES {
                    notes.push(note);
                }
            }
        }
        degrees.push(TemplateDegree {
            offset: offset.rem_euclid(12) as u8,
            chord_id: chord_id.chars().take(crate::model::MAX_CHORD_ID).collect(),
            notes,
        });
    }
    degrees
}

fn parse_degree_note(token: &str, index: usize) -> Option<ChordNote> {
    let token = token.trim();
    if token.is_empty() {
        return None;
    }
    let (semi, slot) = match token.split_once('/') {
        Some((semi, slot)) => (semi, slot.trim().parse::<u8>().ok()),
        None => (token, None),
    };
    let semitone: i16 = semi.trim().parse().ok()?;
    let note_no = slot.unwrap_or_else(|| ((index % crate::model::NUM_NOTE_SLOTS) + 1) as u8);
    Some(ChordNote::new(
        semitone.clamp(-48, 48),
        note_no.clamp(1, crate::model::NUM_NOTE_SLOTS as u8),
    ))
}

pub fn builtin_library() -> &'static Library {
    static LIBRARY: OnceLock<Library> = OnceLock::new();
    LIBRARY.get_or_init(|| {
        let (chords, chord_menu) = parse_chords(BUILTIN_CHORDS_TXT);
        let (templates, template_menu) = parse_templates(BUILTIN_TEMPLATES_TXT);
        Library {
            chords,
            chord_menu,
            templates,
            template_menu,
        }
    })
}

/// The default location of the editable data files, mirroring the original's
/// `Documents\CodeFN42\Chordz` folder.
pub fn default_data_dir() -> PathBuf {
    let home = std::env::var_os("HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("."));
    let dir = home.join("Documents").join(DATA_DIR_NAME);
    // One-time migration of the pre-rename ChordzMac folder.
    let legacy = home.join("Documents").join(LEGACY_DATA_DIR_NAME);
    if !dir.exists() && legacy.is_dir() {
        let _ = fs::rename(&legacy, &dir);
    }
    dir
}

/// Load the library from `dir`, creating the two text files from the embedded
/// defaults when they do not exist. Parse failures fall back to the built-in
/// library and are returned as a human readable warning.
pub fn load_library(dir: &Path) -> (Library, Vec<String>) {
    let mut warnings = Vec::new();
    if let Err(err) = fs::create_dir_all(dir) {
        warnings.push(format!("Could not create {}: {err}", dir.display()));
        return (builtin_library().clone(), warnings);
    }

    let chords_path = dir.join(CHORDS_FILE);
    let templates_path = dir.join(TEMPLATES_FILE);
    ensure_file(&chords_path, BUILTIN_CHORDS_TXT, &mut warnings);
    ensure_file(&templates_path, BUILTIN_TEMPLATES_TXT, &mut warnings);

    let chords_text = fs::read_to_string(&chords_path).unwrap_or_else(|err| {
        warnings.push(format!("Could not read {}: {err}", chords_path.display()));
        BUILTIN_CHORDS_TXT.to_string()
    });
    let templates_text = fs::read_to_string(&templates_path).unwrap_or_else(|err| {
        warnings.push(format!(
            "Could not read {}: {err}",
            templates_path.display()
        ));
        BUILTIN_TEMPLATES_TXT.to_string()
    });

    let (chords, chord_menu) = parse_chords(&chords_text);
    let (templates, template_menu) = parse_templates(&templates_text);
    if chords.is_empty() {
        warnings.push(format!(
            "{} contained no usable chords, using the built-in library",
            chords_path.display()
        ));
        return (builtin_library().clone(), warnings);
    }
    if templates.is_empty() {
        warnings.push(format!(
            "{} contained no usable templates, using the built-in library",
            templates_path.display()
        ));
        return (builtin_library().clone(), warnings);
    }

    (
        Library {
            chords,
            chord_menu,
            templates,
            template_menu,
        },
        warnings,
    )
}

fn ensure_file(path: &Path, contents: &str, warnings: &mut Vec<String>) {
    if path.exists() {
        return;
    }
    if let Err(err) = fs::write(path, contents) {
        warnings.push(format!("Could not create {}: {err}", path.display()));
    }
}

/// Suggest diatonic triads for the current scale.
///
/// The manual describes the feature as suggesting "diatonic chords based on the
/// selected scale and root note". Triads are built by stacking scale degrees a
/// third apart, which is the standard harmonisation of a scale.
pub fn suggest_triads(template: &Template, tonic: u8) -> Vec<(u8, String, Vec<ChordNote>)> {
    let offsets: Vec<i16> = template.degrees.iter().map(|d| d.offset as i16).collect();
    if offsets.is_empty() {
        return Vec::new();
    }

    let mut suggestions = Vec::new();
    for (i, root_offset) in offsets.iter().enumerate() {
        let third = offsets[(i + 2) % offsets.len()] + 12 * (((i + 2) / offsets.len()) as i16);
        let fifth = offsets[(i + 4) % offsets.len()] + 12 * (((i + 4) / offsets.len()) as i16);
        let mut semis = vec![0, third - root_offset, fifth - root_offset];
        semis.sort_unstable();
        semis.dedup();

        let third_semis = semis.get(1).copied().unwrap_or(4);
        let fifth_semis = semis.get(2).copied().unwrap_or(7);
        let chord_id = match (third_semis, fifth_semis) {
            (4, 7) => "Maj",
            (3, 7) => "Min",
            (3, 6) => "Dim",
            (4, 8) => "Aug",
            (5, 7) => "Sus4",
            (2, 7) => "Sus2",
            _ => "Triad",
        };
        suggestions.push((
            (tonic + *root_offset as u8) % 12,
            chord_id.to_string(),
            semis
                .into_iter()
                .enumerate()
                .map(|(idx, semitone)| ChordNote::new(semitone, (idx + 1) as u8))
                .collect(),
        ));
    }
    suggestions
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_factory_library() {
        let library = builtin_library();
        assert!(library.chords.len() >= 55);
        assert!(library.templates.len() >= 40);
        assert_eq!(library.chords[0].name, "Major");
        assert_eq!(library.chords[0].chord_id, "Maj");
        assert_eq!(
            library.chords[0].notes,
            vec![
                ChordNote {
                    semitone: 0,
                    note_no: 1
                },
                ChordNote {
                    semitone: 4,
                    note_no: 2
                },
                ChordNote {
                    semitone: 7,
                    note_no: 3
                },
            ]
        );
        assert!(library.template_index("Major").is_some());
    }

    #[test]
    fn parses_explicit_note_slots() {
        let (chords, _) = parse_chords("Oct;Oct 2,0,-12,12");
        assert_eq!(
            chords[0].notes[1],
            ChordNote {
                semitone: -12,
                note_no: 2
            }
        );

        let (templates, _) = parse_templates("X;0,Maj,-12/1,0/1,4,7");
        let notes = &templates[0].degrees[0].notes;
        assert_eq!(
            notes[0],
            ChordNote {
                semitone: -12,
                note_no: 1
            }
        );
        assert_eq!(
            notes[1],
            ChordNote {
                semitone: 0,
                note_no: 1
            }
        );
        assert_eq!(
            notes[2],
            ChordNote {
                semitone: 4,
                note_no: 3
            }
        );
    }

    #[test]
    fn parses_scale_only_and_variations() {
        let (templates, _) =
            parse_templates("Minor;0;2;3;5;7;8;10\nMajor - 7;0,Maj,0,4,7|0,Maj7,0,4,7,11");
        assert_eq!(templates[0].degrees.len(), 7);
        assert!(templates[0].degrees[0].notes.is_empty());
        assert_eq!(templates[1].variations.len(), 1);
        assert_eq!(templates[1].variations[0].chord_id, "Maj7");
    }
}
