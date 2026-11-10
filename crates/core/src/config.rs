//! Where Glint keeps its things, and the settings the app and its helpers share.

use crate::crawler::Root;
use crate::rules::Custom;
use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

/// `%LOCALAPPDATA%\Glint` (or `$GLINT_HOME`): the index, the model, the settings, the thumbnails.
pub fn home() -> PathBuf {
    if let Some(chosen) = std::env::var_os("GLINT_HOME") {
        return PathBuf::from(chosen);
    }
    let base = std::env::var_os("LOCALAPPDATA")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".local/share")))
        .unwrap_or_else(|| PathBuf::from("."));
    base.join("Glint")
}

pub fn index_file() -> PathBuf {
    home().join("index.db")
}

pub fn models_folder() -> PathBuf {
    std::env::var_os("GLINT_MODELS").map(PathBuf::from).unwrap_or_else(|| home().join("Models"))
}

pub fn config_file() -> PathBuf {
    home().join("config.json")
}

pub fn speed_file() -> PathBuf {
    home().join("speed.json")
}

pub fn cache_folder() -> PathBuf {
    home().join("Cache")
}

fn yes() -> bool {
    true
}

fn default_hotkey() -> String {
    "Ctrl+Shift+Space".into()
}

fn default_device() -> String {
    "auto".into()
}

fn default_theme() -> String {
    "system".into()
}

fn default_accent() -> String {
    "gold".into()
}

fn default_language() -> String {
    "auto".into()
}

fn default_port() -> u16 {
    47_600
}

/// The local search door for other tools. Off unless switched on; it listens on this computer only and wants the
/// token with every request.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct Door {
    #[serde(default)]
    pub enabled: bool,
    #[serde(default = "default_port")]
    pub port: u16,
    #[serde(default)]
    pub token: String,
}

impl Default for Door {
    fn default() -> Door {
        Door { enabled: false, port: default_port(), token: String::new() }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct Config {
    /// The folders to search.
    #[serde(default)]
    pub roots: Vec<Root>,
    #[serde(default)]
    pub rules: CustomRules,
    /// Read the text in pictures and scans.
    #[serde(default = "yes")]
    pub ocr: bool,
    /// `auto` (what the speed check found faster), `cpu` or `gpu`.
    #[serde(default = "default_device")]
    pub device: String,
    #[serde(default = "default_hotkey")]
    pub hotkey: String,
    /// `system`, `light` or `dark`.
    #[serde(default = "default_theme")]
    pub theme: String,
    /// `gold`, `navy` or `violet`.
    #[serde(default = "default_accent")]
    pub accent: String,
    /// `auto`, `en` or `vi`.
    #[serde(default = "default_language")]
    pub language: String,
    #[serde(default)]
    pub autostart: bool,
    /// Index only while the computer is plugged in.
    #[serde(default)]
    pub only_on_power: bool,
    #[serde(default)]
    pub paused: bool,
    /// The first-launch setup is done.
    #[serde(default)]
    pub setup_done: bool,
    #[serde(default)]
    pub door: Door,
}

/// `rules::Custom`, under a name that reads well in the settings file.
pub type CustomRules = Custom;

impl Default for Config {
    fn default() -> Config {
        serde_json::from_str("{}").expect("every field has a default")
    }
}

impl Config {
    pub fn load_from(file: &Path) -> Config {
        std::fs::read(file).ok().and_then(|bytes| serde_json::from_slice(&bytes).ok()).unwrap_or_default()
    }

    pub fn load() -> Config {
        Config::load_from(&config_file())
    }

    /// Written to a temporary file first, so a crash never leaves half a settings file.
    pub fn save_to(&self, file: &Path) -> Result<()> {
        if let Some(folder) = file.parent() {
            std::fs::create_dir_all(folder)?;
        }
        let temporary = file.with_extension("json.tmp");
        std::fs::write(&temporary, serde_json::to_vec_pretty(self)?)?;
        std::fs::rename(&temporary, file)?;
        Ok(())
    }

    pub fn save(&self) -> Result<()> {
        self.save_to(&config_file())
    }

    pub fn root_paths(&self) -> Vec<PathBuf> {
        self.roots.iter().map(|root| root.path.clone()).collect()
    }
}

/// What the speed check measured on this computer: seconds per input on the device it chose.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct Speed {
    /// `cpu` or `gpu`.
    pub device: String,
    pub image: f64,
    pub screenshot: f64,
    pub passage: f64,
    pub sound: f64,
    /// The graphics card's name, when one was tried.
    #[serde(default)]
    pub gpu_name: Option<String>,
    /// How closely the graphics card's vectors agreed with the processor's (cosine).
    #[serde(default)]
    pub agreement: Option<f64>,
    #[serde(default)]
    pub cpu_image: Option<f64>,
    #[serde(default)]
    pub gpu_image: Option<f64>,
}

impl Speed {
    pub fn load() -> Option<Speed> {
        serde_json::from_slice(&std::fs::read(speed_file()).ok()?).ok()
    }

    /// A guess for a computer that hasn't been measured: a mid-range processor.
    pub fn guess() -> Speed {
        Speed {
            device: "cpu".into(), image: 1.5, screenshot: 3.0, passage: 0.4, sound: 3.0, gpu_name: None,
            agreement: None, cpu_image: None, gpu_image: None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn settings_round_trip_and_fill_in_defaults() {
        let folder = tempfile::tempdir().unwrap();
        let file = folder.path().join("config.json");
        std::fs::write(&file, r#"{"roots":[{"path":"C:\\Users\\me\\Pictures"}],"accent":"navy"}"#).unwrap();
        let mut config = Config::load_from(&file);
        assert_eq!(config.roots.len(), 1);
        assert_eq!((config.accent.as_str(), config.hotkey.as_str(), config.ocr), ("navy", "Ctrl+Shift+Space", true));
        assert!(!config.door.enabled);
        config.rules.skip_folders.push("Private".into());
        config.save_to(&file).unwrap();
        assert_eq!(Config::load_from(&file), config);
        // A broken file falls back to the defaults rather than stopping the app.
        std::fs::write(&file, "{ not json").unwrap();
        assert_eq!(Config::load_from(&file), Config::default());
    }
}
