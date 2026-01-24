//! AT-SPI Scroll Detection
//!
//! Uses the Assistive Technology Service Provider Interface (AT-SPI)
//! for accurate scroll detection on both X11 and Wayland.
//!
//! Note: Full AT-SPI integration requires async D-Bus queries. For now,
//! this module provides a simplified heuristic-based detector that can
//! be extended with full AT-SPI support in the future.

use super::heuristic::HeuristicScrollDetector;
use super::ScrollDetector;
use anyhow::{Context, Result};
use std::sync::RwLock;
use std::collections::HashMap;
use std::time::{Duration, Instant};
use tracing::{debug, info};

#[cfg(feature = "x11")]
use x11rb::connection::Connection as X11Connection;
#[cfg(feature = "x11")]
use x11rb::protocol::xproto::ConnectionExt;

/// AT-SPI roles that indicate scrollable content (for future use)
#[allow(dead_code)]
const SCROLLABLE_ROLES: &[&str] = &[
    "scroll pane",
    "viewport",
    "document web",
    "document text",
    "document frame",
    "terminal",
    "text",
    "list",
    "table",
    "tree",
    "tree table",
    "scroll bar",
];

/// AT-SPI roles that indicate non-scrollable UI elements (for future use)
#[allow(dead_code)]
const DENY_ROLES: &[&str] = &[
    "menu bar",
    "menu",
    "menu item",
    "tool bar",
    "push button",
    "toggle button",
    "status bar",
    "panel",
    "desktop pane",
    "desktop frame",
    "dock",
    "popup menu",
    "combo box",
    "tool tip",
    "notification",
    "dialog",
];

/// Cache entry for detection results
struct CacheEntry {
    scrollable: bool,
    timestamp: Instant,
}

/// AT-SPI-based scroll detector
///
/// Currently uses heuristics + X11 cursor position (when available).
/// Full AT-SPI tree walking can be added in the future.
pub struct AtSpiScrollDetector {
    /// Fallback heuristic detector
    heuristic: HeuristicScrollDetector,
    /// Decision cache
    cache: RwLock<HashMap<(i32, i32), CacheEntry>>,
    /// Cache TTL
    cache_ttl: Duration,
    /// Strict default: unknown = not scrollable
    strict_default: bool,
    /// X11 connection for cursor position (if available)
    #[cfg(feature = "x11")]
    x11_conn: Option<(x11rb::rust_connection::RustConnection, u32)>,
}

impl AtSpiScrollDetector {
    /// Create a new AT-SPI scroll detector
    pub fn new() -> Result<Self> {
        // Verify D-Bus accessibility is available
        // For now, just check if we can create a D-Bus connection
        let _session = zbus::blocking::Connection::session()
            .context("D-Bus session bus not available")?;

        // Try to set up X11 connection for cursor position
        #[cfg(feature = "x11")]
        let x11_conn = if std::env::var("DISPLAY").is_ok() {
            match x11rb::connect(None) {
                Ok((conn, screen_num)) => {
                    let root = conn.setup().roots[screen_num].root;
                    Some((conn, root))
                }
                Err(_) => None,
            }
        } else {
            None
        };

        info!("AT-SPI scroll detector initialized (using heuristic fallback)");

        Ok(Self {
            heuristic: HeuristicScrollDetector::new(),
            cache: RwLock::new(HashMap::new()),
            cache_ttl: Duration::from_millis(100),
            strict_default: true,
            #[cfg(feature = "x11")]
            x11_conn,
        })
    }

    /// Check if a role indicates scrollable content
    #[allow(dead_code)]
    fn is_scrollable_role(role: &str) -> bool {
        let role_lower = role.to_lowercase();
        SCROLLABLE_ROLES.iter().any(|r| role_lower.contains(r))
    }

    /// Check if a role indicates non-scrollable UI
    #[allow(dead_code)]
    fn is_deny_role(role: &str) -> bool {
        let role_lower = role.to_lowercase();
        DENY_ROLES.iter().any(|r| role_lower.contains(r))
    }

    /// Cache a detection result
    fn cache_result(&self, x: i32, y: i32, scrollable: bool) {
        if let Ok(mut cache) = self.cache.write() {
            if cache.len() > 50 {
                let now = Instant::now();
                cache.retain(|_, v| now.duration_since(v.timestamp) < self.cache_ttl * 2);
            }

            let key = (x >> 4, y >> 4);
            cache.insert(
                key,
                CacheEntry {
                    scrollable,
                    timestamp: Instant::now(),
                },
            );
        }
    }

    /// Check cache for existing result
    fn check_cache(&self, x: i32, y: i32) -> Option<bool> {
        let cache_key = (x >> 4, y >> 4);
        if let Ok(cache) = self.cache.read() {
            if let Some(entry) = cache.get(&cache_key) {
                if entry.timestamp.elapsed() < self.cache_ttl {
                    return Some(entry.scrollable);
                }
            }
        }
        None
    }
}

impl ScrollDetector for AtSpiScrollDetector {
    fn should_autoscroll(&self) -> bool {
        // Get cursor position if available
        if let Some((x, y)) = self.cursor_position() {
            // Check cache first
            if let Some(result) = self.check_cache(x, y) {
                debug!("AT-SPI cache hit at ({}, {}): {}", x, y, result);
                return result;
            }
        }

        // For now, fall back to heuristic detection
        // TODO: Implement proper AT-SPI tree walking
        let result = self.heuristic.should_autoscroll();
        
        if let Some((x, y)) = self.cursor_position() {
            self.cache_result(x, y, result);
        }

        result
    }

    fn cursor_position(&self) -> Option<(i32, i32)> {
        #[cfg(feature = "x11")]
        {
            if let Some((ref conn, root)) = self.x11_conn {
                if let Ok(cookie) = conn.query_pointer(root) {
                    if let Ok(reply) = cookie.reply() {
                        return Some((reply.root_x as i32, reply.root_y as i32));
                    }
                }
            }
        }

        None
    }

    fn clear_cache(&self) {
        if let Ok(mut cache) = self.cache.write() {
            cache.clear();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_scrollable_roles() {
        assert!(AtSpiScrollDetector::is_scrollable_role("scroll pane"));
        assert!(AtSpiScrollDetector::is_scrollable_role("Scroll Pane"));
        assert!(AtSpiScrollDetector::is_scrollable_role("document web"));
        assert!(AtSpiScrollDetector::is_scrollable_role("terminal"));
        assert!(!AtSpiScrollDetector::is_scrollable_role("push button"));
    }

    #[test]
    fn test_deny_roles() {
        assert!(AtSpiScrollDetector::is_deny_role("menu bar"));
        assert!(AtSpiScrollDetector::is_deny_role("Menu Bar"));
        assert!(AtSpiScrollDetector::is_deny_role("tool bar"));
        assert!(AtSpiScrollDetector::is_deny_role("push button"));
        assert!(!AtSpiScrollDetector::is_deny_role("scroll pane"));
    }
}
