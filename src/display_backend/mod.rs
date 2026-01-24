//! Display Backend Abstraction
//!
//! Provides a unified interface for display server operations that differ
//! between X11 and Wayland, enabling cross-platform support.
//!
//! Components:
//! - `ScrollDetector`: Determines if cursor is over a scrollable area
//! - `OverlayDisplay`: Shows/hides the autoscroll indicator
//! - `CursorTracker`: Gets current cursor position

#[cfg(feature = "x11")]
pub mod x11;

#[cfg(feature = "wayland")]
pub mod wayland;

#[cfg(feature = "atspi")]
pub mod atspi;

pub mod heuristic;
pub mod null;

// Remove unused: use anyhow::Result;
use std::sync::mpsc::Sender;

// Re-export common types
// HeuristicScrollDetector re-exported for potential external use\n#[allow(unused_imports)]\npub use heuristic::HeuristicScrollDetector;

/// Detected display server type
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DisplayServer {
    X11,
    Wayland,
    Unknown,
}

impl DisplayServer {
    /// Detect the current display server from environment variables
    pub fn detect() -> Self {
        // Check XDG_SESSION_TYPE first (most reliable on modern systems)
        if let Ok(session_type) = std::env::var("XDG_SESSION_TYPE") {
            match session_type.to_lowercase().as_str() {
                "wayland" => return DisplayServer::Wayland,
                "x11" => return DisplayServer::X11,
                _ => {}
            }
        }

        // Fallback: check WAYLAND_DISPLAY (indicates Wayland compositor)
        if std::env::var("WAYLAND_DISPLAY").is_ok() {
            return DisplayServer::Wayland;
        }

        // Fallback: check DISPLAY (indicates X11)
        if std::env::var("DISPLAY").is_ok() {
            return DisplayServer::X11;
        }

        DisplayServer::Unknown
    }
    
    /// Get a human-readable name for the display server
    pub fn name(&self) -> &'static str {
        match self {
            DisplayServer::X11 => "X11",
            DisplayServer::Wayland => "Wayland",
            DisplayServer::Unknown => "Unknown",
        }
    }
}

/// Commands for controlling the overlay display
#[derive(Debug, Clone)]
pub enum OverlayCommand {
    /// Show overlay at specified cursor position (x, y in screen coordinates)
    Show(i32, i32),
    /// Hide overlay
    Hide,
    /// Update scroll direction (dx, dy normalized -1 to 1)
    UpdateDirection(f32, f32),
    /// Shutdown the overlay thread
    Shutdown,
}

/// Trait for scroll area detection
/// 
/// Implementations determine if the cursor is over a scrollable area,
/// which is used for Windows-like autoscroll behavior (only activate
/// in scrollable regions).
pub trait ScrollDetector: Send + Sync {
    /// Check if autoscroll should activate at the current cursor position
    /// 
    /// Returns `true` if the cursor is over a scrollable area (browser content,
    /// text editor, terminal, etc.), `false` if over non-scrollable UI
    /// (desktop, panels, menus, buttons).
    fn should_autoscroll(&self) -> bool;
    
    /// Get current cursor position in screen coordinates
    /// 
    /// Returns `None` if cursor position cannot be determined
    fn cursor_position(&self) -> Option<(i32, i32)>;
    
    /// Clear any internal caches (e.g., when focus changes)
    fn clear_cache(&self);
}

/// Trait for overlay display control
///
/// Implementations show/hide the autoscroll indicator at the cursor position.
pub trait OverlayDisplay: Send {
    /// Get a sender to send commands to the overlay
    fn sender(&self) -> Sender<OverlayCommand>;
    
    /// Show the overlay at current cursor position
    fn show(&self);
    
    /// Hide the overlay
    fn hide(&self);
    
    /// Shutdown the overlay system
    fn shutdown(&mut self);
}

/// Factory for creating display backend components
pub struct DisplayBackend {
    display_server: DisplayServer,
}

impl DisplayBackend {
    /// Create a new display backend with auto-detection
    pub fn new() -> Self {
        let display_server = DisplayServer::detect();
        tracing::info!("Detected display server: {}", display_server.name());
        Self { display_server }
    }
    
    /// Create a backend for a specific display server
    pub fn for_server(display_server: DisplayServer) -> Self {
        Self { display_server }
    }
    
    /// Get the detected display server type
    pub fn display_server(&self) -> DisplayServer {
        self.display_server
    }
    
    /// Create a scroll detector for the current display server
    /// 
    /// Returns `None` if scroll detection is not available for this backend.
    /// 
    /// Note: On Wayland with XWayland, we prefer X11 detection for better
    /// compatibility since it can query window properties more reliably.
    pub fn create_scroll_detector(&self) -> Option<Box<dyn ScrollDetector>> {
        match self.display_server {
            #[cfg(feature = "x11")]
            DisplayServer::X11 => {
                match x11::X11ScrollDetector::new() {
                    Ok(detector) => Some(Box::new(detector)),
                    Err(e) => {
                        tracing::warn!("Failed to create X11 scroll detector: {}", e);
                        None
                    }
                }
            }
            #[cfg(feature = "wayland")]
            DisplayServer::Wayland => {
                // On Wayland, prefer X11 scroll detection via XWayland if available
                // for better window property querying
                #[cfg(feature = "x11")]
                if std::env::var("DISPLAY").is_ok() {
                    tracing::info!("Using X11 scroll detector via XWayland");
                    match x11::X11ScrollDetector::new() {
                        Ok(detector) => return Some(Box::new(detector)),
                        Err(e) => {
                            tracing::debug!("XWayland scroll detector unavailable: {}", e);
                        }
                    }
                }
                
                // Fall back to Wayland/AT-SPI scroll detector
                match wayland::WaylandScrollDetector::new() {
                    Ok(detector) => Some(Box::new(detector)),
                    Err(e) => {
                        tracing::warn!("Failed to create Wayland scroll detector: {}", e);
                        // Try AT-SPI fallback
                        #[cfg(feature = "atspi")]
                        {
                            match wayland::AtSpiScrollDetector::new() {
                                Ok(detector) => return Some(Box::new(detector)),
                                Err(e) => tracing::warn!("AT-SPI fallback failed: {}", e),
                            }
                        }
                        None
                    }
                }
            }
            _ => {
                tracing::warn!("No scroll detector available for {:?}", self.display_server);
                None
            }
        }
    }
    
    /// Create an overlay display for the current display server
    /// 
    /// Returns `None` if overlay display is not available for this backend.
    /// 
    /// Note: On Wayland, we prefer X11 overlay via XWayland because layer-shell
    /// cannot position overlays at arbitrary cursor positions. Layer-shell is
    /// designed for screen-edge anchored surfaces (panels, docks, etc.).
    pub fn create_overlay(&self) -> Option<Box<dyn OverlayDisplay>> {
        match self.display_server {
            #[cfg(feature = "x11")]
            DisplayServer::X11 => {
                match x11::X11Overlay::start() {
                    Ok(overlay) => Some(Box::new(overlay)),
                    Err(e) => {
                        tracing::warn!("Failed to create X11 overlay: {}", e);
                        None
                    }
                }
            }
            #[cfg(feature = "wayland")]
            DisplayServer::Wayland => {
                // On Wayland, prefer X11 overlay via XWayland if DISPLAY is set
                // because layer-shell cannot position at cursor location
                #[cfg(feature = "x11")]
                if std::env::var("DISPLAY").is_ok() {
                    tracing::info!("Using X11 overlay via XWayland for cursor-positioned indicator");
                    match x11::X11Overlay::start() {
                        Ok(overlay) => return Some(Box::new(overlay)),
                        Err(e) => {
                            tracing::debug!("XWayland overlay unavailable: {}", e);
                        }
                    }
                }
                
                // Fall back to Wayland layer-shell overlay (limited positioning)
                match wayland::WaylandOverlay::start() {
                    Ok(overlay) => Some(Box::new(overlay)),
                    Err(e) => {
                        tracing::warn!("Failed to create Wayland overlay: {}", e);
                        None
                    }
                }
            }
            _ => {
                tracing::warn!("No overlay available for {:?}", self.display_server);
                None
            }
        }
    }
}

impl Default for DisplayBackend {
    fn default() -> Self {
        Self::new()
    }
}
