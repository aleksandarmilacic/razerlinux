//! System tray icon for RazerLinux
//!
//! Provides a system tray icon with menu for quick access to features.

use anyhow::{Result, anyhow};
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::OnceLock;
use tracing::info;

/// Commands that can be sent from tray menu
#[derive(Debug, Clone)]
pub enum TrayCommand {
    ShowWindow,
    Quit,
}

// Global channel for tray commands
static TRAY_CHANNEL: OnceLock<(Sender<TrayCommand>, std::sync::Mutex<Receiver<TrayCommand>>)> = OnceLock::new();

fn get_or_init_channel() -> &'static (Sender<TrayCommand>, std::sync::Mutex<Receiver<TrayCommand>>) {
    TRAY_CHANNEL.get_or_init(|| {
        let (tx, rx) = mpsc::channel();
        (tx, std::sync::Mutex::new(rx))
    })
}

/// Try to receive a tray command (non-blocking, can be called from anywhere)
pub fn try_recv_command() -> Option<TrayCommand> {
    let (_, rx) = get_or_init_channel();
    rx.lock().ok()?.try_recv().ok()
}

/// System tray icon handler
pub struct TrayIcon {
    _tray: tray_icon::TrayIcon,
}

impl TrayIcon {
    /// Create and show the system tray icon
    pub fn new() -> Result<Self> {
        use tray_icon::TrayIconBuilder;
        
        // On Linux, tray-icon uses GTK for menus which requires initialization
        // We'll create a tray icon without a menu to avoid GTK dependency issues
        
        // Initialize the channel (for future use)
        let _ = get_or_init_channel();
        
        // Create icon (simple green circle)
        let icon = create_default_icon()?;
        
        // Build tray icon without menu (to avoid GTK issues)
        let tray = TrayIconBuilder::new()
            .with_tooltip("RazerLinux - Mouse Configurator")
            .with_icon(icon)
            .build()
            .map_err(|e| anyhow!("Failed to create tray icon: {}", e))?;
        
        info!("System tray icon created");
        
        Ok(Self {
            _tray: tray,
        })
    }
}

/// Create a simple default icon (green circle)
fn create_default_icon() -> Result<tray_icon::Icon> {
    // Create a simple 32x32 RGBA icon
    let size = 32;
    let mut rgba = vec![0u8; size * size * 4];
    
    // Draw a green circle
    let center = size as f32 / 2.0;
    let radius = center - 2.0;
    
    for y in 0..size {
        for x in 0..size {
            let dx = x as f32 - center;
            let dy = y as f32 - center;
            let dist = (dx * dx + dy * dy).sqrt();
            
            let idx = (y * size + x) * 4;
            
            if dist <= radius {
                // Green fill
                rgba[idx] = 34;      // R
                rgba[idx + 1] = 197; // G
                rgba[idx + 2] = 94;  // B
                rgba[idx + 3] = 255; // A
            } else if dist <= radius + 1.0 {
                // Anti-aliased edge
                let alpha = ((radius + 1.0 - dist) * 255.0) as u8;
                rgba[idx] = 34;
                rgba[idx + 1] = 197;
                rgba[idx + 2] = 94;
                rgba[idx + 3] = alpha;
            }
        }
    }
    
    let icon = tray_icon::Icon::from_rgba(rgba, size as u32, size as u32)
        .map_err(|e| anyhow!("Failed to create icon: {}", e))?;
    Ok(icon)
}
