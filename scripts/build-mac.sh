#!/bin/sh
set -eu

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
MAC="$ROOT/crates/fdrive-mac/macos"
DERIVED_DATA="$HOME/Library/Developer/Xcode/DerivedData/Filestash-fdrive"
APP="$DERIVED_DATA/Build/Products/Debug/Filestash.app"

pkill -x Filestash 2>/dev/null || true

xcodegen generate --spec "$MAC/project.yml" --project "$MAC"
xcodebuild -project "$MAC/Filestash.xcodeproj" -scheme Filestash \
    -destination 'platform=macOS' -derivedDataPath "$DERIVED_DATA" build

codesign --verify --deep --strict "$APP"
pluginkit -a "$APP/Contents/PlugIns/FilestashFileProvider.appex"
echo "App: $APP"
