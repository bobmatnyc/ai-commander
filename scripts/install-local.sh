#!/bin/bash
set -e

echo "=== Installing AI Commander Locally ==="
echo ""

PROJECT_ROOT="/Users/masa/Projects/ai-commander"
CLI_BINARY="$PROJECT_ROOT/target/release/ai-commander"
GUI_BINARY="$PROJECT_ROOT/target/release/commander-gui"

# Check binaries exist
if [ ! -f "$CLI_BINARY" ]; then
    echo "Error: CLI binary not found at $CLI_BINARY"
    echo "Run: cargo build --release -p ai-commander"
    exit 1
fi

if [ ! -f "$GUI_BINARY" ]; then
    echo "Error: GUI binary not found at $GUI_BINARY"
    echo "Run: cd crates/commander-gui && cargo tauri build"
    exit 1
fi

# Install CLI binary
echo "Installing CLI binary to /usr/local/bin/ai-commander..."
sudo cp "$CLI_BINARY" /usr/local/bin/ai-commander
sudo chmod +x /usr/local/bin/ai-commander

# Create Application bundle for GUI
echo "Creating AI Commander.app bundle..."
APP_NAME="AI Commander.app"
APP_DIR="$HOME/Applications/$APP_NAME"

# Remove existing app if present
rm -rf "$APP_DIR"

# Create app bundle structure
mkdir -p "$APP_DIR/Contents/MacOS"
mkdir -p "$APP_DIR/Contents/Resources"

# Copy binary
cp "$GUI_BINARY" "$APP_DIR/Contents/MacOS/AI Commander"
chmod +x "$APP_DIR/Contents/MacOS/AI Commander"

# Create Info.plist
cat > "$APP_DIR/Contents/Info.plist" <<'EOF'
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
    <key>CFBundleDevelopmentRegion</key>
    <string>en</string>
    <key>CFBundleDisplayName</key>
    <string>AI Commander</string>
    <key>CFBundleExecutable</key>
    <string>AI Commander</string>
    <key>CFBundleIdentifier</key>
    <string>com.ai-commander.gui</string>
    <key>CFBundleInfoDictionaryVersion</key>
    <string>6.0</string>
    <key>CFBundleName</key>
    <string>AI Commander</string>
    <key>CFBundlePackageType</key>
    <string>APPL</string>
    <key>CFBundleShortVersionString</key>
    <string>0.3.0</string>
    <key>CFBundleVersion</key>
    <string>1</string>
    <key>LSMinimumSystemVersion</key>
    <string>10.13</string>
    <key>NSHighResolutionCapable</key>
    <true/>
</dict>
</plist>
EOF

# Remove quarantine attribute
xattr -cr "$APP_DIR"

echo ""
echo "✓ Installed CLI: /usr/local/bin/ai-commander"
echo "✓ Installed GUI: $APP_DIR"
echo ""
echo "Test CLI with: ai-commander --version"
echo "Test GUI with: open -a 'AI Commander'"
echo ""
echo "Next steps:"
echo "1. Run: ./scripts/setup-telegram-service.sh (to enable Telegram bot auto-start)"
echo "2. Run: ./scripts/manage-services.sh status (to check service status)"
