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
        }
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
            fs::create_dir_all(&profile_dir)
                .context("Failed to create profile directory")?;
            info!("Created profile directory: {:?}", profile_dir);
        }
        
        Ok(Self { profile_dir })
    }
    
    /// Get the profile directory path
    fn get_profile_directory() -> Result<PathBuf> {
        let config_dir = dirs::config_dir()
            .context("Failed to find config directory")?;
        Ok(config_dir.join("razerlinux").join("profiles"))
    }
    
    /// Save a profile to disk
    pub fn save_profile(&self, profile: &Profile) -> Result<PathBuf> {
        let filename = Self::sanitize_filename(&profile.name);
        let path = self.profile_dir.join(format!("{}.toml", filename));
        
        let toml_content = toml::to_string_pretty(profile)
            .context("Failed to serialize profile")?;
        
        fs::write(&path, toml_content)
            .context("Failed to write profile file")?;
        
        info!("Saved profile '{}' to {:?}", profile.name, path);
        Ok(path)
    }
    
    /// Load a profile from disk
    pub fn load_profile(&self, name: &str) -> Result<Profile> {
        let filename = Self::sanitize_filename(name);
        let path = self.profile_dir.join(format!("{}.toml", filename));
        
        let content = fs::read_to_string(&path)
            .context(format!("Failed to read profile file: {:?}", path))?;
        
        let profile: Profile = toml::from_str(&content)
            .context("Failed to parse profile")?;
        
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
        
        fs::remove_file(&path)
            .context(format!("Failed to delete profile: {:?}", path))?;
        
        info!("Deleted profile '{}'", name);
        Ok(())
    }
    
    /// Sanitize a profile name for use as a filename
    fn sanitize_filename(name: &str) -> String {
        name.chars()
            .map(|c| if c.is_alphanumeric() || c == '-' || c == '_' { c } else { '_' })
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
        assert_eq!(ProfileManager::sanitize_filename("Test Profile"), "Test_Profile");
        assert_eq!(ProfileManager::sanitize_filename("my/profile"), "my_profile");
    }
}
