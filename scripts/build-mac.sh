#!/bin/sh
set -eu

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
MAC="$ROOT/crates/fdrive-mac/macos"
HEADERS="$ROOT/target/macos-headers"
LIBRARY="$ROOT/target/aarch64-apple-darwin/release/libfdrive_mac.a"
DERIVED_DATA="$HOME/Library/Developer/Xcode/DerivedData/Filestash-fdrive"
APP="$DERIVED_DATA/Build/Products/Debug/Filestash.app"

pkill -x Filestash 2>/dev/null || true

cargo build -p fdrive-mac --release --target aarch64-apple-darwin
cargo run -p fdrive-mac --bin uniffi-bindgen-swift -- generate --library "$LIBRARY" --language swift --no-format --out-dir "$MAC/Generated"

rm -rf "$HEADERS" "$MAC/Fdrive.xcframework"
mkdir -p "$HEADERS"
cp "$MAC/Generated/fdriveFFI.h" "$HEADERS/"
cp "$MAC/Generated/fdriveFFI.modulemap" "$HEADERS/module.modulemap"
xcodebuild -create-xcframework -library "$LIBRARY" -headers "$HEADERS" -output "$MAC/Fdrive.xcframework"

xcodegen generate --spec "$MAC/project.yml" --project "$MAC"
xcodebuild -project "$MAC/Filestash.xcodeproj" -scheme Filestash -destination 'platform=macOS' -derivedDataPath "$DERIVED_DATA" build

codesign --verify --deep --strict "$APP"
pluginkit -a "$APP/Contents/PlugIns/FilestashFileProvider.appex"
open -n "$APP"
