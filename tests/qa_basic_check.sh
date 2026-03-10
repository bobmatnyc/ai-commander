#!/bin/bash
# Basic UI Verification Check for AI Commander GUI
# Tests that the development server is running and serving content

echo "=================================="
echo "AI Commander GUI - Basic QA Check"
echo "=================================="
echo ""

# Check if Vite server is running
echo "Checking Vite dev server..."
if curl -s http://localhost:5173/ | grep -q "AI Commander"; then
    echo "✓ Vite server is running and serving index.html"
else
    echo "✗ Vite server not responding or content incorrect"
    exit 1
fi

# Check if main CSS is being loaded
echo ""
echo "Checking asset loading..."
if curl -s http://localhost:5173/src/main.ts | grep -q "App"; then
    echo "✓ Main TypeScript entry point accessible"
else
    echo "⚠ Main TS file not accessible (may be normal in production build)"
fi

# Check if App.svelte is accessible via Vite HMR
echo ""
echo "Checking Svelte components..."
if curl -s http://localhost:5173/src/App.svelte 2>&1 | grep -q "SessionList\|ChatView\|InputArea\|BotStatus"; then
    echo "✓ App.svelte is accessible and contains expected imports"
else
    echo "⚠ App.svelte not directly accessible (may be bundled)"
fi

echo ""
echo "=================================="
echo "Basic checks complete!"
echo ""
echo "Next steps for full QA:"
echo "1. Open browser to: http://localhost:5173/"
echo "2. Inspect UI components visually"
echo "3. Run full Tauri app: cd crates/commander-gui && cargo tauri dev"
echo "4. Follow manual testing checklist in QA_TESTING_REPORT.md"
echo "=================================="
