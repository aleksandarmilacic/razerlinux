#!/bin/bash
# Cursor calibration helper for RazerLinux
# This script helps diagnose cursor position tracking issues

echo "=== RazerLinux Cursor Calibration ==="
echo ""
echo "This will help us understand the cursor tracking offset."
echo "You'll point to specific positions and press Enter."
echo ""

# First, make sure razerlinux is not running (it grabs evdev)
pkill razerlinux 2>/dev/null
sleep 1

# Get screen geometry from xrandr
echo "Detecting monitors..."
xrandr --query | grep " connected" | while read line; do
    echo "  $line"
done
echo ""

# Function to get position from various sources
get_positions() {
    echo "--- Position Report ---"
    
    # xdotool position
    XDOTOOL_POS=$(xdotool getmouselocation --shell 2>/dev/null | grep -E "^[XY]=" | tr '\n' ' ')
    XDOTOOL_X=$(echo "$XDOTOOL_POS" | sed 's/.*X=\([0-9]*\).*/\1/')
    XDOTOOL_Y=$(echo "$XDOTOOL_POS" | sed 's/.*Y=\([0-9]*\).*/\1/')
    echo "xdotool reports:     X=$XDOTOOL_X  Y=$XDOTOOL_Y"
    
    # KWin script position (Wayland)
    if [ "$XDG_SESSION_TYPE" = "wayland" ]; then
        MARKER="CALIB_$$"
        cat > /tmp/calib_cursor.js << EOF
var pos = workspace.cursorPos;
print("$MARKER:" + pos.x + "," + pos.y);
EOF
        qdbus6 org.kde.KWin /Scripting org.kde.kwin.Scripting.loadScript /tmp/calib_cursor.js >/dev/null 2>&1
        qdbus6 org.kde.KWin /Scripting org.kde.kwin.Scripting.start >/dev/null 2>&1
        sleep 0.2
        KWIN_POS=$(journalctl --user -n 30 --since "5 seconds ago" --no-pager -o cat 2>/dev/null | grep "$MARKER" | tail -1 | sed "s/.*$MARKER://" | tr -d ' ')
        if [ -n "$KWIN_POS" ]; then
            KWIN_X=$(echo "$KWIN_POS" | cut -d',' -f1)
            KWIN_Y=$(echo "$KWIN_POS" | cut -d',' -f2)
            echo "KWin reports:        X=$KWIN_X  Y=$KWIN_Y"
        else
            echo "KWin reports:        (failed to get position)"
        fi
    fi
    
    echo "------------------------"
}

# Store positions for analysis
declare -a XDOTOOL_POSITIONS
declare -a KWIN_POSITIONS
declare -a CORNER_NAMES

echo "Instructions:"
echo "1. Move your cursor to the specified corner"
echo "2. Keep it STILL and press Enter"
echo "3. Repeat for each corner"
echo ""
echo "Press Enter to begin..."
read

CORNERS=("TOP-LEFT of your LEFT monitor" "TOP-RIGHT of your LEFT monitor" "BOTTOM-LEFT of your LEFT monitor" "TOP-LEFT of your PRIMARY/CENTER monitor" "CENTER of your PRIMARY monitor" "TOP-LEFT of your RIGHT monitor" "BOTTOM-RIGHT of your RIGHT monitor")

for i in "${!CORNERS[@]}"; do
    echo ""
    echo "[$((i+1))/${#CORNERS[@]}] Move cursor to: ${CORNERS[$i]}"
    echo "Press Enter when cursor is in position..."
    read
    
    echo "Reading position..."
    get_positions
    
    # Store for later
    CORNER_NAMES[$i]="${CORNERS[$i]}"
done

echo ""
echo "=== CALIBRATION COMPLETE ==="
echo ""
echo "Now starting RazerLinux to test autoscroll..."
echo "Move your cursor somewhere and middle-click to see if the overlay appears at the right spot."
echo ""
echo "Press Enter to start RazerLinux..."
read

RUST_LOG=razerlinux::remap=info /opt/razerlinux/razerlinux 2>&1 | grep -E "Initial|KWin|Show at" &
RLIN_PID=$!

echo "RazerLinux started (PID: $RLIN_PID)"
echo "Middle-click to test. Press Enter here when done to stop."
read

kill $RLIN_PID 2>/dev/null
echo "Done!"
