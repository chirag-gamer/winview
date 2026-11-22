//! User settings (FR-31/32), persisted via tauri-plugin-store.

use serde::{Deserialize, Serialize};
use tauri::AppHandle;
use tauri_plugin_store::StoreExt;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Mods {
    pub shift: bool,
    pub ctrl: bool,
    pub alt: bool,
}

impl Mods {
    pub fn none() -> Self {
        Self { shift: false, ctrl: false, alt: false }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    /// Master switch: when false the hook passes everything through (FR-31).
    pub enabled: bool,
    /// Trigger virtual-key code (VK_SPACE by default, FR-1).
    pub trigger_vk: u32,
    pub trigger_mods: Mods,
    /// Trigger mode (D-014 / FR-34): "toggle" (press → show, press → hide)
    /// or "hold" (visible only while the key is held; release hides).
    pub trigger_mode: String,
    /// Popup content background: "mica" | "light" | "dark" | "#rrggbb" (FR-13).
    pub background: String,
    /// system | light | dark (FR-32).
    pub theme: String,
    /// File-category switches (FR-32); extension of this map follows preview kinds.
    pub categories: std::collections::BTreeMap<String, bool>,
    /// autostart toggle (wired in 7.3, FR-32).
    pub autostart: bool,
}

impl Default for Settings {
    fn default() -> Self {
        let categories: [(&str, bool); 12] = [
            ("code", true),
            ("text", true),
            ("markdown", true),
            ("json", true),
            ("table", true),
            ("sqlite", true),
            ("log", true),
            ("env", true),
            ("sql", true),
            ("notebook", true),
            ("archive", true),
            ("media", true),
        ];
        Self {
            enabled: true,
            trigger_vk: 0x20, // VK_SPACE
            trigger_mods: Mods::none(),
            trigger_mode: "toggle".into(),
            background: "mica".into(),
            theme: "system".into(),
            categories: categories.into_iter().map(|(k, v)| (k.to_string(), v)).collect(),
            autostart: false,
        }
    }
}

impl Settings {
    pub fn category_enabled(&self, kind: &str) -> bool {
        self.categories.get(kind).copied().unwrap_or(true)
    }
}

const STORE_FILE: &str = "settings.json";
const STORE_KEY: &str = "settings";

/// True when no settings have ever been persisted (first run → wizard, FR-35).
pub fn is_first_run(app: &AppHandle) -> bool {
    match app.store(STORE_FILE) {
        Ok(store) => store.get(STORE_KEY).is_none(),
        Err(_) => true,
    }
}

/// Load settings from the store, falling back to defaults.
pub fn load(app: &AppHandle) -> Settings {
    let store = match app.store(STORE_FILE) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("[settings] store open failed: {e}; using defaults");
            return Settings::default();
        }
    };
    match store.get(STORE_KEY) {
        Some(value) => serde_json::from_value(value).unwrap_or_else(|e| {
            eprintln!("[settings] corrupt settings ({e}); using defaults");
            Settings::default()
        }),
        None => Settings::default(),
    }
}

/// Persist settings.
pub fn save(app: &AppHandle, settings: &Settings) -> Result<(), String> {
    let store = app.store(STORE_FILE).map_err(|e| e.to_string())?;
    store.set(
        STORE_KEY,
        serde_json::to_value(settings).map_err(|e| e.to_string())?,
    );
    store.save().map_err(|e| e.to_string())?;
    Ok(())
}
