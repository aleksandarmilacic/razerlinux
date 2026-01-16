#!/bin/bash
# RazerLinux Installation Script
# Installs RazerLinux system-wide with proper permissions

set -e

INSTALL_DIR="/opt/razerlinux"
BIN_NAME="razerlinux"
DESKTOP_FILE="/usr/share/applications/razerlinux.desktop"
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

# Build release version
echo "[1/6] Building release version..."
cd "$(dirname "$0")"
sudo -u "$REAL_USER" cargo build --release

# Create installation directory
echo "[2/6] Creating installation directory..."
mkdir -p "$INSTALL_DIR"
cp target/release/$BIN_NAME "$INSTALL_DIR/"
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
# RazerLinux launcher with privilege escalation

# Check if udev rules allow non-root access
if [ -r /dev/hidraw0 ] 2>/dev/null; then
    # Can access hidraw without root, run directly
    /opt/razerlinux/razerlinux "$@"
else
    # Need elevated privileges
    pkexec env DISPLAY="$DISPLAY" XAUTHORITY="$XAUTHORITY" /opt/razerlinux/razerlinux "$@"
fi
EOF
chmod 755 "$INSTALL_DIR/razerlinux-launcher"

# Create symlink in /usr/local/bin
ln -sf "$INSTALL_DIR/razerlinux-launcher" /usr/local/bin/razerlinux

# Create desktop entry
echo "[6/6] Creating desktop entry..."
cat > "$DESKTOP_FILE" << EOF
[Desktop Entry]
Name=RazerLinux
Comment=Razer Mouse Configuration Tool
Exec=/usr/local/bin/razerlinux
Icon=input-mouse
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
