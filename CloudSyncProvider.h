#pragma once

#include "FilestashClient.h"
#include <windows.h>
#include <cfapi.h>
#include <string>
#include <memory>

namespace Filestash {

class CloudSyncProvider {
public:
    CloudSyncProvider(std::shared_ptr<Client> client, const std::wstring& syncRootPath);
    ~CloudSyncProvider();

    // Disable copy
    CloudSyncProvider(const CloudSyncProvider&) = delete;
    CloudSyncProvider& operator=(const CloudSyncProvider&) = delete;

    void RegisterSyncRoot();
    void UnregisterSyncRoot();
    void Connect();
    void Disconnect();
    void PopulateNamespace(const std::wstring& relativePath = L"");

private:
    std::shared_ptr<Client> m_client;
    std::wstring m_syncRootPath;
    CF_CONNECTION_KEY m_connectionKey;

    // Callback functions (called by Windows)
    static void CALLBACK OnFetchData(
        const CF_CALLBACK_INFO* callbackInfo,
        const CF_CALLBACK_PARAMETERS* callbackParameters
    );

    static void CALLBACK OnFetchPlaceholders(
        const CF_CALLBACK_INFO* callbackInfo,
        const CF_CALLBACK_PARAMETERS* callbackParameters
    );

    static void CALLBACK OnCancelFetchData(
        const CF_CALLBACK_INFO* callbackInfo,
        const CF_CALLBACK_PARAMETERS* callbackParameters
    );

    void UpdateProviderStatus(CF_SYNC_PROVIDER_STATUS status);
};

} // namespace Filestash
