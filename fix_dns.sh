#!/bin/bash
# Script to permanently fix dnsmasq upstream DNS resolution

set -e

CONFIG_FILE="/etc/dnsmasq.d/upstream.conf"

echo "Creating/updating $CONFIG_FILE..."
sudo tee "$CONFIG_FILE" > /dev/null << 'EOF'
# Permanent upstream forwarders - prevents REFUSED for non-joinzipper domains
# Fixes getaddrinfo EREFUSED on platform.claude.com, api.anthropic.com, etc.
server=8.8.8.8
server=1.1.1.1
EOF

echo "Restarting dnsmasq service..."
sudo systemctl restart dnsmasq

echo "✅ dnsmasq successfully configured and restarted!"
echo "Testing resolution of platform.claude.com via localhost (127.0.0.1)..."
python3 -c "import socket; print('Resolved platform.claude.com to:', socket.gethostbyname('platform.claude.com'))"
