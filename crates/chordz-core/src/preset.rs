//! Preset files.
//!
//! CrabChordz stores its own presets as JSON, and can also read and write the
//! original Windows plugin's `.codefn42p` files. The original keeps presets in
//! `Documents/CodeFN42/Chordz/Presets`; we use the same folder name.

use std::fs;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::data::default_data_dir;
use crate::model::{ChordzState, NoteSetup, NUM_NOTE_SLOTS};

pub mod codefn42p;

pub use codefn42p::EXTENSION as CODEFN42P_EXTENSION;

// Kept as-is so JSON presets saved under the old ChordzMac name still load.
pub const PRESET_FORMAT: &str = "chordzmac-preset";
pub const PRESET_VERSION: u32 = 1;

/// File formats accepted by the preset browser.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PresetFormat {
    /// Original Windows Chordz preset (`.codefn42p`).
    Codefn42p,
    /// CrabChordz's diffable JSON preset (`.json`).
    Json,
}

impl PresetFormat {
    pub fn extension(self) -> &'static str {
        match self {
            PresetFormat::Codefn42p => codefn42p::EXTENSION,
            PresetFormat::Json => "json",
        }
    }

    pub fn from_path(path: &Path) -> Option<Self> {
        match path.extension()?.to_str()? {
            "codefn42p" => Some(PresetFormat::Codefn42p),
            "json" => Some(PresetFormat::Json),
            _ => None,
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Preset {
    pub format: String,
    pub version: u32,
    pub name: String,
    pub state: ChordzState,
    /// Timing/velocity values, which are host parameters rather than part of
    /// the chord setup model in the plugin.
    #[serde(default)]
    pub note_setup: [NoteSetup; NUM_NOTE_SLOTS],
}

impl Preset {
    pub fn new(
        name: impl Into<String>,
        state: ChordzState,
        note_setup: [NoteSetup; NUM_NOTE_SLOTS],
    ) -> Self {
        Self {
            format: PRESET_FORMAT.to_string(),
            version: PRESET_VERSION,
            name: name.into(),
            state,
            note_setup,
        }
    }

    pub fn is_valid(&self) -> bool {
        self.format == PRESET_FORMAT && self.version <= PRESET_VERSION
    }
}

pub fn preset_dir() -> PathBuf {
    default_data_dir().join("Presets")
}

pub fn sanitize_file_name(name: &str) -> String {
    let mut out: String = name
        .chars()
        .map(|c| {
            if c.is_alphanumeric() || matches!(c, ' ' | '-' | '_' | '(' | ')' | '.') {
                c
            } else {
                '_'
            }
        })
        .collect();
    out = out.trim().trim_matches('.').to_string();
    if out.is_empty() {
        out = "preset".to_string();
    }
    out
}

pub fn save_preset(preset: &Preset, dir: &Path) -> Result<PathBuf, String> {
    save_preset_as(preset, dir, PresetFormat::Json)
}

/// Save a preset using one of the supported formats. The file name is derived
/// from the preset name and the format's extension.
pub fn save_preset_as(
    preset: &Preset,
    dir: &Path,
    format: PresetFormat,
) -> Result<PathBuf, String> {
    fs::create_dir_all(dir).map_err(|err| format!("Could not create {}: {err}", dir.display()))?;
    let path = dir.join(format!(
        "{}.{}",
        sanitize_file_name(&preset.name),
        format.extension()
    ));
    match format {
        PresetFormat::Json => {
            let json = serde_json::to_string_pretty(preset)
                .map_err(|err| format!("Could not serialize preset: {err}"))?;
            fs::write(&path, json)
                .map_err(|err| format!("Could not write {}: {err}", path.display()))?;
        }
        PresetFormat::Codefn42p => {
            let bytes = codefn42p::encode(preset);
            fs::write(&path, bytes)
                .map_err(|err| format!("Could not write {}: {err}", path.display()))?;
        }
    }
    Ok(path)
}

/// Load a preset, choosing the reader from the file extension.
pub fn load_preset_file(path: &Path) -> Result<Preset, String> {
    match PresetFormat::from_path(path) {
        Some(PresetFormat::Codefn42p) => {
            let bytes = fs::read(path)
                .map_err(|err| format!("Could not read {}: {err}", path.display()))?;
            codefn42p::decode(&bytes)
                .map_err(|err| format!("{} is not a valid Chordz preset: {err}", path.display()))
        }
        Some(PresetFormat::Json) => load_preset(path),
        None => Err(format!(
            "{} is not a supported preset file (.codefn42p or .json)",
            path.display()
        )),
    }
}

pub fn load_preset(path: &Path) -> Result<Preset, String> {
    let json = fs::read_to_string(path)
        .map_err(|err| format!("Could not read {}: {err}", path.display()))?;
    let preset: Preset = serde_json::from_str(&json)
        .map_err(|err| format!("{} is not a valid CrabChordz preset: {err}", path.display()))?;
    if !preset.is_valid() {
        return Err(format!(
            "{} uses an unsupported preset format or version",
            path.display()
        ));
    }
    Ok(preset)
}

/// List presets in a directory, sorted by name. Both original Chordz presets
/// and CrabChordz JSON presets are included.
pub fn list_presets(dir: &Path) -> Vec<PathBuf> {
    let Ok(entries) = fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut paths: Vec<PathBuf> = entries
        .flatten()
        .map(|entry| entry.path())
        .filter(|path| PresetFormat::from_path(path).is_some())
        .collect();
    paths.sort();
    paths
}
