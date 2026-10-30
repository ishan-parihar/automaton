#!/usr/bin/env bash
set -euo pipefail

# Automaton — Install Script
# Downloads the latest release binary and sets up session hooks.

REPO="ishan-parihar/automaton"
INSTALL_DIR="${AUTOMATON_INSTALL_DIR:-$HOME/.local/bin}"
SKIP_CTA="${SKIP_CTA:-0}"
[ "${1:-}" = "--no-cta" ] && SKIP_CTA=1

print_cta() {
  [ "$SKIP_CTA" = "1" ] && return 0
  printf '\n%s\n' "──────────────────────────────────────────────────────"
  printf ' ⭐  Found automaton useful? Star it:  https://github.com/ishan-parihar/automaton\n'
  printf ' 💜  Support development:\n'
  printf '     GitHub Sponsors : https://github.com/sponsors/ishan-parihar\n'
  printf '     Razorpay (UPI)  : https://rzp.io/rzp/ishan-parihar\n'
  printf '     Or run `automaton donate` anytime.\n'
  printf '%s\n' "──────────────────────────────────────────────────────"
}

ensure_path() {  # idempotent: grep before append
  local dir="$1" rc="$HOME/.bashrc"
  mkdir -p "$dir"
  case ":$PATH:" in *":$dir:"*) return 0 ;; esac
  if [ -f "$HOME/.zshrc" ] || [ -n "${ZSH_VERSION:-}" ]; then rc="$HOME/.zshrc"; fi
  if ! grep -qs "export PATH=\"$dir:\$PATH\"" "$rc"; then
    printf '\n# added by automaton installer\nexport PATH="%s:$PATH"\n' "$dir" >> "$rc"
    echo ">>> generated: $rc (PATH export) <<<"
  fi
  export PATH="$dir:$PATH"
}

echo "=== Automaton Installer ==="
echo ""

# Resolve the exact release asset published by .github/workflows/release.yml.
ARCH=$(uname -m)
OS=$(uname -s)
case "$OS" in
    Linux)
        case "$ARCH" in
            x86_64|amd64) ASSET="automaton-linux-x64" ;;
            *) echo "Error: Automaton releases do not include Linux/$ARCH." >&2; exit 1 ;;
        esac
        ;;
    Darwin)
        case "$ARCH" in
            aarch64|arm64) ASSET="automaton-macos-arm64" ;;
            *) echo "Error: Automaton releases do not include macOS/$ARCH." >&2; exit 1 ;;
        esac
        ;;
    *)      echo "Error: Unsupported OS: $OS"; exit 1 ;;
esac

echo "Platform: $OS/$ARCH ($ASSET)"
echo "Install dir: $INSTALL_DIR"
echo ""

# Get latest release
echo "Fetching latest release..."
LATEST=$(curl -fsSL "https://api.github.com/repos/$REPO/releases/latest" | grep '"tag_name"' | sed -E 's/.*"([^"]+)".*/\1/')
if [ -z "$LATEST" ]; then
    echo "Error: Could not fetch latest release for $REPO (network or API issue)" >&2
    exit 1
fi
echo "Latest version: $LATEST"

# Download the exact binary and the checksum published by the same tagged build.
BINARY_URL="https://github.com/$REPO/releases/download/${LATEST}/${ASSET}"
SUMS_URL="https://github.com/$REPO/releases/download/${LATEST}/SHA256SUMS"
TMPDIR=$(mktemp -d)
trap 'rm -rf "$TMPDIR"' EXIT

echo "Downloading..."
curl --fail --silent --show-error --location --max-time 180 -o "$TMPDIR/$ASSET" "$BINARY_URL" \
    || { echo "Error: download failed: $BINARY_URL" >&2; exit 1; }
curl --fail --silent --show-error --location --max-time 30 -o "$TMPDIR/SHA256SUMS" "$SUMS_URL" \
    || { echo "Error: release has no SHA256SUMS; refusing unverified binary" >&2; exit 1; }
EXPECTED=$(awk -v f="$ASSET" '$2 == f || $2 == "*"f { print $1; exit }' "$TMPDIR/SHA256SUMS")
[ -n "$EXPECTED" ] || { echo "Error: SHA256SUMS has no entry for $ASSET" >&2; exit 1; }
if command -v sha256sum >/dev/null 2>&1; then
    ACTUAL=$(sha256sum "$TMPDIR/$ASSET" | awk '{print $1}')
elif command -v shasum >/dev/null 2>&1; then
    ACTUAL=$(shasum -a 256 "$TMPDIR/$ASSET" | awk '{print $1}')
else
    echo "Error: install sha256sum or shasum to verify the release binary" >&2
    exit 1
fi
[ "$ACTUAL" = "$EXPECTED" ] || { echo "Error: SHA-256 mismatch for $ASSET" >&2; exit 1; }
[ -s "$TMPDIR/$ASSET" ] || { echo "Error: downloaded file is empty" >&2; exit 1; }

# Install binary
mkdir -p "$INSTALL_DIR"
install -m 0755 "$TMPDIR/$ASSET" "$INSTALL_DIR/automaton" \
    || { echo "Error: install to $INSTALL_DIR failed" >&2; exit 1; }
cat > "$INSTALL_DIR/automaton.install-origin.json.tmp" <<EOF
{"schema":1,"application":"automaton","source":"release-binary","channel":"stable","target":"$ASSET","version":"$LATEST","update_origin":"https://api.github.com/repos/$REPO"}
EOF
chmod 0644 "$INSTALL_DIR/automaton.install-origin.json.tmp"
mv -f "$INSTALL_DIR/automaton.install-origin.json.tmp" "$INSTALL_DIR/automaton.install-origin.json"
ensure_path "$INSTALL_DIR"

echo "Installed: $INSTALL_DIR/automaton"

# Verify the binary actually runs before claiming success
case ":$PATH:" in *":$INSTALL_DIR:"*) ;; *) export PATH="$INSTALL_DIR:$PATH" ;; esac
"$INSTALL_DIR/automaton" --version >/dev/null 2>&1 \
    || { echo "Error: installed binary does not run: $INSTALL_DIR/automaton" >&2; exit 1; }
echo "Verified: $INSTALL_DIR/automaton ($LATEST)"
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
echo "  automaton update --check                # Check for a verified release update"
echo "  automaton update                        # Install the latest stable release"
echo "  automaton --help                        # Show all commands"
echo ""

print_cta
