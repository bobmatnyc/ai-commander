#!/bin/bash
set -e

echo "=== Installing AI Commander (User-Only, No Sudo) ==="
echo ""

PROJECT_ROOT="/Users/masa/Projects/ai-commander"
CLI_BINARY="$PROJECT_ROOT/target/release/ai-commander"
GUI_BINARY="$PROJECT_ROOT/target/release/commander-gui"
USER_BIN="$HOME/.local/bin"

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

# Create user bin directory
mkdir -p "$USER_BIN"

# Install CLI binary
echo "Installing CLI binary to $USER_BIN/ai-commander..."
cp "$CLI_BINARY" "$USER_BIN/ai-commander"
chmod +x "$USER_BIN/ai-commander"

# Check if ~/.local/bin is in PATH
if [[ ":$PATH:" != *":$USER_BIN:"* ]]; then
    echo ""
    echo "⚠️  $USER_BIN is not in your PATH"
    echo ""
    echo "Add this to your ~/.zshrc or ~/.bashrc:"
    echo "    export PATH=\"\$HOME/.local/bin:\$PATH\""
    echo ""
    echo "Then reload: source ~/.zshrc"
fi

# Create Application bundle for GUI
echo "Creating AI Commander.app bundle..."
APP_NAME="AI Commander.app"
APP_DIR="$HOME/Applications/$APP_NAME"

# Remove existing app if present
rm -rf "$APP_DIR"

# Create Applications directory
mkdir -p "$HOME/Applications"

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
echo "✓ Installed CLI: $USER_BIN/ai-commander"
echo "✓ Installed GUI: $APP_DIR"
echo ""

# Update service setup script to use user bin
SERVICE_SCRIPT="$PROJECT_ROOT/scripts/setup-telegram-service.sh"
if [ -f "$SERVICE_SCRIPT" ]; then
    echo "Updating service script to use $USER_BIN..."
    sed -i.bak "s|/usr/local/bin/ai-commander|$USER_BIN/ai-commander|g" "$SERVICE_SCRIPT"
fi

echo "Test CLI with:"
echo "  $USER_BIN/ai-commander --version"
echo ""
echo "Test GUI with:"
echo "  open -a 'AI Commander'"
echo ""
echo "Next steps:"
echo "1. If PATH warning shown above, add ~/.local/bin to PATH"
echo "2. Run: ./scripts/setup-telegram-service.sh (to enable Telegram bot auto-start)"
echo "3. Run: ./scripts/manage-services.sh status (to check service status)"
