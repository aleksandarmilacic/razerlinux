#!/bin/bash
# RazerLinux Uninstaller

set -e

echo "=== RazerLinux Uninstaller ==="
echo ""

if [ "$EUID" -ne 0 ]; then
    echo "Please run as root: sudo ./uninstall.sh"
    exit 1
fi

echo "Removing RazerLinux..."

# Stop and disable systemd service for all users
for home_dir in /home/*; do
    if [ -d "$home_dir" ]; then
        username=$(basename "$home_dir")
        # Try to disable the service (ignore errors if not enabled)
        sudo -u "$username" systemctl --user stop razerlinux.service 2>/dev/null || true
        sudo -u "$username" systemctl --user disable razerlinux.service 2>/dev/null || true
    fi
done

# Remove files
rm -f /usr/local/bin/razerlinux
rm -f /usr/share/applications/razerlinux.desktop
rm -f /etc/udev/rules.d/99-razerlinux.rules
rm -f /usr/share/polkit-1/actions/org.razerlinux.policy
rm -f /usr/share/icons/hicolor/scalable/apps/razerlinux.svg
rm -f /usr/lib/systemd/user/razerlinux.service
rm -rf /opt/razerlinux

# Remove autostart entries (for all users)
for home_dir in /home/*; do
    if [ -d "$home_dir" ]; then
        rm -f "$home_dir/.config/autostart/razerlinux.desktop"
    fi
done

# Reload udev
udevadm control --reload-rules

echo ""
echo "RazerLinux has been uninstalled."
echo ""
echo "Note: User configuration in ~/.config/razerlinux/ was preserved."
echo "To remove it: rm -rf ~/.config/razerlinux"
