@echo off
REM Manual build script with explicit VS 2026 support

echo Installing dependencies via vcpkg...
vcpkg install nlohmann-json:x64-windows

echo.
echo Configuring with CMake for Visual Studio 2026...
if not exist build mkdir build
cd build

REM Explicitly specify Visual Studio 18 2026
cmake .. -G "Visual Studio 18 2026" -A x64 ^
    -DCMAKE_TOOLCHAIN_FILE=%VCPKG_ROOT%\scripts\buildsystems\vcpkg.cmake

if errorlevel 1 (
    echo.
    echo VS 2026 generator failed, trying auto-detect...
    cmake .. -A x64 ^
        -DCMAKE_TOOLCHAIN_FILE=%VCPKG_ROOT%\scripts\buildsystems\vcpkg.cmake

    if errorlevel 1 (
        echo.
        echo Error: CMake configuration failed
        echo.
        echo Troubleshooting:
        echo 1. Make sure VCPKG_ROOT is set: set VCPKG_ROOT=C:\vcpkg
        echo 2. Run from "Developer Command Prompt for VS 2026"
        echo 3. Check Visual Studio C++ Desktop workload is installed
        pause
        exit /b 1
    )
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
