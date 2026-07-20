#!/usr/bin/env bash
set -euo pipefail

# Automaton — Install Script
# Downloads the latest release binary and sets up session hooks.

REPO="ishan-parihar/automaton"
INSTALL_DIR="${AUTOMATON_INSTALL_DIR:-$HOME/.local/bin}"

echo "=== Automaton Installer ==="
echo ""

# Detect platform
ARCH=$(uname -m)
OS=$(uname -s)
case "$OS" in
    Linux)  PLATFORM="x86_64-unknown-linux-musl" ;;
    Darwin) PLATFORM="aarch64-apple-darwin" ;;
    *)      echo "Error: Unsupported OS: $OS"; exit 1 ;;
esac
case "$ARCH" in
    x86_64)  [ "$OS" = "Linux" ] && PLATFORM="x86_64-unknown-linux-musl" ;;
    aarch64|arm64)
        [ "$OS" = "Linux" ] && PLATFORM="aarch64-unknown-linux-musl"
        [ "$OS" = "Darwin" ] && PLATFORM="aarch64-apple-darwin"
        ;;
    *)       echo "Error: Unsupported architecture: $ARCH"; exit 1 ;;
esac

echo "Platform: $PLATFORM"
echo "Install dir: $INSTALL_DIR"
echo ""

# Get latest release
echo "Fetching latest release..."
LATEST=$(curl -s "https://api.github.com/repos/$REPO/releases/latest" | grep '"tag_name"' | sed -E 's/.*"([^"]+)".*/\1/')
if [ -z "$LATEST" ]; then
    echo "Error: Could not fetch latest release"
    exit 1
fi
echo "Latest version: $LATEST"

# Download
TARBALL="automaton-${LATEST#v}-${PLATFORM}.tar.gz"
DOWNLOAD_URL="https://github.com/$REPO/releases/download/${LATEST}/${TARBALL}"
TMPDIR=$(mktemp -d)
trap "rm -rf $TMPDIR" EXIT

echo "Downloading..."
curl -L -o "$TMPDIR/$TARBALL" "$DOWNLOAD_URL" 2>/dev/null

# Extract
tar -xzf "$TMPDIR/$TARBALL" -C "$TMPDIR" 2>/dev/null || cp "$TMPDIR/automaton" "$TMPDIR/automaton_bin"

# Install binary
mkdir -p "$INSTALL_DIR"
cp "$TMPDIR/automaton" "$INSTALL_DIR/automaton" 2>/dev/null || cp "$TMPDIR/automaton_bin" "$INSTALL_DIR/automaton"
chmod +x "$INSTALL_DIR/automaton"

echo "Installed: $INSTALL_DIR/automaton"
echo ""

# ── Install Session Hooks (AXI §7) ─────────────────────────────────────────
echo "Installing AI agent session hooks..."

# Claude Code session hook
CLAUDE_SETTINGS="$HOME/.claude/settings.json"
if command -v jq &>/dev/null && [ -f "$CLAUDE_SETTINGS" ]; then
    if jq -e '.hooks.SessionStart[]?.hooks[]?.command == "automaton"' "$CLAUDE_SETTINGS" &>/dev/null; then
        echo "  ✓ Claude Code session hook already installed"
    else
        cp "$CLAUDE_SETTINGS" "${CLAUDE_SETTINGS}.bak.$(date +%s)"
        jq '.hooks.SessionStart += [{"matcher":"","hooks":[{"type":"command","command":"automaton"}]}]' \
            "$CLAUDE_SETTINGS" > "${CLAUDE_SETTINGS}.tmp" && mv "${CLAUDE_SETTINGS}.tmp" "$CLAUDE_SETTINGS"
        echo "  ✓ Claude Code session hook installed"
    fi
else
    echo "  → Claude Code: Add to ~/.claude/settings.json:"
    echo '    {"hooks":{"SessionStart":[{"matcher":"","hooks":[{"type":"command","command":"automaton"}]}]}}'
fi

# Codex session hook
CODEX_DIR="$HOME/.codex"
if [ -d "$CODEX_DIR" ]; then
    CODEX_HOOKS="$CODEX_DIR/hooks.json"
    if [ -f "$CODEX_HOOKS" ] && jq -e '.SessionStart == "automaton"' "$CODEX_HOOKS" &>/dev/null; then
        echo "  ✓ Codex session hook already installed"
    else
        if [ -f "$CODEX_HOOKS" ]; then
            cp "$CODEX_HOOKS" "${CODEX_HOOKS}.bak.$(date +%s)"
            jq '.SessionStart = "automaton"' "$CODEX_HOOKS" > "${CODEX_HOOKS}.tmp" && mv "${CODEX_HOOKS}.tmp" "$CODEX_HOOKS"
        else
            echo '{"SessionStart":"automaton"}' > "$CODEX_HOOKS"
        fi
        echo "  ✓ Codex session hook installed"
        CODEX_CONFIG="$CODEX_DIR/config.toml"
        if [ -f "$CODEX_CONFIG" ] && ! grep -q 'hooks = true' "$CODEX_CONFIG"; then
            echo -e '\n[features]\nhooks = true' >> "$CODEX_CONFIG"
            echo "  ✓ Enabled hooks in config.toml"
        fi
    fi
else
    echo "  → Codex: Create ~/.codex/hooks.json with {"SessionStart":"automaton"}"
fi

# OpenCode session hook
OPENCODE_DIR="$HOME/.config/opencode/plugins"
if [ -d "$HOME/.config/opencode" ]; then
    mkdir -p "$OPENCODE_DIR"
    if [ -f "$OPENCODE_DIR/automaton.ts" ]; then
        echo "  ✓ OpenCode session hook already installed"
    else
        cat > "$OPENCODE_DIR/automaton.ts" << 'OPENCODE_PLUGIN'
export default {
  name: "automaton",
  onSessionStart: async () => {
    const { execSync } = require("child_process");
    return execSync("automaton").toString();
  },
};
OPENCODE_PLUGIN
        echo "  ✓ OpenCode session hook installed"
    fi
else
    echo "  → OpenCode: Create ~/.config/opencode/plugins/automaton.ts (see README)"
fi

# Install SKILL.md
SKILL_DIR="$HOME/.agents/skills/automaton"
mkdir -p "$SKILL_DIR"
if [ ! -f "$SKILL_DIR/SKILL.md" ]; then
    curl -fsSL "https://raw.githubusercontent.com/ishan-parihar/automaton/master/SKILL.md" \
        -o "$SKILL_DIR/SKILL.md" 2>/dev/null && \
        echo "  ✓ Skill installed to $SKILL_DIR/SKILL.md" || \
        echo "  → Skill download failed (non-critical)"
else
    echo "  ✓ Skill already installed at $SKILL_DIR/SKILL.md"
fi

echo ""
echo "=== Installation Complete ==="
echo ""
echo "Quick start:"
echo "  automaton init                          # Initialize the substrate"
echo "  automaton mcp                           # Start MCP server"
echo "  automaton --help                        # Show all commands"
echo ""
