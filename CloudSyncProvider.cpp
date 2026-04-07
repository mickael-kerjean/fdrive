#include "CloudSyncProvider.h"
#include <iostream>
#include <algorithm>
#include <shlwapi.h>
#include <combaseapi.h>
#include <winternl.h>

#pragma comment(lib, "cldapi.lib")
#pragma comment(lib, "shlwapi.lib")

#ifndef STATUS_SUCCESS
#define STATUS_SUCCESS ((NTSTATUS)0x00000000L)
#endif

#ifndef STATUS_UNSUCCESSFUL
#define STATUS_UNSUCCESSFUL ((NTSTATUS)0xC0000001L)
#endif

static CF_CONNECTION_KEY InvalidConnectionKey() {
    CF_CONNECTION_KEY key = {};
    return key;
}

static bool IsValidConnectionKey(const CF_CONNECTION_KEY& key) {
    return memcmp(&key, &InvalidConnectionKey(), sizeof(CF_CONNECTION_KEY)) != 0;
}

namespace Filestash {

CloudSyncProvider::CloudSyncProvider(
    std::shared_ptr<Client> client,
    const std::wstring& syncRootPath)
    : m_client(client)
    , m_syncRootPath(syncRootPath)
    , m_connectionKey(InvalidConnectionKey())
{
}

CloudSyncProvider::~CloudSyncProvider() {
    Disconnect();
}

void CloudSyncProvider::RegisterSyncRoot() {
    GUID providerId;
    CoCreateGuid(&providerId);

    CF_SYNC_REGISTRATION registration = {};
    registration.StructSize = sizeof(CF_SYNC_REGISTRATION);
    registration.ProviderName = L"Filestash";
    registration.ProviderVersion = L"1.0.0";
    registration.ProviderId = providerId;

    CF_SYNC_POLICIES policies = {};
    policies.StructSize = sizeof(CF_SYNC_POLICIES);
    policies.Hydration.Primary = (CF_HYDRATION_POLICY_PRIMARY)2;  // Full
    policies.Hydration.Modifier = (CF_HYDRATION_POLICY_MODIFIER)0;
    policies.Population.Primary = (CF_POPULATION_POLICY_PRIMARY)2;  // Full
    policies.Population.Modifier = (CF_POPULATION_POLICY_MODIFIER)0;

    HRESULT hr = CfRegisterSyncRoot(
        m_syncRootPath.c_str(),
        &registration,
        &policies,
        CF_REGISTER_FLAG_NONE
    );

    if (FAILED(hr)) {
        throw std::runtime_error("Failed to register sync root: HRESULT " + std::to_string(hr));
    }

    std::wcout << L"Sync root registered at: " << m_syncRootPath << std::endl;
}

void CloudSyncProvider::UnregisterSyncRoot() {
    HRESULT hr = CfUnregisterSyncRoot(m_syncRootPath.c_str());
    if (FAILED(hr)) {
        std::wcerr << L"Warning: Failed to unregister sync root: " << hr << std::endl;
    } else {
        std::wcout << L"Sync root unregistered" << std::endl;
    }
}

void CloudSyncProvider::Connect() {
    std::wcout << L"CloudSyncProvider::Connect" << std::endl;
    CF_CALLBACK_REGISTRATION callbacks[] = {
        { CF_CALLBACK_TYPE_FETCH_DATA, OnFetchData },
        { CF_CALLBACK_TYPE_FETCH_PLACEHOLDERS, OnFetchPlaceholders },
        { CF_CALLBACK_TYPE_CANCEL_FETCH_DATA, OnCancelFetchData },
        { CF_CALLBACK_TYPE_NONE, nullptr }
    };

    HRESULT hr = CfConnectSyncRoot(
        m_syncRootPath.c_str(),
        callbacks,
        this,
        CF_CONNECT_FLAG_NONE,
        &m_connectionKey
    );

    if (FAILED(hr)) {
        throw std::runtime_error("Failed to connect sync root: HRESULT " + std::to_string(hr));
    }

    UpdateProviderStatus(CF_PROVIDER_STATUS_IDLE);
    std::wcout << L"Connected to sync root" << std::endl;
}

void CloudSyncProvider::Disconnect() {
    std::wcout << L"CloudSyncProvider::Disconnect" << std::endl;
    if (IsValidConnectionKey(m_connectionKey)) {
        CfDisconnectSyncRoot(m_connectionKey);
        m_connectionKey = InvalidConnectionKey();
        std::wcout << L"Disconnected from sync root" << std::endl;
    }
}

void CloudSyncProvider::PopulateNamespace(const std::wstring& relativePath) {
    std::wcout << L"CloudSyncProvider::PopulateNamespace" << std::endl;
    UpdateProviderStatus(CF_PROVIDER_STATUS_POPULATE_NAMESPACE);

    try {
        // Convert to API path
        std::wstring apiPath = L"/";
        if (!relativePath.empty()) {
            apiPath = L"/" + relativePath;
            std::replace(apiPath.begin(), apiPath.end(), L'\\', L'/');
            if (!apiPath.empty() && apiPath.back() != L'/') {
                apiPath += L'/';
            }
        }

        // Get file list from Filestash
        auto entries = m_client->ListDir(apiPath);

        if (entries.empty()) {
            std::wcout << L"  - no entries found in " << apiPath << std::endl;
            return;
        }

        std::vector<std::wstring> fileNames;
        std::vector<std::wstring> fullPaths;
        std::vector<CF_PLACEHOLDER_CREATE_INFO> placeholders;
        fileNames.reserve(entries.size());
        fullPaths.reserve(entries.size());
        placeholders.reserve(entries.size());

        for (const auto& entry : entries) {
            fileNames.push_back(entry.name);

            std::wstring fullPath = relativePath.empty() ? entry.name : (relativePath + L"\\" + entry.name);
            fullPaths.push_back(fullPath);

            CF_PLACEHOLDER_CREATE_INFO placeholder = {};
            placeholder.RelativeFileName = fileNames.back().c_str();
            placeholder.FileIdentity = (LPCVOID)fullPaths.back().c_str();
            placeholder.FileIdentityLength = (DWORD)((fullPaths.back().size() + 1) * sizeof(WCHAR));

            if (entry.type == L"directory") {
                placeholder.FsMetadata.BasicInfo.FileAttributes = FILE_ATTRIBUTE_DIRECTORY;
                placeholder.FsMetadata.FileSize.QuadPart = 0;
                placeholder.Flags = CF_PLACEHOLDER_CREATE_FLAG_NONE;
            } else {
                placeholder.FsMetadata.FileSize.QuadPart = entry.size;
                placeholder.FsMetadata.BasicInfo.FileAttributes = FILE_ATTRIBUTE_NORMAL;
                placeholder.Flags = CF_PLACEHOLDER_CREATE_FLAG_NONE;
            }

            LONGLONG fileTime;
            if (entry.time > 0 && entry.time < 4102444800000LL) {
                fileTime = (entry.time * 10000LL) + 116444736000000000LL;
            } else {
                FILETIME ft;
                GetSystemTimeAsFileTime(&ft);
                ULARGE_INTEGER uli;
                uli.LowPart = ft.dwLowDateTime;
                uli.HighPart = ft.dwHighDateTime;
                fileTime = uli.QuadPart;
            }

            placeholder.FsMetadata.BasicInfo.CreationTime.QuadPart = fileTime;
            placeholder.FsMetadata.BasicInfo.LastWriteTime.QuadPart = fileTime;
            placeholder.FsMetadata.BasicInfo.LastAccessTime.QuadPart = fileTime;
            placeholder.FsMetadata.BasicInfo.ChangeTime.QuadPart = fileTime;

            placeholders.push_back(placeholder);
        }

        // Create placeholders
        std::wstring baseDir = m_syncRootPath;
        if (!relativePath.empty()) {
            baseDir += L"\\" + relativePath;
        }

        DWORD attrs = GetFileAttributesW(baseDir.c_str());
        if (attrs == INVALID_FILE_ATTRIBUTES || !(attrs & FILE_ATTRIBUTE_DIRECTORY)) {
            std::wcerr << L"ERROR: Invalid target directory: " << baseDir << std::endl;
            return;
        }

        std::wcout << L"  - creating " << placeholders.size() << L" placeholders in " << baseDir << std::endl;

        std::vector<size_t> dirIndices;
        std::vector<size_t> fileIndices;
        for (size_t i = 0; i < placeholders.size(); i++) {
            if (placeholders[i].FsMetadata.BasicInfo.FileAttributes & FILE_ATTRIBUTE_DIRECTORY) {
                dirIndices.push_back(i);
            } else {
                fileIndices.push_back(i);
            }
        }

        DWORD failedCount = 0;
        DWORD successCount = 0;
        for (size_t idx : dirIndices) {
            const auto& ph = placeholders[idx];

            DWORD entriesProcessed = 0;
            HRESULT hr = CfCreatePlaceholders(
                baseDir.c_str(),
                &placeholders[idx],
                1,
                CF_CREATE_FLAG_NONE,
                &entriesProcessed
            );

            if (SUCCEEDED(hr)) {
                successCount++;
            } else if (hr == HRESULT_FROM_WIN32(ERROR_ALREADY_EXISTS)) {
                std::wstring fullPath = baseDir + L"\\" + ph.RelativeFileName;
                HANDLE hFile = CreateFileW(
                    fullPath.c_str(),
                    GENERIC_READ | GENERIC_WRITE,
                    FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE,
                    nullptr,
                    OPEN_EXISTING,
                    FILE_FLAG_BACKUP_SEMANTICS,
                    nullptr
                );

                if (hFile != INVALID_HANDLE_VALUE) {
                    HRESULT hrUpdate = CfUpdatePlaceholder(
                        hFile,
                        &ph.FsMetadata,
                        ph.FileIdentity,
                        ph.FileIdentityLength,
                        nullptr,
                        0,
                        CF_UPDATE_FLAG_PASSTHROUGH_FS_METADATA,
                        nullptr,
                        nullptr
                    );
                    CloseHandle(hFile);
                    if (SUCCEEDED(hrUpdate)) {
                        successCount++;
                    } else {
                        failedCount++;
                    }
                } else {
                    failedCount++;
                }
            } else {
                failedCount++;
            }
        }

        for (size_t idx : fileIndices) {
            const auto& ph = placeholders[idx];

            DWORD entriesProcessed = 0;
            HRESULT hr = CfCreatePlaceholders(
                baseDir.c_str(),
                &placeholders[idx],
                1,
                CF_CREATE_FLAG_NONE,
                &entriesProcessed
            );

            if (SUCCEEDED(hr)) {
                successCount++;
            } else if (hr == HRESULT_FROM_WIN32(ERROR_ALREADY_EXISTS)) {
                std::wstring fullPath = baseDir + L"\\" + ph.RelativeFileName;
                HANDLE hFile = CreateFileW(
                    fullPath.c_str(),
                    GENERIC_READ | GENERIC_WRITE,
                    FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE,
                    nullptr,
                    OPEN_EXISTING,
                    FILE_FLAG_BACKUP_SEMANTICS,
                    nullptr
                );

                if (hFile != INVALID_HANDLE_VALUE) {
                    HRESULT hrUpdate = CfUpdatePlaceholder(
                        hFile,
                        &ph.FsMetadata,
                        ph.FileIdentity,
                        ph.FileIdentityLength,
                        nullptr,
                        0,
                        CF_UPDATE_FLAG_PASSTHROUGH_FS_METADATA,
                        nullptr,
                        nullptr
                    );
                    CloseHandle(hFile);
                    if (SUCCEEDED(hrUpdate)) {
                        successCount++;
                    } else {
                        failedCount++;
                    }
                } else {
                    failedCount++;
                }
            } else {
                failedCount++;
            }
        }

        std::wcout << L"Created " << successCount << L" placeholders";
        if (failedCount > 0) {
            std::wcout << L" (" << failedCount << L" failed)";
        }
        std::wcout << std::endl;
    }
    catch (const std::exception& e) {
        std::cerr << "ERROR populating namespace: " << e.what() << std::endl;
    }

    UpdateProviderStatus(CF_PROVIDER_STATUS_IDLE);
}

void CALLBACK CloudSyncProvider::OnFetchData(
    const CF_CALLBACK_INFO* callbackInfo,
    const CF_CALLBACK_PARAMETERS* callbackParameters)
{
    std::wcout << L"CloudSyncProvider::OnFetchData" << std::endl;
    auto* provider = static_cast<CloudSyncProvider*>(callbackInfo->CallbackContext);

    try {
        if (!callbackInfo->FileIdentity || callbackInfo->FileIdentityLength == 0) {
            throw std::runtime_error("FileIdentity is NULL");
        }

        std::wstring relativePath((WCHAR*)callbackInfo->FileIdentity, callbackInfo->FileIdentityLength / sizeof(WCHAR));
        if (!relativePath.empty() && relativePath.back() == L'\0') {
            relativePath.pop_back();
        }

        std::wstring apiPath = relativePath;
        std::replace(apiPath.begin(), apiPath.end(), L'\\', L'/');
        if (apiPath.empty() || apiPath[0] != L'/') {
            apiPath = L"/" + apiPath;
        }

        std::wcout << L"  - Downloading: " << apiPath << std::endl;
        auto data = provider->m_client->ReadFile(apiPath);
        std::wcout << L"  - Downloaded " << data.size() << L" bytes" << std::endl;

        LARGE_INTEGER requestedOffset = callbackParameters->FetchData.RequiredFileOffset;
        LARGE_INTEGER requestedLength = callbackParameters->FetchData.RequiredLength;

        if (requestedOffset.QuadPart < 0 || requestedOffset.QuadPart > (LONGLONG)data.size()) {
            throw std::runtime_error("Invalid offset");
        }

        LONGLONG bytesToTransfer = requestedLength.QuadPart;
        if (requestedOffset.QuadPart + bytesToTransfer > (LONGLONG)data.size()) {
            bytesToTransfer = data.size() - requestedOffset.QuadPart;
        }

        if (bytesToTransfer <= 0) {
            return;
        }

        CF_OPERATION_INFO opInfo = {};
        opInfo.StructSize = sizeof(CF_OPERATION_INFO);
        opInfo.Type = CF_OPERATION_TYPE_TRANSFER_DATA;
        opInfo.ConnectionKey = callbackInfo->ConnectionKey;
        opInfo.TransferKey = callbackInfo->TransferKey;

        CF_OPERATION_PARAMETERS opParams = {};
        opParams.ParamSize = sizeof(CF_OPERATION_PARAMETERS);
        opParams.TransferData.CompletionStatus = STATUS_SUCCESS;
        opParams.TransferData.Buffer = data.data() + requestedOffset.QuadPart;
        opParams.TransferData.Offset = requestedOffset;
        opParams.TransferData.Length.QuadPart = bytesToTransfer;

        HRESULT hr = CfExecute(&opInfo, &opParams);
        if (FAILED(hr)) {
            throw std::runtime_error("Transfer failed");
        }
    }
    catch (const std::exception& e) {
        std::cerr << "ERROR in OnFetchData: " << e.what() << std::endl;

        CF_OPERATION_INFO opInfo = {};
        opInfo.StructSize = sizeof(CF_OPERATION_INFO);
        opInfo.Type = CF_OPERATION_TYPE_TRANSFER_DATA;
        opInfo.ConnectionKey = callbackInfo->ConnectionKey;
        opInfo.TransferKey = callbackInfo->TransferKey;

        CF_OPERATION_PARAMETERS opParams = {};
        opParams.ParamSize = sizeof(CF_OPERATION_PARAMETERS);
        opParams.TransferData.CompletionStatus = STATUS_UNSUCCESSFUL;
        opParams.TransferData.Buffer = nullptr;
        opParams.TransferData.Offset.QuadPart = 0;
        opParams.TransferData.Length.QuadPart = 0;

        CfExecute(&opInfo, &opParams);
    }
}

void CALLBACK CloudSyncProvider::OnFetchPlaceholders(
    const CF_CALLBACK_INFO* callbackInfo,
    const CF_CALLBACK_PARAMETERS* callbackParameters)
{
    std::wcout << L"CloudSyncProvider::OnFetchPlaceholder" << std::endl;
    auto* provider = static_cast<CloudSyncProvider*>(callbackInfo->CallbackContext);

    try {
        std::wstring path = callbackInfo->NormalizedPath;
        std::wstring relativePath;

        size_t syncRootOffset = provider->m_syncRootPath.length();
        if (syncRootOffset >= 2 && provider->m_syncRootPath[1] == L':') {
            syncRootOffset -= 2;
        }

        if (path.length() >= syncRootOffset) {
            relativePath = path.substr(syncRootOffset);
            if (!relativePath.empty() && relativePath[0] == L'\\') {
                relativePath = relativePath.substr(1);
            }
        }

        std::wcout << L"  - Fetching placeholders: " << (relativePath.empty() ? L"(root)" : relativePath) << std::endl;

        provider->PopulateNamespace(relativePath);

        CF_OPERATION_INFO opInfo = {};
        opInfo.StructSize = sizeof(CF_OPERATION_INFO);
        opInfo.Type = CF_OPERATION_TYPE_TRANSFER_PLACEHOLDERS;
        opInfo.ConnectionKey = callbackInfo->ConnectionKey;
        opInfo.TransferKey = callbackInfo->TransferKey;

        CF_OPERATION_PARAMETERS opParams = {};
        opParams.ParamSize = sizeof(CF_OPERATION_PARAMETERS);
        opParams.TransferPlaceholders.CompletionStatus = STATUS_SUCCESS;
        opParams.TransferPlaceholders.Flags = CF_OPERATION_TRANSFER_PLACEHOLDERS_FLAG_DISABLE_ON_DEMAND_POPULATION;
        opParams.TransferPlaceholders.PlaceholderTotalCount.QuadPart = 0;

        HRESULT hr = CfExecute(&opInfo, &opParams);
        if (FAILED(hr)) {
            std::wcerr << L"ERROR Failed to complete placeholder transfer: 0x" << std::hex << hr << std::dec << std::endl;
        }
    }
    catch (const std::exception& e) {
        std::cerr << "ERROR in OnFetchPlaceholders: " << e.what() << std::endl;
    }
}

void CALLBACK CloudSyncProvider::OnCancelFetchData(
    const CF_CALLBACK_INFO* callbackInfo,
    const CF_CALLBACK_PARAMETERS* callbackParameters)
{
    std::wcout << L"CloudSyncProvider::OnCancelFetchData" << std::endl;
}

void CloudSyncProvider::UpdateProviderStatus(CF_SYNC_PROVIDER_STATUS status) {
    std::wcout << L"CloudSyncProvider::UpdateProviderStatus" << std::endl;
    if (IsValidConnectionKey(m_connectionKey)) {
        CfUpdateSyncProviderStatus(m_connectionKey, status);
    }
}

} // namespace Filestash
