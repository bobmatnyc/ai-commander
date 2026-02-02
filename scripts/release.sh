#!/bin/bash
# AI Commander Release Script
# Handles semantic versioning, build tracking, and homebrew publishing

set -e

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_ROOT="$(dirname "$SCRIPT_DIR")"
CARGO_TOML="$PROJECT_ROOT/Cargo.toml"
HOMEBREW_TAP="bobmatnyc/homebrew-tools"
FORMULA_PATH="Formula/ai-commander.rb"

# Colors
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
BLUE='\033[0;34m'
NC='\033[0m' # No Color

# Print with color
info() { echo -e "${BLUE}ℹ${NC} $1"; }
success() { echo -e "${GREEN}✓${NC} $1"; }
warn() { echo -e "${YELLOW}⚠${NC} $1"; }
error() { echo -e "${RED}✗${NC} $1"; exit 1; }

# Get current version from Cargo.toml
get_version() {
    grep '^version = ' "$CARGO_TOML" | head -1 | sed 's/version = "\(.*\)"/\1/'
}

# Bump version
bump_version() {
    local current=$1
    local type=$2

    IFS='.' read -r major minor patch <<< "$current"

    case $type in
        major)
            major=$((major + 1))
            minor=0
            patch=0
            ;;
        minor)
            minor=$((minor + 1))
            patch=0
            ;;
        patch)
            patch=$((patch + 1))
            ;;
        *)
            error "Invalid bump type: $type (use major, minor, or patch)"
            ;;
    esac

    echo "$major.$minor.$patch"
}

# Update version in Cargo.toml
update_version() {
    local new_version=$1
    sed -i.bak "s/^version = \".*\"/version = \"$new_version\"/" "$CARGO_TOML"
    rm -f "$CARGO_TOML.bak"
}

# Build and test
build_and_test() {
    info "Building project..."
    cargo build --release

    info "Running tests..."
    cargo test --release

    success "Build and tests passed"
}

# Create git tag and push
create_release() {
    local version=$1

    info "Committing version bump..."
    git add "$CARGO_TOML"
    git commit -m "chore: bump version to $version"

    info "Creating tag v$version..."
    git tag -a "v$version" -m "Release v$version"

    info "Pushing to origin..."
    git push origin main --tags

    success "Release v$version pushed to GitHub"
}

# Update homebrew formula
update_homebrew() {
    local version=$1

    info "Getting tarball SHA256..."
    local sha256=$(curl -sL "https://github.com/bobmatnyc/ai-commander/archive/refs/tags/v$version.tar.gz" | shasum -a 256 | cut -d' ' -f1)

    if [ -z "$sha256" ]; then
        error "Failed to get SHA256 for tarball"
    fi

    info "Updating homebrew formula..."

    # Create updated formula content
    local formula_content="class AiCommander < Formula
  desc \"Multi-interface AI coding session manager - TUI, REPL, and Telegram\"
  homepage \"https://github.com/bobmatnyc/ai-commander\"
  url \"https://github.com/bobmatnyc/ai-commander/archive/refs/tags/v$version.tar.gz\"
  sha256 \"$sha256\"
  license \"MIT\"
  head \"https://github.com/bobmatnyc/ai-commander.git\", branch: \"main\"

  depends_on \"rust\" => :build

  def install
    system \"cargo\", \"install\", *std_cargo_args(path: \"crates/commander-cli\")

    # Also build the telegram bot binary
    system \"cargo\", \"build\", \"--release\", \"-p\", \"commander-telegram\"
    bin.install \"target/release/commander-telegram\"
  end

  def caveats
    <<~EOS
      To use the Telegram bot integration:
        1. Create a bot via @BotFather on Telegram
        2. Add to .env.local: TELEGRAM_BOT_TOKEN=your_token
        3. Run: commander tui
        4. Use /telegram to generate a pairing code

      For response summarization, add:
        OPENROUTER_API_KEY=your_key
    EOS
  end

  test do
    assert_match \"commander\", shell_output(\"#{bin}/commander --version\")
  end
end"

    # Update via GitHub API
    info "Pushing formula update to homebrew tap..."

    # Get current file SHA
    local file_sha=$(gh api "repos/$HOMEBREW_TAP/contents/$FORMULA_PATH" --jq '.sha' 2>/dev/null || echo "")

    if [ -n "$file_sha" ]; then
        # Update existing file
        echo "$formula_content" | gh api "repos/$HOMEBREW_TAP/contents/$FORMULA_PATH" \
            -X PUT \
            -f message="chore: update ai-commander to v$version" \
            -f content="$(echo "$formula_content" | base64)" \
            -f sha="$file_sha" \
            > /dev/null
    else
        # Create new file
        echo "$formula_content" | gh api "repos/$HOMEBREW_TAP/contents/$FORMULA_PATH" \
            -X PUT \
            -f message="feat: add ai-commander v$version" \
            -f content="$(echo "$formula_content" | base64)" \
            > /dev/null
    fi

    success "Homebrew formula updated to v$version"
}

# Generate changelog
generate_changelog() {
    local version=$1
    local prev_tag=$(git describe --tags --abbrev=0 HEAD^ 2>/dev/null || echo "")

    echo ""
    echo "## v$version"
    echo ""

    if [ -n "$prev_tag" ]; then
        echo "### Changes since $prev_tag"
        echo ""
        git log "$prev_tag"..HEAD --pretty=format:"- %s" --no-merges | grep -v "^- chore:" | head -20
    else
        echo "### Initial release"
    fi
    echo ""
}

# Main
main() {
    cd "$PROJECT_ROOT"

    echo ""
    echo "╔════════════════════════════════════════╗"
    echo "║     AI Commander Release Script        ║"
    echo "╚════════════════════════════════════════╝"
    echo ""

    local bump_type=${1:-patch}
    local current_version=$(get_version)
    local new_version=$(bump_version "$current_version" "$bump_type")

    info "Current version: $current_version"
    info "New version: $new_version ($bump_type bump)"
    echo ""

    # Confirm
    read -p "Proceed with release? (y/N) " -n 1 -r
    echo ""
    if [[ ! $REPLY =~ ^[Yy]$ ]]; then
        warn "Release cancelled"
        exit 0
    fi

    echo ""

    # Update version
    info "Updating version in Cargo.toml..."
    update_version "$new_version"
    success "Version updated to $new_version"

    # Build and test
    build_and_test

    # Create release
    create_release "$new_version"

    # Update homebrew
    update_homebrew "$new_version"

    # Show changelog
    echo ""
    echo "━━━ Release Notes ━━━"
    generate_changelog "$new_version"

    echo ""
    echo "━━━ Release Complete! ━━━"
    echo ""
    success "Version: v$new_version"
    success "GitHub: https://github.com/bobmatnyc/ai-commander/releases/tag/v$new_version"
    success "Homebrew: brew upgrade ai-commander"
    echo ""
}

# Show usage
usage() {
    echo "Usage: $0 [major|minor|patch]"
    echo ""
    echo "  major  - Breaking changes (1.0.0 -> 2.0.0)"
    echo "  minor  - New features (1.0.0 -> 1.1.0)"
    echo "  patch  - Bug fixes (1.0.0 -> 1.0.1) [default]"
    echo ""
    echo "Examples:"
    echo "  $0          # Patch release"
    echo "  $0 patch    # Patch release"
    echo "  $0 minor    # Minor release"
    echo "  $0 major    # Major release"
}

if [[ "$1" == "-h" || "$1" == "--help" ]]; then
    usage
    exit 0
fi

main "$@"
