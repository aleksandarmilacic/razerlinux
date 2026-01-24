//! Null/Fallback Display Backend
//!
//! Used when no display server is available or detection fails.
//! All operations are no-ops.

use super::{OverlayCommand, OverlayDisplay, ScrollDetector};
use std::sync::mpsc::{self, Sender};

/// Null scroll detector - always returns false (no autoscroll)
pub struct NullScrollDetector;

impl ScrollDetector for NullScrollDetector {
    fn should_autoscroll(&self) -> bool {
        false
    }
    
    fn cursor_position(&self) -> Option<(i32, i32)> {
        None
    }
    
    fn clear_cache(&self) {
        // No-op
    }
}

/// Null overlay - no visible indicator
pub struct NullOverlay {
    sender: Sender<OverlayCommand>,
}

impl NullOverlay {
    pub fn new() -> Self {
        let (tx, _rx) = mpsc::channel();
        Self { sender: tx }
    }
}

impl OverlayDisplay for NullOverlay {
    fn sender(&self) -> Sender<OverlayCommand> {
        self.sender.clone()
    }
    
    fn show(&self) {
        // No-op
    }
    
    fn hide(&self) {
        // No-op
    }
    
    fn shutdown(&mut self) {
        // No-op
    }
}

impl Default for NullOverlay {
    fn default() -> Self {
        Self::new()
    }
}
