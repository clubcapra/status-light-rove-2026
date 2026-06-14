#!/usr/bin/env bash
set -euo pipefail

if [[ "$EUID" -eq 0 ]]; then
    echo "Cannot run this as root, run this script as a user (it uses sudo where needed)"
    exit 1
fi

SCRIPT_DIR="$(dirname "$(readlink -f "$0")")"
REPO_DIR="$(dirname "$SCRIPT_DIR")"

# 1. Build the tower-api binary (release) as the current user
echo "1/6 Build tower-api (release)"
( cd "$REPO_DIR" && cargo build --release )

# 2. Install the binary to /usr/local/bin
echo "2/6 Install binary to /usr/local/bin"
sudo install -m 0755 "$REPO_DIR/target/release/tower-api" /usr/local/bin/tower-api

# 3. Install the udev rule and reload so /dev/tower-light appears
echo "3/6 Install udev rule"
sudo cp "$SCRIPT_DIR/99-tower-light.rules" /etc/udev/rules.d/
sudo udevadm control --reload-rules
sudo udevadm trigger

# 4. Install the systemd service
echo "4/6 Install systemd service"
sudo cp "$SCRIPT_DIR/tower-api.service" /etc/systemd/system/

# 5. Reload the systemd daemon
echo "5/6 Reload systemd daemon"
sudo systemctl daemon-reload

# 6. Enable at boot and (re)start now
echo "6/6 Enable and start tower-api"
sudo systemctl enable tower-api.service
sudo systemctl restart tower-api.service

echo 'Done! Check status with: systemctl status tower-api  and  journalctl -u tower-api -f'
