#!/bin/sh
set -eu

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
MAC="$ROOT/crates/fdrive-mac/macos"
HEADERS="$ROOT/target/macos-headers"
LIBRARY="$ROOT/target/aarch64-apple-darwin/release/libfdrive_mac.a"

CONFIG=Debug
if [ "${1:-}" = "--release" ]; then
    CONFIG=Release
fi
DERIVED_DATA="$HOME/Library/Developer/Xcode/DerivedData/Filestash-fdrive"
if [ "$CONFIG" = Release ]; then
    DERIVED_DATA="$DERIVED_DATA-release"
fi
APP="$DERIVED_DATA/Build/Products/$CONFIG/Filestash.app"

if [ "$CONFIG" = Debug ]; then
    pkill -x Filestash 2>/dev/null || true
fi

cargo build -p fdrive-mac --release --target aarch64-apple-darwin
cargo run -p fdrive-mac --bin uniffi-bindgen-swift -- generate --library "$LIBRARY" --language swift --no-format --out-dir "$MAC/Generated"

rm -rf "$HEADERS" "$MAC/Fdrive.xcframework"
mkdir -p "$HEADERS"
cp "$MAC/Generated/fdriveFFI.h" "$HEADERS/"
cp "$MAC/Generated/fdriveFFI.modulemap" "$HEADERS/module.modulemap"
xcodebuild -create-xcframework -library "$LIBRARY" -headers "$HEADERS" -output "$MAC/Fdrive.xcframework"

xcodegen generate --spec "$MAC/project.yml" --project "$MAC"
if [ "$CONFIG" = Release ]; then
    rm -rf "$APP"
fi
xcodebuild -project "$MAC/Filestash.xcodeproj" -scheme Filestash -destination 'platform=macOS' -derivedDataPath "$DERIVED_DATA" -configuration "$CONFIG" build

if [ "$CONFIG" = Debug ]; then
    codesign --verify --deep --strict "$APP"
    pluginkit -a "$APP/Contents/PlugIns/FilestashFileProvider.appex"
    open -n "$APP"
    exit 0
fi

OUT="$ROOT/target/mac-release"
STAGE="$OUT/dmg/Filestash.app"
IDENTITY="Developer ID Application: Mickael KERJEAN (3736F8X9F9)"
PROFILE="filestash"

rm -rf "$OUT"
mkdir -p "$OUT/dmg"
ditto "$APP" "$STAGE"
xattr -cr "$STAGE"
ln -s /Applications "$OUT/dmg/Applications"

for bundle in "$STAGE/Contents/PlugIns/FilestashFileProvider.appex" "$STAGE"; do
    ENT=/tmp/fdrive-entitlements.plist
    codesign -d --entitlements - --xml "$bundle" > "$ENT"
    plutil -remove 'com\.apple\.security\.get-task-allow' "$ENT" 2>/dev/null || true
    plutil -insert 'com\.apple\.security\.application-identifier' -string "3736F8X9F9.$(plutil -extract CFBundleIdentifier raw "$bundle/Contents/Info.plist")" "$ENT"
    codesign --force --options runtime --timestamp --sign "$IDENTITY" --entitlements "$ENT" "$bundle"
done
codesign --verify --deep --strict "$STAGE"

hdiutil create -volname Filestash -srcfolder "$OUT/dmg" -format UDZO -ov "$OUT/Filestash.dmg"
codesign --sign "$IDENTITY" "$OUT/Filestash.dmg"
xcrun notarytool submit "$OUT/Filestash.dmg" --keychain-profile "$PROFILE" --wait
xcrun stapler staple "$OUT/Filestash.dmg"
rm -rf "$OUT/dmg"
echo "release artifact: $OUT/Filestash.dmg"
