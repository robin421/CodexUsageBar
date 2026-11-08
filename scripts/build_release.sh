#!/usr/bin/env bash
set -e

export COPYFILE_DISABLE=1

DIR="$( cd "$( dirname "${BASH_SOURCE[0]}" )/.." && pwd )"
cd "$DIR"

echo "==> 1. Generating Xcode project with XcodeGen..."
xcodegen generate

# Auto-detect Apple Developer signing identity
SIGN_IDENTITY=$(security find-identity -v -p codesigning | grep -o 'Apple Development: [^"]*' | head -n 1 || true)
if [ -z "$SIGN_IDENTITY" ]; then
    SIGN_IDENTITY="-"
    echo "==> No Apple Developer certificate found, fallback to ad-hoc (-)"
else
    echo "==> Found Apple Developer Identity: \"$SIGN_IDENTITY\""
fi

# Use internal APFS directory for build output
BUILD_DIR="/tmp/CodexUsageBar-Build"
OUTPUT_DIR="$DIR/dist"
mkdir -p "$BUILD_DIR"
mkdir -p "$OUTPUT_DIR"

echo "==> 2. Building Release configuration..."
xcodebuild -project CodexUsageBar.xcodeproj \
           -scheme CodexUsageBar \
           -configuration Release \
           -derivedDataPath "$BUILD_DIR/DerivedData" \
           build

APP_PATH=$(find "$BUILD_DIR/DerivedData/Build/Products/Release" -name "CodexUsageBar.app" -type d | head -n 1)

if [ -z "$APP_PATH" ] || [ ! -d "$APP_PATH" ]; then
    echo "Error: CodexUsageBar.app was not found in build products!"
    exit 1
fi

echo "==> Found App at: $APP_PATH"

echo "==> 3. Signing App with developer certificate..."
codesign --force --deep --sign "$SIGN_IDENTITY" -o runtime "$APP_PATH"

echo "==> 4. Creating DMG distribution package..."
DMG_PATH="$OUTPUT_DIR/CodexUsageBar.dmg"
rm -f "$DMG_PATH"

hdiutil create -volname "CodexUsageBar" \
               -srcfolder "$APP_PATH" \
               -ov \
               -format UDZO \
               "$DMG_PATH"

echo "==> 5. Packaging ZIP distribution..."
ZIP_PATH="$OUTPUT_DIR/CodexUsageBar-macOS.zip"
rm -f "$ZIP_PATH"
(cd "$(dirname "$APP_PATH")" && zip -r -q "$ZIP_PATH" "CodexUsageBar.app")

echo "==> 6. Copying .app bundle to dist folder..."
rm -rf "$OUTPUT_DIR/CodexUsageBar.app"
ditto --noextattr --norsrc "$APP_PATH" "$OUTPUT_DIR/CodexUsageBar.app"

echo "==> 7. Installing to /Applications..."
killall CodexUsageBar 2>/dev/null || true
rm -rf "/Applications/CodexUsageBar.app"
ditto --noextattr --norsrc "$APP_PATH" "/Applications/CodexUsageBar.app"

echo ""
echo "=================================================="
echo "  Build, Signing & Installation Succeeded!"
echo "  - Signed with: $SIGN_IDENTITY"
echo "  - Installed at: /Applications/CodexUsageBar.app"
echo "  - DMG Package: $OUTPUT_DIR/CodexUsageBar.dmg"
echo "  - ZIP Archive: $OUTPUT_DIR/CodexUsageBar-macOS.zip"
echo "=================================================="
