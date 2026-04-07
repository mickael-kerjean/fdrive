#include "FilestashClient.h"
#include <winhttp.h>
#include <sstream>
#include <stdexcept>
#include <iostream>
#include <nlohmann/json.hpp>

#pragma comment(lib, "winhttp.lib")

using json = nlohmann::json;

namespace Filestash {

Client::Client(const std::wstring& baseUrl, const std::wstring& token)
    : m_baseUrl(baseUrl), m_token(token), m_session(nullptr)
{
    m_session = WinHttpOpen(
        L"FilestashSync/1.0",
        WINHTTP_ACCESS_TYPE_DEFAULT_PROXY,
        WINHTTP_NO_PROXY_NAME,
        WINHTTP_NO_PROXY_BYPASS,
        0
    );

    if (!m_session) {
        throw std::runtime_error("Failed to create HTTP session");
    }

    // Note: WinHTTP automatically handles cookies between requests in the same session
    // No additional configuration needed for cookie persistence
}

Client::~Client() {
    if (m_session) {
        WinHttpCloseHandle(m_session);
    }
}

std::vector<uint8_t> Client::HttpRequest(
    const std::wstring& method,
    const std::wstring& path,
    const std::vector<uint8_t>* body)
{
    // Parse URL
    URL_COMPONENTS urlComp = {};
    urlComp.dwStructSize = sizeof(urlComp);
    wchar_t hostName[256] = {};
    wchar_t urlPath[1024] = {};
    urlComp.lpszHostName = hostName;
    urlComp.dwHostNameLength = _countof(hostName);
    urlComp.lpszUrlPath = urlPath;
    urlComp.dwUrlPathLength = _countof(urlPath);

    std::wstring fullUrl = m_baseUrl + path;
    if (!WinHttpCrackUrl(fullUrl.c_str(), 0, 0, &urlComp)) {
        throw std::runtime_error("Failed to parse URL");
    }

    // Connect
    HINTERNET hConnect = WinHttpConnect(
        m_session,
        urlComp.lpszHostName,
        urlComp.nPort,
        0
    );

    if (!hConnect) {
        throw std::runtime_error("Failed to connect");
    }

    // Open request
    DWORD flags = (urlComp.nScheme == INTERNET_SCHEME_HTTPS) ? WINHTTP_FLAG_SECURE : 0;
    HINTERNET hRequest = WinHttpOpenRequest(
        hConnect,
        method.c_str(),
        urlComp.lpszUrlPath,
        nullptr,
        WINHTTP_NO_REFERER,
        WINHTTP_DEFAULT_ACCEPT_TYPES,
        flags
    );

    if (!hRequest) {
        WinHttpCloseHandle(hConnect);
        throw std::runtime_error("Failed to open request");
    }

    // Add authorization header
    std::wstring authHeader = L"Authorization: Bearer " + m_token;
    WinHttpAddRequestHeaders(
        hRequest,
        authHeader.c_str(),
        (DWORD)-1,
        WINHTTP_ADDREQ_FLAG_ADD
    );

    // Add X-Requested-With header for CSRF protection
    // This identifies the request as coming from an AJAX/API client
    WinHttpAddRequestHeaders(
        hRequest,
        L"X-Requested-With: XMLHttpRequest",
        (DWORD)-1,
        WINHTTP_ADDREQ_FLAG_ADD
    );

    // Send request
    BOOL result = WinHttpSendRequest(
        hRequest,
        WINHTTP_NO_ADDITIONAL_HEADERS,
        0,
        body ? (LPVOID)body->data() : WINHTTP_NO_REQUEST_DATA,
        body ? (DWORD)body->size() : 0,
        body ? (DWORD)body->size() : 0,
        0
    );

    if (!result) {
        WinHttpCloseHandle(hRequest);
        WinHttpCloseHandle(hConnect);
        throw std::runtime_error("Failed to send request");
    }

    // Receive response
    if (!WinHttpReceiveResponse(hRequest, nullptr)) {
        WinHttpCloseHandle(hRequest);
        WinHttpCloseHandle(hConnect);
        throw std::runtime_error("Failed to receive response");
    }

    // Read data
    std::vector<uint8_t> responseData;
    DWORD bytesAvailable = 0;
    DWORD bytesRead = 0;
    std::vector<uint8_t> buffer(4096);

    do {
        bytesAvailable = 0;
        if (!WinHttpQueryDataAvailable(hRequest, &bytesAvailable)) {
            break;
        }

        if (bytesAvailable > 0) {
            if (buffer.size() < bytesAvailable) {
                buffer.resize(bytesAvailable);
            }

            if (WinHttpReadData(hRequest, buffer.data(), bytesAvailable, &bytesRead)) {
                responseData.insert(responseData.end(), buffer.begin(), buffer.begin() + bytesRead);
            }
        }
    } while (bytesAvailable > 0);

    WinHttpCloseHandle(hRequest);
    WinHttpCloseHandle(hConnect);

    return responseData;
}

std::vector<FileEntry> Client::ListDir(const std::wstring& path) {
    std::wstring apiPath = L"/api/files/ls?path=" + path;
    auto response = HttpRequest(L"GET", apiPath);

    std::string jsonStr(response.begin(), response.end());
    auto j = json::parse(jsonStr);

    if (j["status"] != "ok") {
        throw std::runtime_error("List failed");
    }

    std::vector<FileEntry> entries;
    for (const auto& item : j["results"]) {
        FileEntry entry;
        std::string name = item["name"];
        std::string type = item["type"];
        entry.name = std::wstring(name.begin(), name.end());
        entry.type = std::wstring(type.begin(), type.end());

        // Size - default to 0 if missing
        entry.size = item.contains("size") && !item["size"].is_null() ? item["size"].get<int64_t>() : 0;

        // Time - default to 0 (will be replaced with current time in CloudSyncProvider)
        entry.time = item.contains("time") && !item["time"].is_null() ? item["time"].get<int64_t>() : 0;

        entries.push_back(entry);
    }

    return entries;
}

std::vector<uint8_t> Client::ReadFile(const std::wstring& path) {
    std::wstring apiPath = L"/api/files/cat?path=" + path;
    return HttpRequest(L"GET", apiPath);
}

void Client::ReadFileStreaming(
    const std::wstring& path,
    int64_t offset,
    int64_t length,
    std::function<bool(const uint8_t* data, size_t size)> callback)
{
    std::wstring apiPath = L"/api/files/cat?path=" + path;
    std::wstring fullUrl = m_baseUrl + apiPath;

    // Parse URL
    URL_COMPONENTS urlComp = {};
    urlComp.dwStructSize = sizeof(urlComp);
    wchar_t hostName[256] = {};
    wchar_t urlPath[1024] = {};
    urlComp.lpszHostName = hostName;
    urlComp.dwHostNameLength = _countof(hostName);
    urlComp.lpszUrlPath = urlPath;
    urlComp.dwUrlPathLength = _countof(urlPath);

    if (!WinHttpCrackUrl(fullUrl.c_str(), 0, 0, &urlComp)) {
        throw std::runtime_error("Failed to parse URL");
    }

    // Connect
    HINTERNET hConnect = WinHttpConnect(m_session, urlComp.lpszHostName, urlComp.nPort, 0);
    if (!hConnect) {
        throw std::runtime_error("Failed to connect");
    }

    // Open request
    DWORD flags = (urlComp.nScheme == INTERNET_SCHEME_HTTPS) ? WINHTTP_FLAG_SECURE : 0;
    HINTERNET hRequest = WinHttpOpenRequest(
        hConnect, L"GET", urlComp.lpszUrlPath, nullptr,
        WINHTTP_NO_REFERER, WINHTTP_DEFAULT_ACCEPT_TYPES, flags
    );

    if (!hRequest) {
        WinHttpCloseHandle(hConnect);
        throw std::runtime_error("Failed to open request");
    }

    // Add authorization header
    std::wstring authHeader = L"Authorization: Bearer " + m_token;
    WinHttpAddRequestHeaders(hRequest, authHeader.c_str(), (DWORD)-1, WINHTTP_ADDREQ_FLAG_ADD);

    // Add X-Requested-With header for CSRF protection
    WinHttpAddRequestHeaders(hRequest, L"X-Requested-With: XMLHttpRequest", (DWORD)-1, WINHTTP_ADDREQ_FLAG_ADD);

    // Send request (full file, no Range header for now)
    if (!WinHttpSendRequest(hRequest, WINHTTP_NO_ADDITIONAL_HEADERS, 0, WINHTTP_NO_REQUEST_DATA, 0, 0, 0)) {
        WinHttpCloseHandle(hRequest);
        WinHttpCloseHandle(hConnect);
        throw std::runtime_error("Failed to send request");
    }

    // Receive response
    if (!WinHttpReceiveResponse(hRequest, nullptr)) {
        WinHttpCloseHandle(hRequest);
        WinHttpCloseHandle(hConnect);
        throw std::runtime_error("Failed to receive response");
    }

    // Check HTTP status code
    DWORD statusCode = 0;
    DWORD statusCodeSize = sizeof(statusCode);
    WinHttpQueryHeaders(hRequest,
        WINHTTP_QUERY_STATUS_CODE | WINHTTP_QUERY_FLAG_NUMBER,
        nullptr, &statusCode, &statusCodeSize, nullptr);

    std::wcout << L"  HTTP Status Code: " << statusCode << std::endl;

    if (statusCode != 200 && statusCode != 206) {
        // Read error response for debugging
        std::vector<uint8_t> errorBuffer(4096);
        DWORD bytesRead = 0;
        if (WinHttpReadData(hRequest, errorBuffer.data(), 4096, &bytesRead)) {
            std::string errorMsg(errorBuffer.begin(), errorBuffer.begin() + bytesRead);
            std::wcerr << L"  HTTP Error Response: " << std::wstring(errorMsg.begin(), errorMsg.end()) << std::endl;
        }
        WinHttpCloseHandle(hRequest);
        WinHttpCloseHandle(hConnect);
        throw std::runtime_error("HTTP request failed with status " + std::to_string(statusCode));
    }

    // Stream data in chunks - call callback as data arrives
    std::vector<uint8_t> buffer(65536);  // 64KB chunks
    DWORD bytesAvailable = 0;
    DWORD bytesRead = 0;
    bool firstChunk = true;

    do {
        bytesAvailable = 0;
        if (!WinHttpQueryDataAvailable(hRequest, &bytesAvailable)) {
            break;
        }

        if (bytesAvailable > 0) {
            if (buffer.size() < bytesAvailable) {
                buffer.resize(bytesAvailable);
            }

            if (WinHttpReadData(hRequest, buffer.data(), bytesAvailable, &bytesRead)) {
                // Debug: Show first bytes of first chunk to see if it's HTML or binary
                if (firstChunk) {
                    std::wcout << L"  First 100 bytes: ";
                    DWORD maxBytes = (bytesRead < 100) ? bytesRead : 100;
                    for (DWORD i = 0; i < maxBytes; i++) {
                        if (buffer[i] >= 32 && buffer[i] < 127) {
                            std::wcout << (wchar_t)buffer[i];
                        } else {
                            std::wcout << L".";
                        }
                    }
                    std::wcout << std::endl;
                    firstChunk = false;
                }

                // Call the callback with this chunk - if it returns false, stop
                if (!callback(buffer.data(), bytesRead)) {
                    break;
                }
            }
        }
    } while (bytesAvailable > 0);

    WinHttpCloseHandle(hRequest);
    WinHttpCloseHandle(hConnect);
}

void Client::WriteFile(const std::wstring& path, const std::vector<uint8_t>& data) {
    std::wstring apiPath = L"/api/files/cat?path=" + path;
    auto response = HttpRequest(L"POST", apiPath, &data);

    std::string jsonStr(response.begin(), response.end());
    auto j = json::parse(jsonStr);

    if (j["status"] != "ok") {
        throw std::runtime_error("Write failed");
    }
}

void Client::MkDir(const std::wstring& path) {
    std::wstring apiPath = L"/api/files/mkdir?path=" + path;
    auto response = HttpRequest(L"POST", apiPath);

    std::string jsonStr(response.begin(), response.end());
    auto j = json::parse(jsonStr);

    if (j["status"] != "ok") {
        throw std::runtime_error("MkDir failed");
    }
}

void Client::Remove(const std::wstring& path) {
    std::wstring apiPath = L"/api/files/rm?path=" + path;
    auto response = HttpRequest(L"POST", apiPath);

    std::string jsonStr(response.begin(), response.end());
    auto j = json::parse(jsonStr);

    if (j["status"] != "ok") {
        throw std::runtime_error("Remove failed");
    }
}

void Client::Rename(const std::wstring& from, const std::wstring& to) {
    std::wstring apiPath = L"/api/files/mv?from=" + from + L"&to=" + to;
    auto response = HttpRequest(L"POST", apiPath);

    std::string jsonStr(response.begin(), response.end());
    auto j = json::parse(jsonStr);

    if (j["status"] != "ok") {
        throw std::runtime_error("Rename failed");
    }
}

void Client::Touch(const std::wstring& path) {
    std::wstring apiPath = L"/api/files/touch?path=" + path;
    auto response = HttpRequest(L"POST", apiPath);

    std::string jsonStr(response.begin(), response.end());
    auto j = json::parse(jsonStr);

    if (j["status"] != "ok") {
        throw std::runtime_error("Touch failed");
    }
}

} // namespace Filestash
