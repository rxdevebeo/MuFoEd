//! User settings, kept as JSON in the configuration directory.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

/// How many recent files the start page remembers.
const RECENT_LIMIT: usize = 12;

/// The window's size when nothing has been remembered yet.
pub(crate) const DEFAULT_WIDTH: f32 = 1280.0;
/// The window's size when nothing has been remembered yet.
pub(crate) const DEFAULT_HEIGHT: f32 = 860.0;

fn remember_by_default() -> bool {
    true
}

/// Where the window was, so the next start can put it back.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub(crate) struct WindowPlacement {
    /// Outer left edge, in points.
    pub(crate) x: f32,
    /// Outer top edge, in points.
    pub(crate) y: f32,
    /// Inner width, in points.
    pub(crate) width: f32,
    /// Inner height, in points.
    pub(crate) height: f32,
    /// The window filled the screen.
    pub(crate) maximized: bool,
}

/// Everything the application remembers between runs.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default)]
pub(crate) struct Settings {
    /// Interface language code; `None` follows the system.
    pub(crate) language: Option<String>,
    /// Dark interface.
    pub(crate) dark: bool,
    /// Most recent first.
    pub(crate) recent: Vec<PathBuf>,
    /// Zoom for new tabs, 1.0 = 100 %.
    pub(crate) zoom: f32,
    /// Show the navigation pane.
    pub(crate) navigation: bool,
    /// Show the rulers.
    pub(crate) ruler: bool,
    /// Lay pages side by side, as many as the window and zoom fit.
    pub(crate) pages_across: bool,
    /// Measure and draw with the machine's installed fonts as well.
    pub(crate) system_fonts: bool,
    /// Ollama endpoint for recognition.
    pub(crate) ocr_endpoint: String,
    /// Ollama vision model.
    pub(crate) ocr_model: String,
    /// Recognition language hint, empty for none.
    pub(crate) ocr_language: String,
    /// The name comments are signed with.
    pub(crate) author: String,
    /// Mark misspelt words.
    pub(crate) spelling: bool,
    /// The Ollama model the AI tab writes with; empty chooses one.
    pub(crate) ai_model: String,
    /// The language the AI tab translates into (its English name).
    pub(crate) ai_language: String,
    /// The fidelity report pane was open.
    pub(crate) report: bool,
    /// Put the window back where it was. Missing from an older file means yes.
    #[serde(default = "remember_by_default")]
    pub(crate) remember_window: bool,
    /// The last place the window occupied.
    pub(crate) window: Option<WindowPlacement>,
}

impl Default for Settings {
    fn default() -> Self {
        // The library's own defaults, after OLLAMA_HOST and OLLAMA_MODEL.
        #[cfg(feature = "ocr")]
        let (endpoint, model) = {
            let config = strict_ooxml::OllamaConfig::from_env();
            (config.endpoint, config.model)
        };
        #[cfg(not(feature = "ocr"))]
        let (endpoint, model) = (
            "http://127.0.0.1:11434".to_owned(),
            "qwen3-vl:8b-instruct-ctx16k".to_owned(),
        );
        Self {
            language: None,
            dark: false,
            recent: Vec::new(),
            zoom: 1.0,
            navigation: true,
            ruler: true,
            pages_across: false,
            system_fonts: false,
            ocr_endpoint: endpoint,
            ocr_model: model,
            ocr_language: String::new(),
            spelling: true,
            ai_model: String::new(),
            ai_language: "English".to_owned(),
            report: false,
            remember_window: true,
            window: None,
            author: std::env::var("USERNAME")
                .or_else(|_| std::env::var("USER"))
                .unwrap_or_default(),
        }
    }
}

impl Settings {
    /// Reads the settings file, or the defaults when there is none.
    pub(crate) fn load() -> Self {
        let Some(path) = settings_path() else {
            return Self::default();
        };
        std::fs::read_to_string(path)
            .ok()
            .and_then(|text| serde_json::from_str(&text).ok())
            .unwrap_or_default()
    }

    /// Writes the settings file; a failure is not worth interrupting anyone.
    pub(crate) fn save(&self) {
        let Some(path) = settings_path() else {
            return;
        };
        if let Some(parent) = path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        if let Ok(text) = serde_json::to_string_pretty(self) {
            let _ = std::fs::write(path, text);
        }
    }

    /// Puts `path` first in the recent list.
    pub(crate) fn remember(&mut self, path: &Path) {
        self.recent.retain(|known| known != path);
        self.recent.insert(0, path.to_path_buf());
        self.recent.truncate(RECENT_LIMIT);
    }
}

/// `…/MuFoEd` under the platform's configuration directory.
pub(crate) fn config_dir() -> Option<PathBuf> {
    let base = if cfg!(windows) {
        std::env::var_os("APPDATA").map(PathBuf::from)
    } else if cfg!(target_os = "macos") {
        std::env::var_os("HOME").map(|home| PathBuf::from(home).join("Library/Application Support"))
    } else {
        std::env::var_os("XDG_CONFIG_HOME")
            .map(PathBuf::from)
            .or_else(|| std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".config")))
    }?;
    Some(base.join("MuFoEd"))
}

fn settings_path() -> Option<PathBuf> {
    config_dir().map(|dir| dir.join("settings.json"))
}

/// Where extra language files are looked for: `locales/` beside the
/// executable, then in the configuration directory.
pub(crate) fn locale_dirs() -> Vec<PathBuf> {
    let mut dirs = Vec::new();
    if let Some(dir) = std::env::current_exe()
        .ok()
        .and_then(|exe| exe.parent().map(Path::to_path_buf))
    {
        dirs.push(dir.join("locales"));
    }
    if let Some(dir) = config_dir() {
        dirs.push(dir.join("locales"));
    }
    dirs
}

#[cfg(test)]
mod tests {
    use super::Settings;

    #[test]
    fn an_older_file_still_remembers_the_window() {
        let settings: Settings = serde_json::from_str(r#"{"dark":true,"zoom":1.25}"#).unwrap();
        assert!(settings.dark);
        assert!((settings.zoom - 1.25).abs() < f32::EPSILON);
        assert!(settings.remember_window);
        assert!(settings.window.is_none());
        assert!(!settings.report);
    }
}
