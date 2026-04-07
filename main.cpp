#include "FilestashClient.h"
#include "CloudSyncProvider.h"
#include <iostream>
#include <string>
#include <memory>
#include <atomic>
#include <windows.h>

// Global flag for graceful shutdown
std::atomic<bool> g_shutdownRequested(false);

void PrintUsage(const wchar_t* programName) {
    std::wcout << L"Usage: " << programName << L" --url URL --token TOKEN\n";
    std::wcout << L"\nMount a Filestash instance as a Windows Cloud Files sync provider.\n\n";
    std::wcout << L"Arguments:\n";
    std::wcout << L"  --url URL      Filestash instance URL (e.g., https://filestash.example.com)\n";
    std::wcout << L"  --token TOKEN  Authentication token\n";
    std::wcout << L"\nThe sync root will be automatically created at: %USERPROFILE%\\Filestash\n";
    std::wcout << L"\nExample:\n";
    std::wcout << L"  " << programName << L" --url https://filestash.example.com --token YOUR_TOKEN\n";
}

struct Args {
    std::wstring url;
    std::wstring token;
};

bool ParseArgs(int argc, wchar_t* argv[], Args& args) {
    if (argc < 5) {
        return false;
    }

    for (int i = 1; i < argc; i++) {
        std::wstring arg = argv[i];

        if (arg == L"--url" && i + 1 < argc) {
            args.url = argv[++i];
        }
        else if (arg == L"--token" && i + 1 < argc) {
            args.token = argv[++i];
        }
    }

    return !args.url.empty() && !args.token.empty();
}

BOOL WINAPI ConsoleCtrlHandler(DWORD ctrlType) {
    if (ctrlType == CTRL_C_EVENT || ctrlType == CTRL_BREAK_EVENT) {
        std::wcout << L"\n\nShutting down...\n";
        g_shutdownRequested = true;
        return TRUE;  // We handled it
    }
    return FALSE;  // Let default handler handle it
}

int wmain(int argc, wchar_t* argv[]) {
    Args args;

    if (!ParseArgs(argc, argv, args)) {
        PrintUsage(argv[0]);
        return 1;
    }

    // Remove trailing slash from URL
    if (!args.url.empty() && args.url.back() == L'/') {
        args.url.pop_back();
    }

    // Construct sync root path automatically: %USERPROFILE%\Filestash
    wchar_t userProfile[MAX_PATH];
    DWORD result = GetEnvironmentVariableW(L"USERPROFILE", userProfile, MAX_PATH);
    if (result == 0 || result >= MAX_PATH) {
        std::wcerr << L"Error: Could not get USERPROFILE environment variable\n";
        return 1;
    }

    std::wstring syncRoot = std::wstring(userProfile) + L"\\Filestash";
    std::wcout << L"Sync root will be: " << syncRoot << L"\n\n";

    try {
        // Check if sync root exists and is empty
        DWORD attrs = GetFileAttributesW(syncRoot.c_str());
        if (attrs == INVALID_FILE_ATTRIBUTES) {
            std::wcout << L"Creating sync root directory: " << syncRoot << std::endl;
            if (!CreateDirectoryW(syncRoot.c_str(), nullptr)) {
                std::wcerr << L"Failed to create directory\n";
                return 1;
            }
        }
        else if (!(attrs & FILE_ATTRIBUTE_DIRECTORY)) {
            std::wcerr << L"Error: Sync root must be a directory\n";
            return 1;
        }

        // Check if directory is empty
        WIN32_FIND_DATAW findData;
        std::wstring searchPath = syncRoot + L"\\*";
        HANDLE hFind = FindFirstFileW(searchPath.c_str(), &findData);

        if (hFind != INVALID_HANDLE_VALUE) {
            bool isEmpty = true;
            do {
                std::wstring name = findData.cFileName;
                if (name != L"." && name != L"..") {
                    isEmpty = false;
                    break;
                }
            } while (FindNextFileW(hFind, &findData));
            FindClose(hFind);

            if (!isEmpty) {
                std::wcerr << L"Error: Sync root directory must be empty\n";
                return 1;
            }
        }

        // Test connection
        std::wcout << L"Testing connection to " << args.url << L"...\n";
        auto client = std::make_shared<Filestash::Client>(args.url, args.token);

        auto testList = client->ListDir(L"/");
        std::wcout << L"Connection successful! Found " << testList.size() << L" entries in root\n";

        // Create provider
        std::wcout << L"\nRegistering sync root...\n";
        auto provider = std::make_unique<Filestash::CloudSyncProvider>(client, syncRoot);

        // Try to unregister first in case it's already registered (ignore errors)
        // Note: Windows returns different HRESULTs for "already registered":
        // - ERROR_CLOUD_FILE_PROVIDER_ALREADY_REGISTERED (from winerror.h)
        // - You can check specific codes with: err.exe 0xHRESULT (Windows SDK tool)
        // Simpler to just always unregister first rather than checking specific codes
        try {
            provider->UnregisterSyncRoot();
            Sleep(500);
        }
        catch (...) {
            // Ignore - sync root probably wasn't registered
        }

        // Now register
        provider->RegisterSyncRoot();

        std::wcout << L"Connecting to sync root...\n";
        provider->Connect();

        std::wcout << L"Populating initial namespace...\n";
        provider->PopulateNamespace();

        std::wcout << L"\nFilestash mounted at: " << syncRoot << std::endl;
        std::wcout << L"Files will be downloaded on-demand when accessed\n";
        std::wcout << L"Press Ctrl+C to stop and unmount...\n\n";

        // Set up Ctrl+C handler
        SetConsoleCtrlHandler(ConsoleCtrlHandler, TRUE);

        // Wait until shutdown is requested
        while (!g_shutdownRequested) {
            Sleep(100);  // Check every 100ms
        }

        // Cleanup (now reachable!)
        std::wcout << L"Disconnecting...\n";
        provider->Disconnect();

        std::wcout << L"Unregistering sync root...\n";
        provider->UnregisterSyncRoot();

        std::wcout << L"Done!\n";
        return 0;
    }
    catch (const std::exception& e) {
        std::cerr << "Error: " << e.what() << std::endl;
        return 1;
    }
}
