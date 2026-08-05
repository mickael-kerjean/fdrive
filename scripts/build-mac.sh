#!/bin/sh
set -eu

ROOT="$(cd "$(dirname "$0")/.." && pwd)"

if [ ! -d "$ROOT/crates/fdrive-mac/macos/Filestash.xcodeproj" ]; then
    xcodegen generate --spec "$ROOT/crates/fdrive-mac/macos/project.yml" --project "$ROOT/crates/fdrive-mac/macos"
fi

xcodebuild -project "$ROOT/crates/fdrive-mac/macos/Filestash.xcodeproj"
