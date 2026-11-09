#define wmain primitive_main
#include "native_update_helper.cpp"
#undef wmain
#include <array>
#include <cstdint>
#include <cstring>

using Frame = std::array<BYTE, 264>;

uint64_t number(const Frame& frame, size_t offset) {
    uint64_t value = 0;
    std::memcpy(&value, frame.data() + offset, sizeof(value));
    return value;
}

void put(Frame& frame, size_t offset, uint64_t value) {
    std::memcpy(frame.data() + offset, &value, sizeof(value));
}

uint64_t creation(HANDLE process) {
    FILETIME made{}, exited{}, kernel{}, user{};
    if (!GetProcessTimes(process, &made, &exited, &kernel, &user)) return 0;
    return (static_cast<uint64_t>(made.dwHighDateTime) << 32) | made.dwLowDateTime;
}

uint64_t unix_ms() {
    FILETIME time{};
    GetSystemTimeAsFileTime(&time);
    const uint64_t ticks = (static_cast<uint64_t>(time.dwHighDateTime) << 32) | time.dwLowDateTime;
    return (ticks - 116444736000000000ULL) / 10000;
}

bool token_sid(HANDLE process, std::vector<BYTE>& bytes) {
    Handle token;
    if (!OpenProcessToken(process, TOKEN_QUERY, &token.value)) return false;
    DWORD size = 0;
    GetTokenInformation(token.value, TokenUser, nullptr, 0, &size);
    if (size == 0 || size > 65536) return false;
    bytes.resize(size);
    return GetTokenInformation(token.value, TokenUser, bytes.data(), size, &size) != FALSE;
}

bool same_user(HANDLE process, PSID sid) {
    std::vector<BYTE> bytes;
    return token_sid(process, bytes) && EqualSid(reinterpret_cast<TOKEN_USER*>(bytes.data())->User.Sid, sid) != FALSE;
}

bool transfer(HANDLE pipe, void* bytes, DWORD size, bool write) {
    DWORD count = 0;
    const BOOL ok = write ? WriteFile(pipe, bytes, size, &count, nullptr) : ReadFile(pipe, bytes, size, &count, nullptr);
    return ok != FALSE && count == size;
}

bool event_frame(BYTE event, const Frame& frame) {
    auto output = GetStdHandle(STD_OUTPUT_HANDLE);
    return transfer(output, &event, 1, true) && transfer(output, const_cast<BYTE*>(frame.data()), static_cast<DWORD>(frame.size()), true);
}

struct Job {
    Handle handle;
    bool assigned = false;
    bool drained = false;
    bool finish() {
        if (!assigned) { drained = true; return true; }
        if (!TerminateJobObject(handle.value, 125)) return false;
        for (;;) {
            JOBOBJECT_BASIC_ACCOUNTING_INFORMATION info{};
            if (!QueryInformationJobObject(handle.value, JobObjectBasicAccountingInformation, &info, sizeof(info), nullptr)) return false;
            if (info.ActiveProcesses == 0) { drained = true; return true; }
            Sleep(10);
        }
    }
    ~Job() { if (assigned && !drained) finish(); }
};

int control() {
    DWORD available = 0;
    const auto input = GetStdHandle(STD_INPUT_HANDLE);
    if (!PeekNamedPipe(input, nullptr, 0, nullptr, &available, nullptr)) return -1;
    if (available == 0) return 0;
    BYTE byte = 0;
    if (!transfer(input, &byte, 1, false)) return -1;
    return byte;
}

bool alive(HANDLE owner, HANDLE target, uint64_t deadline) {
    return WaitForSingleObject(owner, 0) == WAIT_TIMEOUT && WaitForSingleObject(target, 0) == WAIT_TIMEOUT &&
        unix_ms() < deadline && control() == 0;
}

bool overlapped(HANDLE pipe, void* bytes, DWORD size, bool write, HANDLE owner, HANDLE target, uint64_t deadline) {
    Handle event;
    event.value = CreateEventW(nullptr, TRUE, FALSE, nullptr);
    if (event.value == nullptr) return false;
    OVERLAPPED operation{};
    operation.hEvent = event.value;
    DWORD count = 0;
    const BOOL immediate = write ? WriteFile(pipe, bytes, size, &count, &operation) : ReadFile(pipe, bytes, size, &count, &operation);
    if (immediate) return count == size;
    if (GetLastError() != ERROR_IO_PENDING) return false;
    while (alive(owner, target, deadline)) {
        if (WaitForSingleObject(event.value, 10) == WAIT_OBJECT_0) {
            return GetOverlappedResult(pipe, &operation, &count, FALSE) != FALSE && count == size;
        }
    }
    CancelIoEx(pipe, &operation);
    GetOverlappedResult(pipe, &operation, &count, TRUE);
    return false;
}

bool health(HANDLE pipe, Frame& frame, HANDLE owner, HANDLE target, PSID sid) {
    Handle event;
    event.value = CreateEventW(nullptr, TRUE, FALSE, nullptr);
    if (event.value == nullptr) return false;
    OVERLAPPED connect{};
    connect.hEvent = event.value;
    const BOOL connected = ConnectNamedPipe(pipe, &connect);
    const DWORD error = connected ? ERROR_SUCCESS : GetLastError();
    if (error == ERROR_IO_PENDING) {
        bool done = false;
        while (alive(owner, target, number(frame, 64))) {
            if (WaitForSingleObject(event.value, 10) == WAIT_OBJECT_0) { done = true; break; }
        }
        DWORD ignored = 0;
        if (!done) {
            CancelIoEx(pipe, &connect);
            GetOverlappedResult(pipe, &connect, &ignored, TRUE);
            return false;
        }
        if (!GetOverlappedResult(pipe, &connect, &ignored, FALSE)) return false;
    } else if (error != ERROR_SUCCESS && error != ERROR_PIPE_CONNECTED) return false;
    ULONG pid = 0;
    if (!GetNamedPipeClientProcessId(pipe, &pid) || pid != number(frame, 48) ||
        creation(target) != number(frame, 56) || !same_user(target, sid)) return false;
    if (!overlapped(pipe, frame.data(), static_cast<DWORD>(frame.size()), true, owner, target, number(frame, 64))) return false;
    Frame ack{};
    const bool accepted = overlapped(pipe, ack.data(), static_cast<DWORD>(ack.size()), false, owner, target, number(frame, 64)) && ack == frame && alive(owner, target, number(frame, 64));
    SecureZeroMemory(ack.data(), ack.size());
    return accepted;
}

int finish_host(Job& job, HANDLE target, Frame& frame) {
    if (!job.finish() || WaitForSingleObject(target, INFINITE) != WAIT_OBJECT_0) return 3;
    const bool observed = event_frame('E', frame);
    SecureZeroMemory(frame.data(), frame.size());
    return observed ? 0 : 3;
}

int host(const std::wstring& image) {
    Frame frame{};
    if (!transfer(GetStdHandle(STD_INPUT_HANDLE), frame.data(), static_cast<DWORD>(frame.size()), false) ||
        std::memcmp(frame.data(), "ZABHOST1", 8) != 0 || number(frame, 8) == 0 ||
        number(frame, 16) == 0 || number(frame, 16) > MAXDWORD || number(frame, 24) != 0 ||
        number(frame, 32) != 0 || number(frame, 40) != 0 || number(frame, 48) != 0 || number(frame, 56) != 0 ||
        number(frame, 64) <= unix_ms() || number(frame, 64) - unix_ms() > 120000) return 2;
    Handle owner;
    owner.value = OpenProcess(SYNCHRONIZE | PROCESS_QUERY_LIMITED_INFORMATION, FALSE, static_cast<DWORD>(number(frame, 16)));
    std::vector<BYTE> sid_bytes;
    if (owner.value == nullptr || !token_sid(GetCurrentProcess(), sid_bytes)) return 2;
    const PSID sid = reinterpret_cast<TOKEN_USER*>(sid_bytes.data())->User.Sid;
    if (!same_user(owner.value, sid) || creation(owner.value) == 0) return 2;
    put(frame, 24, creation(owner.value));
    put(frame, 32, GetCurrentProcessId());
    put(frame, 40, creation(GetCurrentProcess()));
    DWORD hash_size = 32;
    if (!CryptHashCertificate2(BCRYPT_SHA256_ALGORITHM, 0, nullptr, static_cast<const BYTE*>(sid), GetLengthSid(sid), frame.data() + 232, &hash_size)) return 2;
    LPWSTR sid_text = nullptr;
    if (!ConvertSidToStringSidW(sid, &sid_text)) return 2;
    const std::wstring dacl = L"D:P(A;;GA;;;" + std::wstring(sid_text) + L")";
    LocalFree(sid_text);
    PSECURITY_DESCRIPTOR descriptor = nullptr;
    if (!ConvertStringSecurityDescriptorToSecurityDescriptorW(dacl.c_str(), SDDL_REVISION_1, &descriptor, nullptr)) return 2;
    SECURITY_ATTRIBUTES security{sizeof(SECURITY_ATTRIBUTES), descriptor, FALSE};
    std::array<BYTE, 32> random{};
    if (BCryptGenRandom(nullptr, random.data(), static_cast<ULONG>(random.size()), BCRYPT_USE_SYSTEM_PREFERRED_RNG) != 0) { LocalFree(descriptor); return 2; }
    std::wstring pipe_name = L"\\\\.\\pipe\\zabor-native-health-";
    const wchar_t* digits = L"0123456789abcdef";
    for (BYTE byte : random) { pipe_name.push_back(digits[byte >> 4]); pipe_name.push_back(digits[byte & 15]); }
    SecureZeroMemory(random.data(), random.size());
    Handle pipe;
    pipe.value = CreateNamedPipeW(pipe_name.c_str(), PIPE_ACCESS_DUPLEX | FILE_FLAG_OVERLAPPED | FILE_FLAG_FIRST_PIPE_INSTANCE,
        PIPE_TYPE_MESSAGE | PIPE_READMODE_MESSAGE | PIPE_WAIT | PIPE_REJECT_REMOTE_CLIENTS, 1, 512, 512, 0, &security);
    LocalFree(descriptor);
    if (pipe.value == INVALID_HANDLE_VALUE) return 2;
    std::vector<std::unique_ptr<Handle>> image_chain;
    if (!path_chain(image, image_chain)) return 2;
    Handle held_image;
    held_image.value = CreateFileW(image.c_str(), GENERIC_READ, FILE_SHARE_READ, nullptr, OPEN_EXISTING, FILE_FLAG_OPEN_REPARSE_POINT, nullptr);
    if (held_image.value == INVALID_HANDLE_VALUE || image.find(L'"') != std::wstring::npos) return 2;
    Job job;
    job.handle.value = CreateJobObjectW(nullptr, nullptr);
    JOBOBJECT_EXTENDED_LIMIT_INFORMATION limits{};
    limits.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
    if (job.handle.value == nullptr || !SetInformationJobObject(job.handle.value, JobObjectExtendedLimitInformation, &limits, sizeof(limits))) return 2;
    SECURITY_ATTRIBUTES inherited{sizeof(SECURITY_ATTRIBUTES), nullptr, TRUE};
    Handle boot_read, boot_write, null_output;
    if (!CreatePipe(&boot_read.value, &boot_write.value, &inherited, 1024) || !SetHandleInformation(boot_write.value, HANDLE_FLAG_INHERIT, 0)) return 2;
    null_output.value = CreateFileW(L"NUL", GENERIC_WRITE, FILE_SHARE_READ | FILE_SHARE_WRITE, &inherited, OPEN_EXISTING, 0, nullptr);
    if (null_output.value == INVALID_HANDLE_VALUE) return 2;
    SIZE_T attribute_size = 0;
    InitializeProcThreadAttributeList(nullptr, 1, 0, &attribute_size);
    std::vector<BYTE> attributes(attribute_size);
    auto list = reinterpret_cast<LPPROC_THREAD_ATTRIBUTE_LIST>(attributes.data());
    if (!InitializeProcThreadAttributeList(list, 1, 0, &attribute_size)) return 2;
    HANDLE allowed[] = {boot_read.value, null_output.value};
    const BOOL configured = UpdateProcThreadAttribute(list, 0, PROC_THREAD_ATTRIBUTE_HANDLE_LIST, allowed, sizeof(allowed), nullptr, nullptr);
    STARTUPINFOEXW startup{};
    startup.StartupInfo.cb = sizeof(startup);
    startup.StartupInfo.dwFlags = STARTF_USESTDHANDLES;
    startup.StartupInfo.hStdInput = boot_read.value;
    startup.StartupInfo.hStdOutput = null_output.value;
    startup.StartupInfo.hStdError = null_output.value;
    startup.lpAttributeList = list;
    PROCESS_INFORMATION process{};
    std::wstring command = L"\"" + image + L"\" --native-update-health";
    const BOOL spawned = configured && CreateProcessW(image.c_str(), command.data(), nullptr, nullptr, TRUE,
        CREATE_SUSPENDED | EXTENDED_STARTUPINFO_PRESENT | CREATE_UNICODE_ENVIRONMENT, nullptr, nullptr, &startup.StartupInfo, &process);
    DeleteProcThreadAttributeList(list);
    if (!spawned) return 2;
    Handle target, thread;
    target.value = process.hProcess;
    thread.value = process.hThread;
    if (!AssignProcessToJobObject(job.handle.value, target.value)) {
        TerminateProcess(target.value, 125);
        WaitForSingleObject(target.value, INFINITE);
        return 2;
    }
    job.assigned = true;
    put(frame, 48, process.dwProcessId);
    put(frame, 56, creation(target.value));
    std::string narrow;
    for (wchar_t byte : pipe_name) {
        if (byte > 127) return 2;
        narrow.push_back(static_cast<char>(byte));
    }
    const uint16_t length = static_cast<uint16_t>(narrow.size());
    if (number(frame, 56) == 0 || !event_frame('S', frame) ||
        !transfer(boot_write.value, frame.data(), static_cast<DWORD>(frame.size()), true) ||
        !transfer(boot_write.value, const_cast<uint16_t*>(&length), sizeof(length), true) ||
        !transfer(boot_write.value, narrow.data(), length, true)) return finish_host(job, target.value, frame);
#ifdef ZABOR_HOST_TEST_ACQUISITION
    BYTE release = 0;
    if (!transfer(GetStdHandle(STD_INPUT_HANDLE), &release, 1, false) || release != 'R') return finish_host(job, target.value, frame);
#endif
    if (!alive(owner.value, target.value, number(frame, 64)) || ResumeThread(thread.value) == MAXDWORD) return finish_host(job, target.value, frame);
    CloseHandle(boot_write.value);
    boot_write.value = INVALID_HANDLE_VALUE;
    bool committed = false;
    if (health(pipe.value, frame, owner.value, target.value, sid) && event_frame('H', frame)) {
        while (WaitForSingleObject(owner.value, 0) == WAIT_TIMEOUT && WaitForSingleObject(target.value, 0) == WAIT_TIMEOUT && unix_ms() < number(frame, 64)) {
            const int value = control();
            if (value == 'C') { committed = event_frame('A', frame); break; }
            if (value != 0) break;
            Sleep(10);
        }
    }
    DisconnectNamedPipe(pipe.value);
    if (committed) {
        for (;;) {
            if (WaitForSingleObject(owner.value, 10) != WAIT_TIMEOUT || WaitForSingleObject(target.value, 0) != WAIT_TIMEOUT || control() != 0) break;
        }
    }
    return finish_host(job, target.value, frame);
}

int wmain(int argc, wchar_t** argv) {
    if (argc == 3 && std::wstring(argv[1]) == L"host") return host(argv[2]);
    return primitive_main(argc, argv);
}
