#!/bin/sh
set -eu

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
IOS="$ROOT/crates/fdrive-ios/ios"
HEADERS="$ROOT/target/ios-headers"

rustup target add aarch64-apple-ios aarch64-apple-ios-sim
PATH="$(dirname "$(rustup which rustc)"):$PATH"

cargo build -p fdrive-ios --release --target aarch64-apple-ios
cargo build -p fdrive-ios --release --target aarch64-apple-ios-sim

cargo run -p fdrive-ios --bin uniffi-bindgen-swift -- \
    generate --library "$ROOT/target/aarch64-apple-ios/release/libfdrive_ios.a" \
    --language swift --no-format --out-dir "$IOS/Generated"

rm -rf "$HEADERS" "$IOS/Fdrive.xcframework"
mkdir -p "$HEADERS"
cp "$IOS/Generated/fdriveFFI.h" "$HEADERS/"
cp "$IOS/Generated/fdriveFFI.modulemap" "$HEADERS/module.modulemap"
xcodebuild -create-xcframework \
    -library "$ROOT/target/aarch64-apple-ios/release/libfdrive_ios.a" -headers "$HEADERS" \
    -library "$ROOT/target/aarch64-apple-ios-sim/release/libfdrive_ios.a" -headers "$HEADERS" \
    -output "$IOS/Fdrive.xcframework"

xcodegen generate --spec "$IOS/project.yml" --project "$IOS"
DERIVED_DATA="$HOME/Library/Developer/Xcode/DerivedData/Filestash-fdrive-ios"
xcodebuild -project "$IOS/Filestash.xcodeproj" -scheme Filestash \
    -destination 'generic/platform=iOS Simulator' ARCHS=arm64 -derivedDataPath "$DERIVED_DATA" build

UDID="$(xcrun simctl list devices available | awk -F '[()]' '/iPhone/ && /Booted/ {print $2; exit}')"
if [ -z "$UDID" ]; then
    UDID="$(xcrun simctl list devices available | awk -F '[()]' '/iPhone/ {print $2; exit}')"
    xcrun simctl boot "$UDID"
fi
open -a Simulator
xcrun simctl install "$UDID" "$DERIVED_DATA/Build/Products/Debug-iphonesimulator/Filestash.app"
xcrun simctl launch "$UDID" app.filestash.ios
