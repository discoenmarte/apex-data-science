#!/bin/sh

echo "========================================="
echo "VPN Cliente FortiClient"
echo "Conectando a $VPN_SERVER..."
echo "========================================="

exec openconnect \
  --protocol=fortinet \
  --user="$VPN_USER" \
  --servercert=$SERVER_CERT \
  --reconnect-timeout=60 \
  --no-http-keepalive \
  "$VPN_SERVER"
