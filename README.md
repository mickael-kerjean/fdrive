# Filestash Windows Cloud Sync Provider (C++)

Native C++ implementation using Windows Cloud Files API - no driver installation required.

## Quick Start

### Requirements
- Windows 10 version 1809+
- Visual Studio 2019+ with C++ Desktop Development workload
- vcpkg (for dependencies)

### Build

**Option 1: Automatic (recommended)**
```bash
build.bat
```

**Option 2: Manual**
```bash
# Install dependency
vcpkg install nlohmann-json:x64-windows

# Configure (CMake auto-detects Visual Studio)
mkdir build && cd build
cmake .. -A x64 -DCMAKE_TOOLCHAIN_FILE=%VCPKG_ROOT%\scripts\buildsystems\vcpkg.cmake

# Build
cmake --build . --config Release
```

**Option 3: Explicit VS version**
```bash
# For Visual Studio 2026 (version 18)
cmake .. -G "Visual Studio 18 2026" -A x64 -DCMAKE_TOOLCHAIN_FILE=%VCPKG_ROOT%\scripts\buildsystems\vcpkg.cmake

# For Visual Studio 2022 (version 17)
cmake .. -G "Visual Studio 17 2022" -A x64 -DCMAKE_TOOLCHAIN_FILE=%VCPKG_ROOT%\scripts\buildsystems\vcpkg.cmake
```

Binary will be at: `build\Release\FilestashSync.exe`

### Troubleshooting

**CMake can't find Visual Studio:**
- Run from "Developer Command Prompt for VS" (search in Start menu)
- Or run: `"C:\Program Files\Microsoft Visual Studio\2026\Community\Common7\Tools\VsDevCmd.bat"`

**VCPKG_ROOT not set:**
```bash
set VCPKG_ROOT=C:\path\to\vcpkg
```

### Run

```bash
FilestashSync.exe --url https://filestash.example.com --token YOUR_TOKEN C:\Users\Name\Filestash
```

**Important:** The sync root directory must be empty.

## What Works

- ✅ List directories
- ✅ Read files (on-demand download)
- ✅ File Explorer integration (appears in navigation pane)
- ✅ Download progress indicators
- ❌ File upload (not implemented)
- ❌ Rename/delete sync (not implemented)
- ❌ Bidirectional sync (not implemented)

## Architecture

- **main.cpp**: CLI entry point
- **FilestashClient.cpp**: HTTP client (WinHTTP)
- **CloudSyncProvider.cpp**: Cloud Files API integration (cfapi.h)

## Why C++ instead of C#?

C++ uses `#include <cfapi.h>` directly - no need to manually declare 300+ lines of enums/structs/P/Invoke signatures like in C#.
