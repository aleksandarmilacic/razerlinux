#!/bin/bash
# Build RazerLinux Flatpak
# Requires: flatpak-builder, org.freedesktop.Platform//23.08

set -e

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
PROJECT_DIR="$(dirname "$SCRIPT_DIR")"
BUILD_DIR="$PROJECT_DIR/build/flatpak"

echo "=== Building RazerLinux Flatpak ==="
echo ""

# Check for flatpak-builder
if ! command -v flatpak-builder &> /dev/null; then
    echo "Error: flatpak-builder not found"
    echo "Install with: sudo zypper install flatpak-builder"
    exit 1
fi

# Install runtime if needed
echo "[1/4] Checking Flatpak runtime..."
flatpak install -y --user flathub org.freedesktop.Platform//23.08 org.freedesktop.Sdk//23.08 || true
flatpak install -y --user flathub org.freedesktop.Sdk.Extension.rust-stable//23.08 || true

# Create manifest
echo "[2/4] Creating Flatpak manifest..."
mkdir -p "$BUILD_DIR"

cat > "$BUILD_DIR/org.razerlinux.RazerLinux.yml" << EOF
app-id: org.razerlinux.RazerLinux
runtime: org.freedesktop.Platform
runtime-version: '23.08'
sdk: org.freedesktop.Sdk
sdk-extensions:
  - org.freedesktop.Sdk.Extension.rust-stable
command: razerlinux

finish-args:
  # X11 access
  - --share=ipc
  - --socket=x11
  - --socket=fallback-x11
  # Wayland
  - --socket=wayland
  # USB/HID device access
  - --device=all
  # System tray
  - --talk-name=org.kde.StatusNotifierWatcher
  - --talk-name=org.freedesktop.Notifications
  # Config storage
  - --filesystem=~/.config/razerlinux:create

build-options:
  append-path: /usr/lib/sdk/rust-stable/bin
  env:
    CARGO_HOME: /run/build/razerlinux/cargo
    RUSTUP_HOME: /usr/lib/sdk/rust-stable/rustup

modules:
  - name: razerlinux
    buildsystem: simple
    build-commands:
      - cargo build --release
      - install -Dm755 target/release/razerlinux /app/bin/razerlinux
      - install -Dm644 assets/razerlinux.svg /app/share/icons/hicolor/scalable/apps/org.razerlinux.RazerLinux.svg
      - install -Dm644 flatpak/org.razerlinux.RazerLinux.desktop /app/share/applications/org.razerlinux.RazerLinux.desktop
    sources:
      - type: dir
        path: $PROJECT_DIR
EOF

# Create desktop file for Flatpak
mkdir -p "$PROJECT_DIR/flatpak"
cat > "$PROJECT_DIR/flatpak/org.razerlinux.RazerLinux.desktop" << EOF
[Desktop Entry]
Name=RazerLinux
Comment=Razer Mouse Configuration Tool
Exec=razerlinux
Icon=org.razerlinux.RazerLinux
Terminal=false
Type=Application
Categories=Settings;HardwareSettings;
Keywords=razer;mouse;gaming;dpi;macro;
StartupNotify=true
EOF

# Create appdata
cat > "$PROJECT_DIR/flatpak/org.razerlinux.RazerLinux.metainfo.xml" << EOF
<?xml version="1.0" encoding="UTF-8"?>
<component type="desktop-application">
  <id>org.razerlinux.RazerLinux</id>
  <name>RazerLinux</name>
  <summary>Razer Mouse Configuration Tool</summary>
  <metadata_license>CC0-1.0</metadata_license>
  <project_license>GPL-3.0-only</project_license>
  <description>
    <p>
      RazerLinux is an open-source configuration tool for Razer mice on Linux.
      It supports the Razer Naga Trinity with features including:
    </p>
    <ul>
      <li>Button remapping and macros</li>
      <li>DPI settings with per-profile configuration</li>
      <li>RGB lighting control</li>
      <li>Profile management</li>
      <li>System tray integration</li>
    </ul>
  </description>
  <url type="homepage">https://github.com/aleksandarmilacic/razerlinux</url>
  <url type="bugtracker">https://github.com/aleksandarmilacic/razerlinux/issues</url>
  <developer_name>Aleksandar Milacic</developer_name>
  <releases>
    <release version="0.1.0" date="2026-01-17">
      <description>
        <p>Initial release with core functionality</p>
      </description>
    </release>
  </releases>
</component>
EOF

echo "[3/4] Building Flatpak..."
cd "$BUILD_DIR"
flatpak-builder --user --install --force-clean build-dir org.razerlinux.RazerLinux.yml

echo ""
echo "[4/4] Creating distributable bundle..."
flatpak build-bundle ~/.local/share/flatpak/repo \
    "$PROJECT_DIR/RazerLinux.flatpak" \
    org.razerlinux.RazerLinux

echo ""
echo "=== Flatpak built successfully! ==="
echo "Output: $PROJECT_DIR/RazerLinux.flatpak"
echo ""
echo "To install: flatpak install --user RazerLinux.flatpak"
echo "To run: flatpak run org.razerlinux.RazerLinux"
echo ""
echo "Note: You still need udev rules for device access:"
echo "  sudo cp $PROJECT_DIR/config/99-razermouse.rules /etc/udev/rules.d/"
echo "  sudo udevadm control --reload-rules && sudo udevadm trigger"
