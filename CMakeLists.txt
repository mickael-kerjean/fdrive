cmake_minimum_required(VERSION 3.20)
project(FilestashSync)

set(CMAKE_CXX_STANDARD 17)
set(CMAKE_CXX_STANDARD_REQUIRED ON)

# Windows SDK required
if(NOT WIN32)
    message(FATAL_ERROR "This project requires Windows")
endif()

# Require Windows 10 version 1809+
add_definitions(-DWINVER=0x0A00 -D_WIN32_WINNT=0x0A00)
add_definitions(-DUNICODE -D_UNICODE)

# Find nlohmann_json (install via vcpkg: vcpkg install nlohmann-json)
find_package(nlohmann_json CONFIG REQUIRED)

# Source files
add_executable(FilestashSync
    main.cpp
    FilestashClient.cpp
    CloudSyncProvider.cpp
)

# Link libraries
target_link_libraries(FilestashSync
    PRIVATE
    nlohmann_json::nlohmann_json
    cldapi.lib      # Cloud Files API
    winhttp.lib     # HTTP client
    ole32.lib       # COM
    shlwapi.lib     # Shell utilities
)
