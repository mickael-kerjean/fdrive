#pragma once

#include <string>
#include <vector>
#include <functional>
#include <windows.h>
#include <winhttp.h>

namespace Filestash {

struct FileEntry {
    std::wstring name;
    std::wstring type;
    int64_t size;
    int64_t time;
};

class Client {
public:
    Client(const std::wstring& baseUrl, const std::wstring& token);
    ~Client();

    // Disable copy
    Client(const Client&) = delete;
    Client& operator=(const Client&) = delete;

    std::vector<FileEntry> ListDir(const std::wstring& path);
    std::vector<uint8_t> ReadFile(const std::wstring& path);

    // Streaming version - callback receives chunks as they arrive from network
    void ReadFileStreaming(
        const std::wstring& path,
        int64_t offset,
        int64_t length,
        std::function<bool(const uint8_t* data, size_t size)> callback
    );

    void WriteFile(const std::wstring& path, const std::vector<uint8_t>& data);
    void MkDir(const std::wstring& path);
    void Remove(const std::wstring& path);
    void Rename(const std::wstring& from, const std::wstring& to);
    void Touch(const std::wstring& path);

private:
    std::wstring m_baseUrl;
    std::wstring m_token;
    HINTERNET m_session;

    std::vector<uint8_t> HttpRequest(
        const std::wstring& method,
        const std::wstring& path,
        const std::vector<uint8_t>* body = nullptr
    );
};

} // namespace Filestash
