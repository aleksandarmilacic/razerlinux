//! System tray icon for RazerLinux
//!
//! Provides a system tray icon with menu for quick access to features.

use anyhow::Result;
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

/// System tray icon handler (Linux uses StatusNotifier for KDE)
#[cfg(target_os = "linux")]
pub struct TrayIcon {
    _handle: ksni::Handle<LinuxTray>,
}

#[cfg(target_os = "linux")]
struct LinuxTray {
    sender: Sender<TrayCommand>,
}

#[cfg(target_os = "linux")]
impl ksni::Tray for LinuxTray {
    fn title(&self) -> String {
        // Add (Debug) suffix for debug builds
        #[cfg(debug_assertions)]
        { "RazerLinux (Debug)".to_string() }
        #[cfg(not(debug_assertions))]
        { "RazerLinux".to_string() }
    }

    fn icon_name(&self) -> String {
        // Use different icon for debug builds to differentiate from release
        #[cfg(debug_assertions)]
        { "applications-development".to_string() }
        #[cfg(not(debug_assertions))]
        { "input-mouse".to_string() }
    }

    fn id(&self) -> String {
        "razerlinux".to_string()
    }

    fn menu(&self) -> Vec<ksni::menu::MenuItem<Self>> {
        use ksni::menu::{MenuItem, StandardItem};

        vec![
            MenuItem::Standard(StandardItem {
                label: "Show RazerLinux".to_string(),
                activate: Box::new(|this| {
                    let _ = this.sender.send(TrayCommand::ShowWindow);
                }),
                ..Default::default()
            }),
            MenuItem::Separator,
            MenuItem::Standard(StandardItem {
                label: "Quit".to_string(),
                activate: Box::new(|this| {
                    let _ = this.sender.send(TrayCommand::Quit);
                }),
                ..Default::default()
            }),
        ]
    }
}

#[cfg(target_os = "linux")]
impl TrayIcon {
    /// Create and show the system tray icon
    pub fn new() -> Result<Self> {
        let (sender, _) = get_or_init_channel();
        let tray = LinuxTray { sender: sender.clone() };
        let service = ksni::TrayService::new(tray);
        let handle = service.handle();
        service.spawn();

        info!("System tray icon created (StatusNotifier)");

        Ok(Self { _handle: handle })
    }
}

/// Non-Linux fallback using tray-icon
#[cfg(not(target_os = "linux"))]
pub struct TrayIcon {
    _tray: tray_icon::TrayIcon,
}

#[cfg(not(target_os = "linux"))]
impl TrayIcon {
    pub fn new() -> Result<Self> {
        use tray_icon::TrayIconBuilder;

        let _ = get_or_init_channel();
        let icon = create_default_icon()?;
        let tray = TrayIconBuilder::new()
            .with_tooltip("RazerLinux - Mouse Configurator")
            .with_icon(icon)
            .build()
            .map_err(|e| anyhow!("Failed to create tray icon: {}", e))?;

        info!("System tray icon created");

        Ok(Self { _tray: tray })
    }
}

#[cfg(not(target_os = "linux"))]
fn create_default_icon() -> Result<tray_icon::Icon> {
    let size = 32;
    let mut rgba = vec![0u8; size * size * 4];

    let center = size as f32 / 2.0;
    let radius = center - 2.0;

    for y in 0..size {
        for x in 0..size {
            let dx = x as f32 - center;
            let dy = y as f32 - center;
            let dist = (dx * dx + dy * dy).sqrt();

            let idx = (y * size + x) * 4;

            if dist <= radius {
                rgba[idx] = 34;
                rgba[idx + 1] = 197;
                rgba[idx + 2] = 94;
                rgba[idx + 3] = 255;
            } else if dist <= radius + 1.0 {
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
