#!/bin/bash
# Build RazerLinux AppImage
# Requires: appimagetool (https://github.com/AppImage/AppImageKit)

set -e

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
PROJECT_DIR="$(dirname "$SCRIPT_DIR")"
BUILD_DIR="$PROJECT_DIR/build/appimage"
APPDIR="$BUILD_DIR/RazerLinux.AppDir"

echo "=== Building RazerLinux AppImage ==="
echo ""

# Check for appimagetool
if ! command -v appimagetool &> /dev/null; then
    echo "appimagetool not found. Installing..."
    ARCH=$(uname -m)
    wget -q "https://github.com/AppImage/AppImageKit/releases/download/continuous/appimagetool-${ARCH}.AppImage" \
        -O /tmp/appimagetool
    chmod +x /tmp/appimagetool
    APPIMAGETOOL="/tmp/appimagetool"
else
    APPIMAGETOOL="appimagetool"
fi

# Build release binary
echo "[1/4] Building release binary..."
cd "$PROJECT_DIR"
cargo build --release

# Create AppDir structure
echo "[2/4] Creating AppDir structure..."
rm -rf "$BUILD_DIR"
mkdir -p "$APPDIR/usr/bin"
mkdir -p "$APPDIR/usr/share/applications"
mkdir -p "$APPDIR/usr/share/icons/hicolor/scalable/apps"

# Copy binary
cp "$PROJECT_DIR/target/release/razerlinux" "$APPDIR/usr/bin/"

# Copy icon
cp "$PROJECT_DIR/assets/razerlinux.svg" "$APPDIR/usr/share/icons/hicolor/scalable/apps/"
cp "$PROJECT_DIR/assets/razerlinux.svg" "$APPDIR/razerlinux.svg"

# Create desktop file
cat > "$APPDIR/razerlinux.desktop" << EOF
[Desktop Entry]
Name=RazerLinux
Comment=Razer Mouse Configuration Tool
Exec=razerlinux
Icon=razerlinux
Terminal=false
Type=Application
Categories=Settings;HardwareSettings;
Keywords=razer;mouse;gaming;dpi;macro;
StartupNotify=true
EOF

cp "$APPDIR/razerlinux.desktop" "$APPDIR/usr/share/applications/"

# Create AppRun script
cat > "$APPDIR/AppRun" << 'EOF'
#!/bin/bash
SELF=$(readlink -f "$0")
HERE=${SELF%/*}
export PATH="${HERE}/usr/bin:${PATH}"
export LD_LIBRARY_PATH="${HERE}/usr/lib:${LD_LIBRARY_PATH}"

# Check if we need elevated privileges for HID access
if [ -r /dev/hidraw0 ] 2>/dev/null; then
    exec "${HERE}/usr/bin/razerlinux" "$@"
else
    # Try with pkexec if available
    if command -v pkexec &> /dev/null; then
        exec pkexec env \
            DISPLAY="$DISPLAY" \
            XAUTHORITY="$XAUTHORITY" \
            XDG_RUNTIME_DIR="$XDG_RUNTIME_DIR" \
            "${HERE}/usr/bin/razerlinux" "$@"
    else
        echo "Note: Run with sudo or set up udev rules for non-root access"
        exec "${HERE}/usr/bin/razerlinux" "$@"
    fi
fi
EOF
chmod +x "$APPDIR/AppRun"

# Build AppImage
echo "[3/4] Building AppImage..."
cd "$BUILD_DIR"
ARCH=$(uname -m) "$APPIMAGETOOL" "$APPDIR" "RazerLinux-${ARCH}.AppImage"

# Move to project root
mv "RazerLinux-${ARCH}.AppImage" "$PROJECT_DIR/"

echo ""
echo "[4/4] Cleanup..."
rm -rf "$BUILD_DIR"

echo ""
echo "=== AppImage built successfully! ==="
echo "Output: $PROJECT_DIR/RazerLinux-${ARCH}.AppImage"
echo ""
echo "To run: ./RazerLinux-${ARCH}.AppImage"
echo ""
echo "Note: For non-root access, install udev rules:"
echo "  sudo cp $PROJECT_DIR/config/99-razermouse.rules /etc/udev/rules.d/"
echo "  sudo udevadm control --reload-rules && sudo udevadm trigger"
