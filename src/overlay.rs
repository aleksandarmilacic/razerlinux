//! Autoscroll Visual Overlay
//!
//! This module provides the autoscroll indicator overlay.
//! It delegates to the display_backend module for cross-platform support
//! (X11 and Wayland).
//!
//! Windows autoscroll icon design:
//! - Small circle in the center (origin point marker)
//! - Four triangular arrows pointing up, down, left, right
//! - Semi-transparent dark background
//! - Clean, minimal design matching Windows style

use crate::display_backend::{self, DisplayBackend, OverlayDisplay};
use anyhow::Result;
use std::sync::mpsc::Sender;
use tracing::info;

// Re-export OverlayCommand from display_backend
pub use crate::display_backend::OverlayCommand;

/// Handle to control the autoscroll overlay
/// 
/// This is a wrapper around the display-backend-specific overlay implementation.
pub struct AutoscrollOverlay {
    inner: Box<dyn OverlayDisplay>,
}

impl AutoscrollOverlay {
    /// Start the overlay system
    /// 
    /// Automatically detects the display server (X11 or Wayland) and
    /// creates the appropriate overlay implementation.
    pub fn start() -> Result<Self> {
        let backend = DisplayBackend::new();
        info!("Creating overlay for {} session", backend.display_server().name());
        
        match backend.create_overlay() {
            Some(inner) => Ok(Self { inner }),
            None => {
                // Fall back to null overlay if no display-specific one is available
                Ok(Self {
                    inner: Box::new(display_backend::null::NullOverlay::new()),
                })
            }
        }
    }
    
    /// Get a sender to send commands to the overlay
    pub fn sender(&self) -> Sender<OverlayCommand> {
        self.inner.sender()
    }
    
    /// Show the overlay at current cursor position
    pub fn show(&self) {
        self.inner.show();
    }
    
    /// Hide the overlay
    pub fn hide(&self) {
        self.inner.hide();
    }
    
    /// Shutdown the overlay
    pub fn shutdown(&mut self) {
        self.inner.shutdown();
    }
}
