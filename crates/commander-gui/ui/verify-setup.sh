#!/bin/bash

# AI Commander GUI - Setup Verification Script
# Verifies that the Svelte frontend is properly configured

set -e

echo "================================"
echo "AI Commander GUI - Setup Verification"
echo "================================"
echo ""

# Colors
GREEN='\033[0;32m'
RED='\033[0;31m'
YELLOW='\033[1;33m'
NC='\033[0m' # No Color

# Check Node.js
echo -n "Checking Node.js... "
if command -v node &> /dev/null; then
    NODE_VERSION=$(node --version)
    echo -e "${GREEN}✓${NC} $NODE_VERSION"
else
    echo -e "${RED}✗ Node.js not found${NC}"
    exit 1
fi

# Check npm
echo -n "Checking npm... "
if command -v npm &> /dev/null; then
    NPM_VERSION=$(npm --version)
    echo -e "${GREEN}✓${NC} v$NPM_VERSION"
else
    echo -e "${RED}✗ npm not found${NC}"
    exit 1
fi

# Check if node_modules exists
echo -n "Checking node_modules... "
if [ -d "node_modules" ]; then
    echo -e "${GREEN}✓${NC} Installed"
else
    echo -e "${YELLOW}⚠ Not installed${NC}"
    echo "  Run: npm install"
fi

# Verify key files exist
echo ""
echo "Verifying file structure..."

check_file() {
    if [ -f "$1" ]; then
        echo -e "  ${GREEN}✓${NC} $1"
    else
        echo -e "  ${RED}✗${NC} $1 (missing)"
        return 1
    fi
}

check_file "package.json"
check_file "vite.config.ts"
check_file "tsconfig.json"
check_file "tailwind.config.js"
check_file "index.html"
check_file "src/main.ts"
check_file "src/App.svelte"
check_file "src/lib/stores/app.ts"
check_file "src/lib/components/SessionList.svelte"
check_file "src/lib/components/ChatView.svelte"
check_file "src/lib/components/InputArea.svelte"
check_file "src/lib/components/BotStatus.svelte"

# Count components
echo ""
echo -n "Counting components... "
COMPONENT_COUNT=$(find src/lib/components -name "*.svelte" 2>/dev/null | wc -l | tr -d ' ')
if [ "$COMPONENT_COUNT" -eq 4 ]; then
    echo -e "${GREEN}✓${NC} $COMPONENT_COUNT/4"
else
    echo -e "${RED}✗${NC} $COMPONENT_COUNT/4 (expected 4)"
fi

# Try to build (if node_modules exists)
if [ -d "node_modules" ]; then
    echo ""
    echo "Testing build process..."
    if npm run build > /tmp/gui-build.log 2>&1; then
        echo -e "  ${GREEN}✓${NC} Build successful"

        # Check dist folder
        if [ -d "dist" ]; then
            echo -e "  ${GREEN}✓${NC} dist/ folder created"

            # Check bundle sizes
            if [ -f "dist/index.html" ]; then
                HTML_SIZE=$(wc -c < "dist/index.html" | tr -d ' ')
                echo -e "  ${GREEN}✓${NC} index.html: $HTML_SIZE bytes"
            fi

            JS_FILE=$(find dist/assets -name "*.js" 2>/dev/null | head -1)
            if [ -n "$JS_FILE" ]; then
                JS_SIZE=$(wc -c < "$JS_FILE" | tr -d ' ')
                echo -e "  ${GREEN}✓${NC} JavaScript: $JS_SIZE bytes"
            fi

            CSS_FILE=$(find dist/assets -name "*.css" 2>/dev/null | head -1)
            if [ -n "$CSS_FILE" ]; then
                CSS_SIZE=$(wc -c < "$CSS_FILE" | tr -d ' ')
                echo -e "  ${GREEN}✓${NC} CSS: $CSS_SIZE bytes"
            fi
        fi
    else
        echo -e "  ${RED}✗${NC} Build failed"
        echo "  Check /tmp/gui-build.log for details"
        exit 1
    fi
else
    echo ""
    echo -e "${YELLOW}⚠ Skipping build test (node_modules not installed)${NC}"
fi

# Final summary
echo ""
echo "================================"
echo -e "${GREEN}✓ Verification Complete${NC}"
echo "================================"
echo ""
echo "Next steps:"
echo "  1. npm install          # Install dependencies"
echo "  2. npm run dev          # Start dev server"
echo "  3. npm run build        # Production build"
echo ""
echo "For full integration:"
echo "  cd .. && cargo tauri dev"
echo ""
