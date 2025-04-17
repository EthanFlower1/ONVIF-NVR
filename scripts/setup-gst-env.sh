#!/bin/bash

# Make sure we're using Homebrew's pkg-config
export PKG_CONFIG="/opt/homebrew/bin/pkg-config"
export PATH="/opt/homebrew/bin:$PATH"

# Set PKG_CONFIG_PATH to look for Homebrew packages
export PKG_CONFIG_PATH="/opt/homebrew/lib/pkgconfig"

# Set plugin path for GStreamer
export GST_PLUGIN_PATH="/opt/homebrew/lib/gstreamer-1.0"

# Library path for dynamic linking
export DYLD_LIBRARY_PATH="/opt/homebrew/lib:$DYLD_LIBRARY_PATH"

# Skip Python Plugin 
export GST_PLUGIN_FEATURE_RANK="python:0"

# Verify pkg-config can find the libraries
echo "Testing pkg-config..."
$PKG_CONFIG --exists --print-errors gstreamer-1.0
if [ $? -eq 0 ]; then
    echo "GStreamer found!"
else
    echo "GStreamer not found."
fi

# Print environment details
echo "PATH: $PATH"
echo "PKG_CONFIG_PATH: $PKG_CONFIG_PATH"
echo "PKG_CONFIG: $PKG_CONFIG"
