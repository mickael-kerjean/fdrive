@echo off
REM Build script for FilestashSync (C++ version)

echo Installing dependencies via vcpkg...
vcpkg install nlohmann-json:x64-windows

echo.
echo Configuring with CMake...
if not exist build mkdir build
cd build

REM Let CMake auto-detect Visual Studio version (works with VS 2019, 2022, 2026, etc.)
cmake .. -A x64 ^
    -DCMAKE_TOOLCHAIN_FILE=%VCPKG_ROOT%\scripts\buildsystems\vcpkg.cmake

if errorlevel 1 (
    echo.
    echo Error: CMake configuration failed
    echo.
    echo Troubleshooting:
    echo 1. Make sure VCPKG_ROOT environment variable is set
    echo    Example: set VCPKG_ROOT=C:\vcpkg
    echo 2. Make sure Visual Studio C++ workload is installed
    echo 3. Try running from "Developer Command Prompt for VS"
    pause
    exit /b 1
)

echo.
echo Building...
cmake --build . --config Release

if errorlevel 1 (
    echo.
    echo Error: Build failed
    pause
    exit /b 1
)

echo.
echo Build successful!
echo Binary location: build\Release\FilestashSync.exe
pause
