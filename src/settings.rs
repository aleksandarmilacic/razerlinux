//! Application settings management
//!
//! Handles autostart configuration and default profile settings.

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;
use tracing::{info, warn};

/// Application settings
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppSettings {
    /// Whether to start on system startup
    #[serde(default)]
    pub autostart: bool,
    
    /// Default profile to load on startup (defaults to "Default")
    #[serde(default = "default_profile_name")]
    pub default_profile: String,
    
    /// Minimize to tray on close (future feature)
    #[serde(default)]
    pub minimize_to_tray: bool,
    
    /// Show DPI change notifications (future feature)
    #[serde(default)]
    pub show_dpi_notifications: bool,
}

fn default_profile_name() -> String {
    "Default".to_string()
}

impl Default for AppSettings {
    fn default() -> Self {
        Self {
            autostart: false,
            default_profile: "Default".to_string(),
            minimize_to_tray: false,
            show_dpi_notifications: false,
        }
    }
}

impl AppSettings {
    /// Get the settings file path
    fn settings_path() -> Result<PathBuf> {
        let config_dir = dirs::config_dir()
            .context("Could not find config directory")?
            .join("razerlinux");
        
        fs::create_dir_all(&config_dir)?;
        Ok(config_dir.join("settings.toml"))
    }
    
    /// Load settings from file (or create defaults)
    pub fn load() -> Result<Self> {
        let path = Self::settings_path()?;
        
        if path.exists() {
            let content = fs::read_to_string(&path)?;
            let settings: AppSettings = toml::from_str(&content)?;
            info!("Loaded settings from {:?}", path);
            Ok(settings)
        } else {
            info!("No settings file found, using defaults");
            Ok(Self::default())
        }
    }
    
    /// Save settings to file
    pub fn save(&self) -> Result<()> {
        let path = Self::settings_path()?;
        let content = toml::to_string_pretty(self)?;
        fs::write(&path, content)?;
        info!("Saved settings to {:?}", path);
        Ok(())
    }
    
    /// Enable or disable autostart
    pub fn set_autostart(&mut self, enabled: bool) -> Result<()> {
        self.autostart = enabled;
        
        if enabled {
            create_autostart_entry()?;
            info!("Autostart enabled");
        } else {
            remove_autostart_entry()?;
            info!("Autostart disabled");
        }
        
        self.save()
    }
    
    /// Set the default profile
    pub fn set_default_profile(&mut self, profile: &str) -> Result<()> {
        self.default_profile = profile.to_string();
        info!("Default profile set to: '{}'", profile);
        self.save()
    }
    
    /// Set minimize to tray on close
    pub fn set_minimize_to_tray(&mut self, enabled: bool) -> Result<()> {
        self.minimize_to_tray = enabled;
        info!("Minimize to tray on close: {}", enabled);
        self.save()
    }
}

/// Get the autostart desktop file path
fn autostart_path() -> Result<PathBuf> {
    let autostart_dir = dirs::config_dir()
        .context("Could not find config directory")?
        .join("autostart");
    
    fs::create_dir_all(&autostart_dir)?;
    Ok(autostart_dir.join("razerlinux.desktop"))
}

/// Create the autostart desktop entry
fn create_autostart_entry() -> Result<()> {
    let path = autostart_path()?;
    
    // Find the executable path
    let exe_path = std::env::current_exe()
        .unwrap_or_else(|_| PathBuf::from("/usr/bin/razerlinux"));
    
    let desktop_entry = format!(
r#"[Desktop Entry]
Type=Application
Name=RazerLinux
Comment=Razer Mouse Configuration Tool
Exec={}
Icon=input-mouse
Terminal=false
Categories=Settings;HardwareSettings;
StartupNotify=false
X-GNOME-Autostart-enabled=true
"#, 
        exe_path.display()
    );
    
    fs::write(&path, desktop_entry)?;
    info!("Created autostart entry at {:?}", path);
    Ok(())
}

/// Remove the autostart desktop entry
fn remove_autostart_entry() -> Result<()> {
    let path = autostart_path()?;
    
    if path.exists() {
        fs::remove_file(&path)?;
        info!("Removed autostart entry");
    }
    
    Ok(())
}

/// Check if autostart is currently enabled
pub fn is_autostart_enabled() -> bool {
    autostart_path()
        .map(|p| p.exists())
        .unwrap_or(false)
}

// ============ Systemd User Service Control ============

/// Get the real user (not root when running via pkexec/sudo)
fn get_real_user() -> Option<String> {
    // Check SUDO_USER first (set by sudo/pkexec)
    std::env::var("SUDO_USER").ok()
        .or_else(|| std::env::var("PKEXEC_UID").ok().and_then(|uid| {
            // Convert UID to username
            std::process::Command::new("id")
                .args(["-nu", &uid])
                .output()
                .ok()
                .and_then(|o| String::from_utf8(o.stdout).ok())
                .map(|s| s.trim().to_string())
        }))
        .or_else(|| std::env::var("USER").ok())
}

/// Run systemctl --user as the real user (works even when running as root via pkexec)
fn run_systemctl_user(args: &[&str]) -> std::io::Result<std::process::Output> {
    let real_user = get_real_user();
    
    // If we're running as root but have a real user, use sudo -u
    if std::env::var("SUDO_USER").is_ok() || std::env::var("PKEXEC_UID").is_ok() {
        if let Some(ref user) = real_user {
            // Get the user's UID for XDG_RUNTIME_DIR
            let uid = std::process::Command::new("id")
                .args(["-u", user])
                .output()
                .ok()
                .and_then(|o| String::from_utf8(o.stdout).ok())
                .map(|s| s.trim().to_string())
                .unwrap_or_else(|| "1000".to_string());
            
            let runtime_dir_env = format!("XDG_RUNTIME_DIR=/run/user/{}", uid);
            
            // Build command: sudo -u USER -- env XDG_RUNTIME_DIR=... systemctl --user <args>
            let mut full_args: Vec<String> = vec![
                "-u".to_string(),
                user.clone(),
                "--".to_string(),
                "env".to_string(),
                runtime_dir_env,
                "systemctl".to_string(),
                "--user".to_string(),
            ];
            full_args.extend(args.iter().map(|s| s.to_string()));
            
            return std::process::Command::new("sudo")
                .args(&full_args)
                .output();
        }
    }
    
    // Running as normal user, use systemctl directly
    let mut cmd_args = vec!["--user"];
    cmd_args.extend(args.iter().copied());
    std::process::Command::new("systemctl")
        .args(&cmd_args)
        .output()
}

/// Check if systemd user service is enabled
pub fn is_systemd_enabled() -> bool {
    run_systemctl_user(&["is-enabled", "razerlinux.service"])
        .map(|output| output.status.success())
        .unwrap_or(false)
}

/// Check if systemd user service is available (installed)
pub fn is_systemd_available() -> bool {
    std::path::Path::new("/usr/lib/systemd/user/razerlinux.service").exists()
        || std::path::Path::new("/etc/systemd/user/razerlinux.service").exists()
}

/// Enable the systemd user service
pub fn enable_systemd_service() -> Result<()> {
    if !is_systemd_available() {
        anyhow::bail!("Systemd service not installed. Reinstall with: sudo ./install.sh");
    }
    
    // Reload daemon to pick up any changes
    let _ = run_systemctl_user(&["daemon-reload"]);
    
    // Enable the service
    let output = run_systemctl_user(&["enable", "razerlinux.service"])
        .context("Failed to run systemctl")?;
    
    if output.status.success() {
        info!("Systemd user service enabled");
        Ok(())
    } else {
        let stderr = String::from_utf8_lossy(&output.stderr);
        anyhow::bail!("Failed to enable systemd service: {}", stderr)
    }
}

/// Disable the systemd user service
pub fn disable_systemd_service() -> Result<()> {
    let output = run_systemctl_user(&["disable", "razerlinux.service"])
        .context("Failed to run systemctl")?;
    
    if output.status.success() {
        info!("Systemd user service disabled");
        Ok(())
    } else {
        let stderr = String::from_utf8_lossy(&output.stderr);
        anyhow::bail!("Failed to disable systemd service: {}", stderr)
    }
}

/// Get list of available profile names
pub fn get_profile_list() -> Result<Vec<String>> {
    let profile_dir = dirs::config_dir()
        .context("Could not find config directory")?
        .join("razerlinux")
        .join("profiles");
    
    if !profile_dir.exists() {
        return Ok(Vec::new());
    }
    
    let mut profiles = Vec::new();
    
    for entry in fs::read_dir(&profile_dir)? {
        let entry = entry?;
        let path = entry.path();
        
        if path.extension().map_or(false, |ext| ext == "toml") {
            if let Some(name) = path.file_stem() {
                profiles.push(name.to_string_lossy().to_string());
            }
        }
    }
    
    profiles.sort();
    Ok(profiles)
}

/// Ensure the Default profile exists, create if not
pub fn ensure_default_profile_exists() -> Result<()> {
    use crate::profile::{Profile, ProfileManager};
    
    let manager = ProfileManager::new()?;
    
    // Check if Default profile exists
    if manager.load_profile("Default").is_err() {
        // Create a default profile
        let default_profile = Profile::default();
        manager.save_profile(&default_profile)?;
        info!("Created Default profile");
    }
    
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_settings_default() {
        let settings = AppSettings::default();
        assert!(!settings.autostart);
        assert_eq!(settings.default_profile, "Default");
    }
    
    #[test]
    fn test_settings_serialization() {
        let mut settings = AppSettings::default();
        settings.autostart = true;
        settings.default_profile = "gaming".to_string();
        
        let toml = toml::to_string(&settings).unwrap();
        assert!(toml.contains("autostart = true"));
        assert!(toml.contains("default_profile = \"gaming\""));
    }
    
    #[test]
    fn test_settings_deserialization() {
        let toml = r#"
autostart = true
default_profile = "work"
"#;
        let settings: AppSettings = toml::from_str(toml).unwrap();
        assert!(settings.autostart);
        assert_eq!(settings.default_profile, "work");
    }
}
