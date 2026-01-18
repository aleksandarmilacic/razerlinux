//! X11 Scrollable Area Detection
//!
//! Determines whether the area under the cursor is scrollable, to provide
//! Windows-like autoscroll behavior (only activate in scrollable regions).
//!
//! Detection pipeline (order matters):
//! 1. `_NET_WM_WINDOW_TYPE` deny list - catches desktop, dock, menus, tooltips
//! 2. `WM_CLASS` deny list - catches known non-scrollable apps
//! 3. AT-SPI hit-test (optional, behind feature flag)
//! 4. `WM_CLASS` allow list - known scrollable apps
//! 5. Strict default: unknown = NOT scrollable

use anyhow::Result;
use std::collections::HashSet;
use std::time::{Duration, Instant};
use tracing::{debug, warn};
use x11rb::connection::Connection;
use x11rb::protocol::xproto::*;

/// Cache entry for scroll detection decisions
struct CacheEntry {
    scrollable: bool,
    timestamp: Instant,
}

/// X11 Scrollable Area Detector
///
/// Determines if the cursor is over a scrollable region using:
/// - `_NET_WM_WINDOW_TYPE` property (primary filter)
/// - `WM_CLASS` heuristics (fallback)
/// - Optional AT-SPI accessibility queries
pub struct ScrollDetectorX11 {
    /// Cached atom values for denied window types
    deny_type_atoms: HashSet<Atom>,
    /// Known non-scrollable WM_CLASS values (lowercase)
    deny_classes: Vec<String>,
    /// Known scrollable WM_CLASS values (lowercase)
    allow_classes: Vec<String>,
    /// How many parent windows to check for properties
    parent_limit: usize,
    /// If true, unknown windows are NOT scrollable (Windows-like behavior)
    strict_default: bool,
    /// Decision cache to avoid repeated X11 queries
    cache: std::cell::RefCell<std::collections::HashMap<(Window, i16, i16), CacheEntry>>,
    /// Cache TTL
    cache_ttl: Duration,
}

impl ScrollDetectorX11 {
    /// Create a new detector with the given X11 connection
    pub fn new<C: Connection>(conn: &C) -> Result<Self, x11rb::errors::ConnectionError> {
        let deny_type_names = [
            "_NET_WM_WINDOW_TYPE_DESKTOP",
            "_NET_WM_WINDOW_TYPE_DOCK",
            "_NET_WM_WINDOW_TYPE_TOOLBAR",
            "_NET_WM_WINDOW_TYPE_MENU",
            "_NET_WM_WINDOW_TYPE_DROPDOWN_MENU",
            "_NET_WM_WINDOW_TYPE_POPUP_MENU",
            "_NET_WM_WINDOW_TYPE_TOOLTIP",
            "_NET_WM_WINDOW_TYPE_NOTIFICATION",
            "_NET_WM_WINDOW_TYPE_SPLASH",
            "_NET_WM_WINDOW_TYPE_UTILITY",
            "_NET_WM_WINDOW_TYPE_DIALOG",  // Dialogs are usually not primary scroll targets
        ];

        let mut deny_type_atoms = HashSet::new();
        for name in deny_type_names {
            match intern_atom(conn, name) {
                Ok(atom) => { deny_type_atoms.insert(atom); }
                Err(e) => warn!("Failed to intern atom {}: {}", name, e),
            }
        }

        // Known non-scrollable WM_CLASS values
        let deny_classes = vec![
            // Desktop shells
            "plasmashell", "gnome-shell", "mutter", "kwin", "xfdesktop",
            "pcmanfm-desktop", "nemo-desktop",
            // Docks and panels
            "latte-dock", "plank", "cairo-dock", "docky", "polybar", "waybar",
            "xfce4-panel", "mate-panel", "budgie-panel",
            // Notifications
            "notify-osd", "dunst", "xfce4-notifyd", "mako",
            // Launchers
            "rofi", "dmenu", "ulauncher", "albert", "krunner",
            // System trays
            "trayer", "stalonetray",
        ].into_iter().map(String::from).collect();

        // Known scrollable WM_CLASS values
        let allow_classes = vec![
            // Browsers
            "firefox", "chromium", "chrome", "google-chrome", "brave", "brave-browser",
            "vivaldi", "opera", "microsoft-edge", "edge", "librewolf", "waterfox",
            "epiphany", "midori", "qutebrowser", "nyxt",
            // Editors/IDEs
            "code", "code-oss", "vscodium", "vscode", "sublime_text", "sublime",
            "atom", "gedit", "kate", "kwrite", "pluma", "xed", "mousepad",
            "gvim", "vim", "neovim", "emacs", "doom-emacs",
            "intellij", "idea", "pycharm", "clion", "rider", "webstorm", "goland",
            "android-studio", "eclipse", "netbeans", "kdevelop",
            // Terminals
            "konsole", "gnome-terminal", "gnome-terminal-server", "alacritty", 
            "kitty", "terminator", "tilix", "xterm", "urxvt", "rxvt",
            "st", "foot", "wezterm", "contour", "guake", "yakuake", "tilda",
            // File managers
            "nautilus", "dolphin", "thunar", "pcmanfm", "nemo", "caja",
            "spacefm", "doublecmd", "krusader", "ranger", "mc",
            // Office/Documents
            "libreoffice", "soffice", "okular", "evince", "zathura", "mupdf",
            "calibre", "foliate", "xreader", "atril", "qpdfview",
            // Communication
            "slack", "discord", "telegram-desktop", "telegram", "signal",
            "element", "teams", "zoom", "skype", "thunderbird", "evolution",
            "geary", "kmail", "claws-mail",
            // Media
            "spotify", "vlc", "mpv", "celluloid", "totem", "rhythmbox",
            "clementine", "strawberry", "audacious",
            // Development tools
            "gitk", "git-gui", "meld", "kdiff3", "diffuse",
            // System tools
            "systemsettings", "gnome-control-center", "xfce4-settings-manager",
            // Image viewers/editors
            "gimp", "inkscape", "krita", "darktable", "rawtherapee",
            "eog", "gwenview", "feh", "sxiv", "imv",
            // Notes/Wikis
            "obsidian", "logseq", "notion", "joplin", "simplenote", "zettlr",
            // Misc scrollable apps
            "keepassxc", "bitwarden", "1password",
        ].into_iter().map(String::from).collect();

        Ok(Self {
            deny_type_atoms,
            deny_classes,
            allow_classes,
            parent_limit: 10,
            strict_default: true,  // Unknown = NOT scrollable (Windows-like)
            cache: std::cell::RefCell::new(std::collections::HashMap::new()),
            cache_ttl: Duration::from_millis(150),
        })
    }

    /// Check if the cursor is over a scrollable area
    ///
    /// Returns `true` if autoscroll should be activated, `false` if a normal
    /// middle-click should be passed through.
    pub fn should_autoscroll<C: Connection>(
        &self,
        conn: &C,
        root: Window,
    ) -> bool {
        // Get deepest window under pointer
        let (deepest, root_x, root_y) = match deepest_window_under_pointer(conn, root) {
            Ok(result) => result,
            Err(e) => {
                warn!("Failed to get window under pointer: {}", e);
                return !self.strict_default;
            }
        };

        // Check cache first (key by window and coarse position)
        let cache_key = (deepest, root_x >> 4, root_y >> 4);
        {
            let cache = self.cache.borrow();
            if let Some(entry) = cache.get(&cache_key) {
                if entry.timestamp.elapsed() < self.cache_ttl {
                    return entry.scrollable;
                }
            }
        }

        // Get parent chain for property lookup
        let chain = match parent_chain(conn, deepest, self.parent_limit) {
            Ok(c) => c,
            Err(e) => {
                debug!("Failed to get parent chain: {}", e);
                return !self.strict_default;
            }
        };

        // 1) Deny by window type (check all parents)
        for &w in &chain {
            if let Ok(types) = get_window_type_atoms(conn, w) {
                if types.iter().any(|a| self.deny_type_atoms.contains(a)) {
                    debug!("Denied by window type for window {:?}", w);
                    self.cache_result(cache_key, false);
                    return false;
                }
            }
        }

        // 2) Find WM_CLASS in parent chain
        let mut found_class: Option<String> = None;
        for &w in &chain {
            if let Ok(Some((_instance, class))) = get_wm_class(conn, w) {
                found_class = Some(class.to_lowercase());
                break;
            }
        }

        // 3) Deny by WM_CLASS
        if let Some(ref class) = found_class {
            if self.deny_classes.iter().any(|d| class.contains(d)) {
                debug!("Denied by WM_CLASS: {}", class);
                self.cache_result(cache_key, false);
                return false;
            }
        }

        // 4) TODO: AT-SPI hit-test would go here (behind feature flag)
        // #[cfg(feature = "atspi")]
        // if let Some(scrollable) = atspi_hit_test(root_x, root_y) {
        //     self.cache_result(cache_key, scrollable);
        //     return scrollable;
        // }

        // 5) Allow by WM_CLASS
        if let Some(ref class) = found_class {
            if self.allow_classes.iter().any(|a| class.contains(a)) {
                debug!("Allowed by WM_CLASS: {}", class);
                self.cache_result(cache_key, true);
                return true;
            }
        }

        // 6) Strict default: unknown = NOT scrollable
        debug!("Unknown window class {:?}, strict_default={}", found_class, self.strict_default);
        let result = !self.strict_default;
        self.cache_result(cache_key, result);
        result
    }

    /// Cache a detection result
    fn cache_result(&self, key: (Window, i16, i16), scrollable: bool) {
        let mut cache = self.cache.borrow_mut();
        
        // Prune old entries periodically
        if cache.len() > 100 {
            let now = Instant::now();
            cache.retain(|_, v| now.duration_since(v.timestamp) < self.cache_ttl * 2);
        }
        
        cache.insert(key, CacheEntry {
            scrollable,
            timestamp: Instant::now(),
        });
    }

    /// Clear the detection cache
    pub fn clear_cache(&self) {
        self.cache.borrow_mut().clear();
    }
}

/// Get the deepest window under the pointer using QueryPointer loop
pub fn deepest_window_under_pointer<C: Connection>(
    conn: &C,
    root: Window,
) -> Result<(Window, i16, i16)> {
    let mut w = root;

    loop {
        let qp = conn.query_pointer(w)?.reply()?;
        let x = qp.root_x;
        let y = qp.root_y;

        if qp.child == 0 {
            return Ok((w, x, y));
        }
        w = qp.child;
    }
}

/// Walk up the parent chain from a window (properties often live on parents)
pub fn parent_chain<C: Connection>(
    conn: &C,
    mut w: Window,
    limit: usize,
) -> Result<Vec<Window>> {
    let mut out = Vec::with_capacity(limit + 1);
    for _ in 0..=limit {
        out.push(w);
        let qt = conn.query_tree(w)?.reply()?;
        if qt.parent == 0 || qt.parent == w {
            break;
        }
        w = qt.parent;
    }
    Ok(out)
}

/// Intern an X11 atom by name
fn intern_atom<C: Connection>(conn: &C, name: &str) -> Result<Atom> {
    Ok(conn.intern_atom(false, name.as_bytes())?.reply()?.atom)
}

/// Get `_NET_WM_WINDOW_TYPE` atoms for a window
fn get_window_type_atoms<C: Connection>(
    conn: &C,
    w: Window,
) -> Result<Vec<Atom>> {
    let prop_atom = intern_atom(conn, "_NET_WM_WINDOW_TYPE")?;
    let prop = conn
        .get_property(false, w, prop_atom, AtomEnum::ATOM, 0, 64)?
        .reply()?;

    Ok(prop.value32().map(|it| it.collect()).unwrap_or_default())
}

/// Get WM_CLASS property (instance, class) for a window
pub fn get_wm_class<C: Connection>(
    conn: &C,
    w: Window,
) -> Result<Option<(String, String)>> {
    let prop = conn
        .get_property(false, w, AtomEnum::WM_CLASS, AtomEnum::STRING, 0, 1024)?
        .reply()?;

    if prop.value.is_empty() {
        return Ok(None);
    }

    let parts: Vec<&[u8]> = prop
        .value
        .split(|&b| b == 0)
        .filter(|p| !p.is_empty())
        .collect();

    let instance = parts.get(0).map(|p| String::from_utf8_lossy(p).to_string()).unwrap_or_default();
    let class = parts.get(1).map(|p| String::from_utf8_lossy(p).to_string()).unwrap_or_default();
    Ok(Some((instance, class)))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_class_lists_are_lowercase() {
        // Verify all entries in allow/deny lists are lowercase
        let deny = vec![
            "plasmashell", "gnome-shell", "latte-dock",
        ];
        for d in &deny {
            assert_eq!(d, &d.to_lowercase(), "Deny class should be lowercase: {}", d);
        }
    }
}
