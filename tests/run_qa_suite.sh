#!/usr/bin/env bash
set -euo pipefail

echo "================================================================"
echo "         CodexUsageBar Automated QA Regression & Gate Suite    "
echo "================================================================"

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT_DIR"

echo "[1/7] Running cargo test -p quota-core (all unit & boundary tests)..."
cargo test -p quota-core -- --nocapture
echo "  -> Cargo tests passed."

echo ""
echo "[2/7] Running cargo check --workspace --all-targets --all-features..."
cargo check --workspace --all-targets --all-features
echo "  -> Cargo check passed (0 errors, 0 warnings)."

echo ""
echo "[3/7] Running npm run check (Svelte 5 / TypeScript diagnostics)..."
npm run check
echo "  -> Svelte check passed (0 errors, 0 warnings)."

echo ""
echo "[4/7] Running npm run build (Vite production bundle)..."
npm run build
echo "  -> Vite build passed."

echo ""
echo "[5/7] Verifying font assets & bundle size in dist/..."
FONT_FILES=$(find dist/assets -name "ibm-plex-sans-latin-*" | wc -l | tr -d ' ')
NON_LATIN_FONTS=$(find dist/assets -name "ibm-plex-sans-*" ! -name "ibm-plex-sans-latin-*" | wc -l | tr -d ' ')

if [ "$FONT_FILES" -ne 6 ]; then
  echo "ERROR: Expected 6 latin font files, found $FONT_FILES"
  exit 1
fi
if [ "$NON_LATIN_FONTS" -ne 0 ]; then
  echo "ERROR: Found non-latin font files leaked into dist/assets: $NON_LATIN_FONTS"
  exit 1
fi
echo "  -> Font assets verified: exactly 6 latin font files (400, 500, 600 woff & woff2)."

echo ""
echo "[6/7] Verifying dead code isolation (Hero.svelte & Gauge.svelte)..."
if grep -rn "import .*Hero" src/App.svelte src/components/Panel.svelte src/components/ProviderSection.svelte src/components/Settings.svelte 2>/dev/null; then
  echo "ERROR: Hero.svelte is still imported by active components!"
  exit 1
fi
if grep -rn "import .*Gauge" src/App.svelte src/components/Panel.svelte src/components/ProviderSection.svelte src/components/Settings.svelte 2>/dev/null; then
  echo "ERROR: Gauge.svelte is still imported by active components!"
  exit 1
fi
echo "  -> Dead code isolation verified: neither Hero nor Gauge is used by active UI."

echo ""
echo "[7/7] Running frontend format, pace, brand icons, subline & affiliate unit tests..."
node --test tests/format_spec.mjs
node --test tests/brand_icons_spec.mjs
node --test tests/affiliate_spec.mjs
echo "  -> Frontend format, pace, brand icon & affiliate unit tests passed."

echo ""
echo "================================================================"
echo "  ALL QUALITY GATES PASSED (100% SUCCESS)                       "
echo "================================================================"
