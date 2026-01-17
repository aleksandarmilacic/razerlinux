#!/bin/bash
# RazerLinux Installation Script
# Installs RazerLinux system-wide with proper permissions

set -e

INSTALL_DIR="/opt/razerlinux"
BIN_NAME="razerlinux"
DESKTOP_FILE="/usr/share/applications/razerlinux.desktop"
ICON_DIR="/usr/share/icons/hicolor/scalable/apps"
ICON_FILE="$ICON_DIR/razerlinux.svg"
UDEV_RULES="/etc/udev/rules.d/99-razerlinux.rules"
POLKIT_RULE="/usr/share/polkit-1/actions/org.razerlinux.policy"

echo "=== RazerLinux Installer ==="
echo ""

# Check if running as root
if [ "$EUID" -ne 0 ]; then
    echo "Please run as root: sudo ./install.sh"
    exit 1
fi

# Get the actual user (not root)
REAL_USER="${SUDO_USER:-$USER}"
REAL_HOME=$(getent passwd "$REAL_USER" | cut -d: -f6)

echo "Installing for user: $REAL_USER"
echo ""

# Parse CLI flags (fallback to env)
BUILD_PROFILE="${BUILD_PROFILE:-release}"
if [ "$1" = "--debug" ]; then
  BUILD_PROFILE="debug"
fi
if [ "$1" = "--release" ]; then
  BUILD_PROFILE="release"
fi
if [ "$1" = "--skip-build" ] || [ "$2" = "--skip-build" ]; then
  SKIP_BUILD=1
fi

# Build version (can be skipped with SKIP_BUILD=1)
echo "[1/6] Building ${BUILD_PROFILE} version..."
cd "$(dirname "$0")"
if [ "$BUILD_PROFILE" = "debug" ]; then
  BUILD_CMD="cargo build"
  BIN_PATH="target/debug/$BIN_NAME"
else
  BUILD_CMD="cargo build --release"
  BIN_PATH="target/release/$BIN_NAME"
fi

if [ -z "$SKIP_BUILD" ]; then
  sudo -u "$REAL_USER" $BUILD_CMD
else
  echo "SKIP_BUILD=1 set, using existing binary"
fi

# Create installation directory
echo "[2/6] Creating installation directory..."
mkdir -p "$INSTALL_DIR"
if [ ! -f "$BIN_PATH" ]; then
  echo "ERROR: $BIN_PATH not found. Build first or unset SKIP_BUILD."
  exit 1
fi
cp "$BIN_PATH" "$INSTALL_DIR/"
chmod 755 "$INSTALL_DIR/$BIN_NAME"

# Install udev rules for non-root HID access
echo "[3/6] Installing udev rules..."
cat > "$UDEV_RULES" << 'EOF'
# RazerLinux - Allow user access to Razer devices
# Razer Naga Trinity
SUBSYSTEM=="hidraw", ATTRS{idVendor}=="1532", ATTRS{idProduct}=="0067", MODE="0666"
SUBSYSTEM=="usb", ATTRS{idVendor}=="1532", ATTRS{idProduct}=="0067", MODE="0666"

# Razer Naga X
SUBSYSTEM=="hidraw", ATTRS{idVendor}=="1532", ATTRS{idProduct}=="0096", MODE="0666"
SUBSYSTEM=="usb", ATTRS{idVendor}=="1532", ATTRS{idProduct}=="0096", MODE="0666"

# Razer Naga Pro
SUBSYSTEM=="hidraw", ATTRS{idVendor}=="1532", ATTRS{idProduct}=="008f", MODE="0666"
SUBSYSTEM=="usb", ATTRS{idVendor}=="1532", ATTRS{idProduct}=="008f", MODE="0666"

# General Razer mice (for future support)
SUBSYSTEM=="hidraw", ATTRS{idVendor}=="1532", MODE="0666"
EOF

# Install polkit policy for privileged operations
echo "[4/6] Installing polkit policy..."
cat > "$POLKIT_RULE" << EOF
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE policyconfig PUBLIC
 "-//freedesktop//DTD PolicyKit Policy Configuration 1.0//EN"
 "http://www.freedesktop.org/standards/PolicyKit/1/policyconfig.dtd">
<policyconfig>
  <action id="org.razerlinux.run">
    <description>Run RazerLinux</description>
    <message>Authentication is required to configure Razer devices</message>
    <defaults>
      <allow_any>auth_admin</allow_any>
      <allow_inactive>auth_admin</allow_inactive>
      <allow_active>auth_admin_keep</allow_active>
    </defaults>
    <annotate key="org.freedesktop.policykit.exec.path">$INSTALL_DIR/$BIN_NAME</annotate>
    <annotate key="org.freedesktop.policykit.exec.allow_gui">true</annotate>
  </action>
</policyconfig>
EOF

# Create wrapper script that uses pkexec
echo "[5/6] Creating launcher wrapper..."
cat > "$INSTALL_DIR/razerlinux-launcher" << 'EOF'
#!/bin/bash
# RazerLinux launcher with privilege escalation and tray helper

# Cleanup function to stop tray helper when main app exits
cleanup() {
    if [ -n "$TRAY_PID" ]; then
        kill "$TRAY_PID" 2>/dev/null
    fi
}
trap cleanup EXIT

# Start the tray helper as the current user (runs in user session for tray icon)
# The tray helper creates a Unix socket for IPC with the main app
/opt/razerlinux/razerlinux --tray-helper &
TRAY_PID=$!

# Give the tray helper time to start and create the socket
sleep 0.3

# Check if udev rules allow non-root access
if [ -r /dev/hidraw0 ] 2>/dev/null; then
    # Can access hidraw without root, run directly
    /opt/razerlinux/razerlinux "$@"
else
    # Need elevated privileges - pass XDG_RUNTIME_DIR for socket path
    pkexec env \
        DISPLAY="$DISPLAY" \
        XAUTHORITY="$XAUTHORITY" \
        XDG_RUNTIME_DIR="$XDG_RUNTIME_DIR" \
        /opt/razerlinux/razerlinux "$@"
fi
EOF
chmod 755 "$INSTALL_DIR/razerlinux-launcher"

# Create symlink in /usr/local/bin
ln -sf "$INSTALL_DIR/razerlinux-launcher" /usr/local/bin/razerlinux

# Create desktop entry
echo "[6/6] Installing icon and desktop entry..."
mkdir -p "$ICON_DIR"
cp "$(dirname "$0")/assets/razerlinux.svg" "$ICON_FILE"
cat > "$DESKTOP_FILE" << EOF
[Desktop Entry]
Name=RazerLinux
Comment=Razer Mouse Configuration Tool
Exec=/usr/local/bin/razerlinux
Icon=razerlinux
Terminal=false
Type=Application
Categories=Settings;HardwareSettings;
Keywords=razer;mouse;gaming;dpi;macro;
StartupNotify=true
EOF

# Reload udev rules
echo ""
echo "Reloading udev rules..."
udevadm control --reload-rules
udevadm trigger

echo ""
echo "=== Installation Complete ==="
echo ""
echo "You may need to:"
echo "  1. Unplug and replug your Razer mouse for udev rules to take effect"
echo "  2. Log out and log back in for group changes to apply"
echo ""
echo "To run RazerLinux:"
echo "  - From terminal: razerlinux"
echo "  - From app menu: Search for 'RazerLinux'"
echo ""
echo "To enable autostart:"
echo "  - Open RazerLinux → Settings → Enable 'Start on system startup'"
echo ""
echo "To uninstall: sudo /opt/razerlinux/uninstall.sh"
