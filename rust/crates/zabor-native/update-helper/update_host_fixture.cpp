#define WIN32_LEAN_AND_MEAN
#define NOMINMAX
#include <windows.h>
#include <array>
#include <cstdint>
#include <cstring>
#include <string>
#include <vector>

uint64_t number(const std::array<BYTE, 264>& frame, size_t offset) {
    uint64_t value = 0;
    std::memcpy(&value, frame.data() + offset, sizeof(value));
    return value;
}

uint64_t creation(HANDLE process) {
    FILETIME made{}, exited{}, kernel{}, user{};
    if (!GetProcessTimes(process, &made, &exited, &kernel, &user)) return 0;
    return (static_cast<uint64_t>(made.dwHighDateTime) << 32) | made.dwLowDateTime;
}

bool transfer(HANDLE pipe, void* bytes, DWORD size, bool write) {
    DWORD count = 0;
    return (write ? WriteFile(pipe, bytes, size, &count, nullptr) : ReadFile(pipe, bytes, size, &count, nullptr)) != FALSE && count == size;
}

int wmain(int argc, wchar_t** argv) {
    if (argc == 2 && std::wstring(argv[1]) == L"--owned-child") {
        Sleep(120000);
        return 0;
    }
    if (argc != 2 || std::wstring(argv[1]) != L"--native-update-health") return 2;
#if FIXTURE_MODE == 4
    wchar_t marker[32768]{};
    if (!GetEnvironmentVariableW(L"ZABOR_HOST_TEST_RESUME_MARKER", marker, 32768)) return 2;
    HANDLE file = CreateFileW(marker, GENERIC_WRITE, 0, nullptr, CREATE_NEW, 0, nullptr);
    if (file == INVALID_HANDLE_VALUE) return 2;
    CloseHandle(file);
#endif
    std::array<BYTE, 264> frame{};
    uint16_t length = 0;
    const auto input = GetStdHandle(STD_INPUT_HANDLE);
    if (!transfer(input, frame.data(), static_cast<DWORD>(frame.size()), false) ||
        !transfer(input, &length, sizeof(length), false) || length == 0 || length > 128) return 2;
    std::vector<char> name(length);
    if (!transfer(input, name.data(), length, false) || number(frame, 48) != GetCurrentProcessId() ||
        number(frame, 56) != creation(GetCurrentProcess())) return 2;
    wchar_t path[32768]{};
    if (!GetModuleFileNameW(nullptr, path, 32768)) return 2;
    std::wstring command = L"\"" + std::wstring(path) + L"\" --owned-child";
    STARTUPINFOW startup{};
    startup.cb = sizeof(startup);
    PROCESS_INFORMATION child{};
    if (!CreateProcessW(path, command.data(), nullptr, nullptr, FALSE, 0, nullptr, nullptr, &startup, &child)) return 2;
    CloseHandle(child.hThread);
    CloseHandle(child.hProcess);
    if (FIXTURE_MODE == 2) { Sleep(120000); return 2; }
    if (FIXTURE_MODE == 3) return 2;
    const std::wstring pipe_name(name.begin(), name.end());
    HANDLE pipe = CreateFileW(pipe_name.c_str(), GENERIC_READ | GENERIC_WRITE, 0, nullptr,
        OPEN_EXISTING, SECURITY_SQOS_PRESENT | SECURITY_IDENTIFICATION, nullptr);
    if (pipe == INVALID_HANDLE_VALUE) return 2;
    ULONG server = 0;
    if (!GetNamedPipeServerProcessId(pipe, &server) || server != number(frame, 32)) { CloseHandle(pipe); return 2; }
    HANDLE helper = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION | SYNCHRONIZE, FALSE, server);
    if (helper == nullptr || creation(helper) != number(frame, 40)) {
        if (helper != nullptr) CloseHandle(helper);
        CloseHandle(pipe);
        return 2;
    }
    CloseHandle(helper);
    std::array<BYTE, 264> challenge{};
    if (!transfer(pipe, challenge.data(), static_cast<DWORD>(challenge.size()), false) || challenge != frame) {
        CloseHandle(pipe);
        return 2;
    }
    if (FIXTURE_MODE == 1) challenge[72] ^= 1;
    const bool sent = transfer(pipe, challenge.data(), static_cast<DWORD>(challenge.size()), true);
    CloseHandle(pipe);
    SecureZeroMemory(challenge.data(), challenge.size());
    SecureZeroMemory(frame.data(), frame.size());
    if (!sent) return 2;
    Sleep(120000);
    return 0;
}
