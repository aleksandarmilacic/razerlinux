//! Profile management for RazerLinux
//!
//! Handles saving and loading mouse configuration profiles to TOML files.

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;
use tracing::info;

/// A mouse configuration profile
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Profile {
    /// Profile name
    pub name: String,

    /// Profile description
    #[serde(default)]
    pub description: String,

    /// DPI settings
    pub dpi: DpiSettings,

    /// Polling rate in Hz (125, 500, 1000)
    #[serde(default = "default_polling_rate")]
    pub polling_rate: u16,

    /// LED brightness (0-255)
    #[serde(default = "default_brightness")]
    pub brightness: u8,

    /// Software remapping settings (evdev/uinput)
    #[serde(default)]
    pub remap: RemapSettings,
    
    /// Macro definitions
    #[serde(default)]
    pub macros: Vec<Macro>,
}

fn default_polling_rate() -> u16 {
    1000
}

fn default_brightness() -> u8 {
    255
}

/// DPI settings
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DpiSettings {
    /// X-axis DPI
    pub x: u16,
    /// Y-axis DPI  
    pub y: u16,
    /// Whether X and Y are linked
    #[serde(default = "default_linked")]
    pub linked: bool,
}

fn default_linked() -> bool {
    true
}

impl Default for Profile {
    fn default() -> Self {
        Self {
            name: "Default".to_string(),
            description: "Default profile".to_string(),
            dpi: DpiSettings {
                x: 800,
                y: 800,
                linked: true,
            },
            polling_rate: 1000,
            brightness: 255,
            remap: RemapSettings::default(),
            macros: Vec::new(),
        }
    }
}

impl Profile {
    /// Create a new profile with the given name
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            ..Default::default()
        }
    }

    /// Create a profile from current device settings
    pub fn from_device_settings(name: impl Into<String>, dpi_x: u16, dpi_y: u16) -> Self {
        Self {
            name: name.into(),
            description: format!("Profile created from device settings"),
            dpi: DpiSettings {
                x: dpi_x,
                y: dpi_y,
                linked: dpi_x == dpi_y,
            },
            polling_rate: 1000,
            brightness: 255,
            remap: RemapSettings::default(),
            macros: Vec::new(),
        }
    }
}

/// Software remapping settings stored in profiles.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct RemapSettings {
    /// Whether remapping should be enabled
    #[serde(default)]
    pub enabled: bool,
    
    /// Whether Windows-style autoscroll is enabled
    #[serde(default)]
    pub autoscroll: bool,

    /// Optional evdev path like /dev/input/eventX
    #[serde(default)]
    pub source_device: Option<String>,

    /// Key/button code mappings (Linux input codes)
    #[serde(default)]
    pub mappings: Vec<RemapMapping>,
    
    /// User-defined macros
    #[serde(default)]
    pub macros: Vec<Macro>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RemapMapping {
    pub source: u16,
        /// Base key/button code
        pub target: u16,
        /// Optional modifiers
        #[serde(default)]
        pub ctrl: bool,
        #[serde(default)]
        pub alt: bool,
        #[serde(default)]
        pub shift: bool,
        #[serde(default)]
        pub meta: bool,
        /// Optional macro ID (if target is a macro instead of a key)
        #[serde(default)]
        pub macro_id: Option<u32>,
}

/// A macro action (single step in a macro)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MacroAction {
    /// Type of action
    pub action_type: MacroActionType,
    /// Key code for key actions
    #[serde(default)]
    pub key_code: Option<u16>,
    /// Delay in milliseconds for delay actions
    #[serde(default)]
    pub delay_ms: Option<u32>,
}

/// Type of macro action
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum MacroActionType {
    /// Press a key (key down)
    KeyPress,
    /// Release a key (key up)
    KeyRelease,
    /// Wait for a duration
    Delay,
    /// Click a mouse button
    MouseClick,
}

/// A complete macro definition
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Macro {
    /// Unique identifier
    pub id: u32,
    /// Human-readable name
    pub name: String,
    /// Sequence of actions
    pub actions: Vec<MacroAction>,
    /// Number of times to repeat (0 = while button held)
    #[serde(default = "default_repeat_count")]
    pub repeat_count: u32,
    /// Delay between repeats in milliseconds
    #[serde(default = "default_repeat_delay")]
    pub repeat_delay_ms: u32,
}

fn default_repeat_count() -> u32 {
    1
}

fn default_repeat_delay() -> u32 {
    50
}

impl Macro {
    /// Create a new empty macro with the given name
    pub fn new(id: u32, name: impl Into<String>) -> Self {
        Self {
            id,
            name: name.into(),
            actions: Vec::new(),
            repeat_count: 1,
            repeat_delay_ms: 50,
        }
    }
    
    /// Add a key press action
    pub fn add_key_press(&mut self, key_code: u16) {
        self.actions.push(MacroAction {
            action_type: MacroActionType::KeyPress,
            key_code: Some(key_code),
            delay_ms: None,
        });
    }
    
    /// Add a key release action
    pub fn add_key_release(&mut self, key_code: u16) {
        self.actions.push(MacroAction {
            action_type: MacroActionType::KeyRelease,
            key_code: Some(key_code),
            delay_ms: None,
        });
    }
    
    /// Add a delay action
    pub fn add_delay(&mut self, delay_ms: u32) {
        self.actions.push(MacroAction {
            action_type: MacroActionType::Delay,
            key_code: None,
            delay_ms: Some(delay_ms),
        });
    }
    
    /// Format as human-readable text for display
    pub fn to_display_text(&self) -> String {
        if self.actions.is_empty() {
            return "No actions".to_string();
        }
        
        self.actions
            .iter()
            .map(|a| match a.action_type {
                MacroActionType::KeyPress => format!("↓ KEY_{}", a.key_code.unwrap_or(0)),
                MacroActionType::KeyRelease => format!("↑ KEY_{}", a.key_code.unwrap_or(0)),
                MacroActionType::Delay => format!("⏱ {}ms", a.delay_ms.unwrap_or(0)),
                MacroActionType::MouseClick => format!("🖱 BTN_{}", a.key_code.unwrap_or(0)),
            })
            .collect::<Vec<_>>()
            .join("\n")
    }
}

/// Profile manager for saving/loading profiles
pub struct ProfileManager {
    /// Directory where profiles are stored
    profile_dir: PathBuf,
}

impl ProfileManager {
    /// Create a new profile manager
    pub fn new() -> Result<Self> {
        let profile_dir = Self::get_profile_directory()?;

        // Create directory if it doesn't exist
        if !profile_dir.exists() {
            fs::create_dir_all(&profile_dir).context("Failed to create profile directory")?;
            info!("Created profile directory: {:?}", profile_dir);
        }

        Ok(Self { profile_dir })
    }

    /// Get the profile directory path
    fn get_profile_directory() -> Result<PathBuf> {
        let config_dir = dirs::config_dir().context("Failed to find config directory")?;
        Ok(config_dir.join("razerlinux").join("profiles"))
    }

    /// Save a profile to disk
    pub fn save_profile(&self, profile: &Profile) -> Result<PathBuf> {
        let filename = Self::sanitize_filename(&profile.name);
        let path = self.profile_dir.join(format!("{}.toml", filename));

        let toml_content =
            toml::to_string_pretty(profile).context("Failed to serialize profile")?;

        fs::write(&path, toml_content).context("Failed to write profile file")?;

        info!("Saved profile '{}' to {:?}", profile.name, path);
        Ok(path)
    }

    /// Load a profile from disk
    pub fn load_profile(&self, name: &str) -> Result<Profile> {
        let filename = Self::sanitize_filename(name);
        let path = self.profile_dir.join(format!("{}.toml", filename));

        let content = fs::read_to_string(&path)
            .context(format!("Failed to read profile file: {:?}", path))?;

        let profile: Profile = toml::from_str(&content).context("Failed to parse profile")?;

        info!("Loaded profile '{}' from {:?}", profile.name, path);
        Ok(profile)
    }

    /// List all available profiles
    pub fn list_profiles(&self) -> Result<Vec<String>> {
        let mut profiles = Vec::new();

        if let Ok(entries) = fs::read_dir(&self.profile_dir) {
            for entry in entries.flatten() {
                let path = entry.path();
                if path.extension().map_or(false, |ext| ext == "toml") {
                    if let Some(name) = path.file_stem() {
                        profiles.push(name.to_string_lossy().to_string());
                    }
                }
            }
        }

        profiles.sort();
        Ok(profiles)
    }

    /// Delete a profile
    pub fn delete_profile(&self, name: &str) -> Result<()> {
        let filename = Self::sanitize_filename(name);
        let path = self.profile_dir.join(format!("{}.toml", filename));

        fs::remove_file(&path).context(format!("Failed to delete profile: {:?}", path))?;

        info!("Deleted profile '{}'", name);
        Ok(())
    }

    /// Sanitize a profile name for use as a filename
    fn sanitize_filename(name: &str) -> String {
        name.chars()
            .map(|c| {
                if c.is_alphanumeric() || c == '-' || c == '_' {
                    c
                } else {
                    '_'
                }
            })
            .collect()
    }
}

impl Default for ProfileManager {
    fn default() -> Self {
        Self::new().expect("Failed to create profile manager")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_profile_serialization() {
        let profile = Profile::new("Test Profile");
        let toml = toml::to_string_pretty(&profile).unwrap();

        assert!(toml.contains("name = \"Test Profile\""));
        assert!(toml.contains("[dpi]"));
    }

    #[test]
    fn test_profile_deserialization() {
        let toml = r#"
name = "Gaming"
description = "High DPI gaming profile"

[dpi]
x = 1600
y = 1600
linked = true

polling_rate = 1000
brightness = 255
"#;

        let profile: Profile = toml::from_str(toml).unwrap();
        assert_eq!(profile.name, "Gaming");
        assert_eq!(profile.dpi.x, 1600);
    }

    #[test]
    fn test_sanitize_filename() {
        assert_eq!(
            ProfileManager::sanitize_filename("Test Profile"),
            "Test_Profile"
        );
        assert_eq!(
            ProfileManager::sanitize_filename("my/profile"),
            "my_profile"
        );
    }
}
