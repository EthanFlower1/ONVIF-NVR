#!/bin/bash
set -e

# Export environment variables if not set
export RECORDINGS_PATH=${RECORDINGS_PATH:-/app/recordings}
export POSTGRES_HOST=${POSTGRES_HOST:-postgres}
export POSTGRES_PORT=${POSTGRES_PORT:-5432}
export POSTGRES_USER=${POSTGRES_USER:-postgres}
export POSTGRES_PASSWORD=${POSTGRES_PASSWORD:-postgres}
export POSTGRES_DB=${POSTGRES_DB:-g_streamer}
export RUST_SERVER_PORT=${RUST_SERVER_PORT:-4750}
export NGINX_PORT=${NGINX_PORT:-8080}
export API_ADDRESS=${API_ADDRESS:-0.0.0.0}
export LOG_LEVEL=${LOG_LEVEL:-info}

# Create necessary directories if they don't exist
mkdir -p $RECORDINGS_PATH /app/public/hls /var/log/nginx /var/cache/nginx/data /var/cache/nginx/temp

# Organize recordings for NGINX VOD access
echo "Organizing recordings for NGINX VOD access..."
/app/scripts/organize_recordings.sh -d $RECORDINGS_PATH

# Start NGINX with VOD module
echo "Starting NGINX with VOD module..."
/usr/local/nginx/sbin/nginx

# Wait for NGINX to start
sleep 2

# Check if NGINX is running
if ! pgrep -x "nginx" > /dev/null; then
    echo "ERROR: NGINX failed to start. Check logs at /var/log/nginx/error.log"
    exit 1
fi

echo "NGINX started successfully. Listening on port ${NGINX_PORT}"

# Start the Rust server
echo "Starting G-Streamer Rust server..."
cd /app
exec ./g-streamer