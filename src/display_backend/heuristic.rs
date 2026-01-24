//! Heuristic-based Scroll Detection
//!
//! Provides application name-based heuristics for determining if an
//! application is likely to have scrollable content. Used as a fallback
//! when more accurate detection (AT-SPI, X11 properties) is unavailable.

use super::ScrollDetector;

/// Known non-scrollable WM_CLASS/app values (lowercase)
pub const DENY_CLASSES: &[&str] = &[
    // Desktop shells
    "plasmashell",
    "gnome-shell",
    "mutter",
    "kwin",
    "xfdesktop",
    "pcmanfm-desktop",
    "nemo-desktop",
    // Docks and panels
    "latte-dock",
    "plank",
    "cairo-dock",
    "docky",
    "polybar",
    "waybar",
    "xfce4-panel",
    "mate-panel",
    "budgie-panel",
    // Notifications
    "notify-osd",
    "dunst",
    "xfce4-notifyd",
    "mako",
    // Launchers
    "rofi",
    "dmenu",
    "ulauncher",
    "albert",
    "krunner",
    "wofi",
    "fuzzel",
    "tofi",
    // System trays
    "trayer",
    "stalonetray",
];

/// Known scrollable WM_CLASS/app values (lowercase)
pub const ALLOW_CLASSES: &[&str] = &[
    // Browsers
    "firefox",
    "chromium",
    "chrome",
    "google-chrome",
    "brave",
    "brave-browser",
    "vivaldi",
    "opera",
    "microsoft-edge",
    "edge",
    "librewolf",
    "waterfox",
    "epiphany",
    "midori",
    "qutebrowser",
    "nyxt",
    "zen-browser",
    "floorp",
    // Editors/IDEs
    "code",
    "code-oss",
    "vscodium",
    "vscode",
    "sublime_text",
    "sublime",
    "atom",
    "gedit",
    "kate",
    "kwrite",
    "pluma",
    "xed",
    "mousepad",
    "gvim",
    "vim",
    "neovim",
    "emacs",
    "doom-emacs",
    "intellij",
    "idea",
    "pycharm",
    "clion",
    "rider",
    "webstorm",
    "goland",
    "android-studio",
    "eclipse",
    "netbeans",
    "kdevelop",
    "zed",
    "lapce",
    "helix",
    // Terminals
    "konsole",
    "gnome-terminal",
    "gnome-terminal-server",
    "alacritty",
    "kitty",
    "terminator",
    "tilix",
    "xterm",
    "urxvt",
    "rxvt",
    "st",
    "foot",
    "wezterm",
    "contour",
    "guake",
    "yakuake",
    "tilda",
    "blackbox",
    "ghostty",
    // File managers
    "nautilus",
    "dolphin",
    "thunar",
    "pcmanfm",
    "nemo",
    "caja",
    "spacefm",
    "doublecmd",
    "krusader",
    "ranger",
    "mc",
    "files",
    // Office/Documents
    "libreoffice",
    "soffice",
    "okular",
    "evince",
    "zathura",
    "mupdf",
    "calibre",
    "foliate",
    "xreader",
    "atril",
    "qpdfview",
    "papers",
    // Communication
    "slack",
    "discord",
    "telegram-desktop",
    "telegram",
    "signal",
    "element",
    "teams",
    "zoom",
    "skype",
    "thunderbird",
    "evolution",
    "geary",
    "kmail",
    "claws-mail",
    "vesktop",
    // Media
    "spotify",
    "vlc",
    "mpv",
    "celluloid",
    "totem",
    "rhythmbox",
    "clementine",
    "strawberry",
    "audacious",
    // Development tools
    "gitk",
    "git-gui",
    "meld",
    "kdiff3",
    "diffuse",
    // System tools
    "systemsettings",
    "gnome-control-center",
    "xfce4-settings-manager",
    // Image viewers/editors
    "gimp",
    "inkscape",
    "krita",
    "darktable",
    "rawtherapee",
    "eog",
    "gwenview",
    "feh",
    "sxiv",
    "imv",
    "loupe",
    // Notes/Wikis
    "obsidian",
    "logseq",
    "notion",
    "joplin",
    "simplenote",
    "zettlr",
    // Misc scrollable apps
    "keepassxc",
    "bitwarden",
    "1password",
];

/// Heuristic-only scroll detector
///
/// Uses only application name matching, without any display server queries.
/// This is the fallback for Wayland when AT-SPI is unavailable.
pub struct HeuristicScrollDetector {
    /// If true, unknown apps are NOT scrollable
    strict_default: bool,
}

impl HeuristicScrollDetector {
    pub fn new() -> Self {
        Self {
            strict_default: true,
        }
    }

    /// Check if an app name indicates scrollable content
    pub fn is_scrollable_app(app_name: &str) -> bool {
        let app_lower = app_name.to_lowercase();

        // Check deny list first
        if DENY_CLASSES.iter().any(|d| app_lower.contains(d)) {
            return false;
        }

        // Check allow list
        if ALLOW_CLASSES.iter().any(|a| app_lower.contains(a)) {
            return true;
        }

        // Unknown - default to not scrollable
        false
    }
}

impl Default for HeuristicScrollDetector {
    fn default() -> Self {
        Self::new()
    }
}

impl ScrollDetector for HeuristicScrollDetector {
    fn should_autoscroll(&self) -> bool {
        // Without additional context, we can't determine the focused app
        // Return the strict default (no autoscroll for unknown)
        !self.strict_default
    }

    fn cursor_position(&self) -> Option<(i32, i32)> {
        // No way to get cursor position without display server access
        None
    }

    fn clear_cache(&self) {
        // No cache in heuristic detector
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_deny_classes_lowercase() {
        for class in DENY_CLASSES {
            assert_eq!(
                *class,
                class.to_lowercase(),
                "Deny class should be lowercase: {}",
                class
            );
        }
    }

    #[test]
    fn test_allow_classes_lowercase() {
        for class in ALLOW_CLASSES {
            assert_eq!(
                *class,
                class.to_lowercase(),
                "Allow class should be lowercase: {}",
                class
            );
        }
    }

    #[test]
    fn test_known_apps() {
        assert!(HeuristicScrollDetector::is_scrollable_app("firefox"));
        assert!(HeuristicScrollDetector::is_scrollable_app("Firefox"));
        assert!(HeuristicScrollDetector::is_scrollable_app("code"));
        assert!(HeuristicScrollDetector::is_scrollable_app("konsole"));

        assert!(!HeuristicScrollDetector::is_scrollable_app("plasmashell"));
        assert!(!HeuristicScrollDetector::is_scrollable_app("rofi"));
        assert!(!HeuristicScrollDetector::is_scrollable_app("dunst"));
    }
}
