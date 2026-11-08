#define WIN32_LEAN_AND_MEAN
#define NOMINMAX
#include <windows.h>
#include <aclapi.h>
#include <sddl.h>
#include <wintrust.h>
#include <softpub.h>
#include <wincrypt.h>
#include <bcrypt.h>
#include <string>
#include <vector>
#include <memory>
#include <iostream>

struct Handle {
    HANDLE value = INVALID_HANDLE_VALUE;
    ~Handle() { if (value != INVALID_HANDLE_VALUE && value != nullptr) CloseHandle(value); }
};

bool leaf(const std::wstring& value) {
    if (value.empty() || value == L"." || value == L".." || value.back() == L'.') return false;
    for (wchar_t c : value) if (!((c >= L'a' && c <= L'z') || (c >= L'A' && c <= L'Z') ||
        (c >= L'0' && c <= L'9') || c == L'.' || c == L'-' || c == L'_')) return false;
    return true;
}

bool path_chain(const std::wstring& path, std::vector<std::unique_ptr<Handle>>& held) {
    if (path.size() < 3 || path[1] != L':' || path[2] != L'\\' || path.find(L"..") != std::wstring::npos) return false;
    for (size_t end = 3; end <= path.size(); ++end) {
        if (end != path.size() && path[end] != L'\\') continue;
        const auto part = path.substr(0, end);
        auto handle = std::make_unique<Handle>();
        handle->value = CreateFileW(part.c_str(), FILE_READ_ATTRIBUTES,
            FILE_SHARE_READ | FILE_SHARE_WRITE, nullptr, OPEN_EXISTING,
            FILE_FLAG_BACKUP_SEMANTICS | FILE_FLAG_OPEN_REPARSE_POINT, nullptr);
        if (handle->value == INVALID_HANDLE_VALUE) return false;
        FILE_ATTRIBUTE_TAG_INFO info{};
        if (!GetFileInformationByHandleEx(handle->value, FileAttributeTagInfo, &info, sizeof(info)) ||
            (info.FileAttributes & FILE_ATTRIBUTE_REPARSE_POINT) != 0) return false;
        held.push_back(std::move(handle));
    }
    return true;
}

bool user_sid(std::vector<BYTE>& storage) {
    Handle token;
    if (!OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &token.value)) return false;
    DWORD size = 0;
    GetTokenInformation(token.value, TokenUser, nullptr, 0, &size);
    if (size == 0) return false;
    storage.resize(size);
    return GetTokenInformation(token.value, TokenUser, storage.data(), size, &size) != FALSE;
}

bool root_access(const std::wstring& root, bool isolate) {
    std::vector<std::unique_ptr<Handle>> held;
    if (!path_chain(root, held)) return false;
    std::vector<BYTE> storage;
    if (!user_sid(storage)) return false;
    const PSID sid = reinterpret_cast<TOKEN_USER*>(storage.data())->User.Sid;
    PSECURITY_DESCRIPTOR descriptor = nullptr;
    PSID owner = nullptr;
    PACL acl = nullptr;
    const auto status = GetNamedSecurityInfoW(root.c_str(), SE_FILE_OBJECT,
        OWNER_SECURITY_INFORMATION | DACL_SECURITY_INFORMATION, &owner, nullptr, &acl, nullptr, &descriptor);
    if (status != ERROR_SUCCESS) return false;
    const bool owns = owner != nullptr && EqualSid(owner, sid) != FALSE;
    bool result = owns && acl != nullptr;
    if (result && isolate) {
        WIN32_FIND_DATAW data{};
        const HANDLE search = FindFirstFileW((root + L"\\*").c_str(), &data);
        if (search != INVALID_HANDLE_VALUE) {
            do { if (std::wstring(data.cFileName) != L"." && std::wstring(data.cFileName) != L"..") result = false; }
            while (FindNextFileW(search, &data));
            FindClose(search);
        }
        if (result) {
            EXPLICIT_ACCESSW access{};
            access.grfAccessPermissions = FILE_ALL_ACCESS;
            access.grfAccessMode = SET_ACCESS;
            access.grfInheritance = SUB_CONTAINERS_AND_OBJECTS_INHERIT;
            access.Trustee.TrusteeForm = TRUSTEE_IS_SID;
            access.Trustee.TrusteeType = TRUSTEE_IS_USER;
            access.Trustee.ptstrName = static_cast<LPWSTR>(sid);
            PACL replacement = nullptr;
            result = SetEntriesInAclW(1, &access, nullptr, &replacement) == ERROR_SUCCESS;
            if (result) result = SetNamedSecurityInfoW(const_cast<LPWSTR>(root.c_str()), SE_FILE_OBJECT,
                DACL_SECURITY_INFORMATION | PROTECTED_DACL_SECURITY_INFORMATION,
                nullptr, nullptr, replacement, nullptr) == ERROR_SUCCESS;
            if (replacement != nullptr) LocalFree(replacement);
        }
    } else if (result) {
        SECURITY_DESCRIPTOR_CONTROL control{};
        DWORD revision = 0;
        result = GetSecurityDescriptorControl(descriptor, &control, &revision) != FALSE &&
            (control & SE_DACL_PROTECTED) != 0;
        for (DWORD index = 0; result && index < acl->AceCount; ++index) {
            void* entry = nullptr;
            if (!GetAce(acl, index, &entry)) { result = false; break; }
            const auto header = static_cast<ACE_HEADER*>(entry);
            if (header->AceType == ACCESS_ALLOWED_ACE_TYPE) {
                const auto allowed = static_cast<ACCESS_ALLOWED_ACE*>(entry);
                const DWORD writes = FILE_WRITE_DATA | FILE_APPEND_DATA | FILE_WRITE_EA | FILE_WRITE_ATTRIBUTES |
                    FILE_DELETE_CHILD | DELETE | WRITE_DAC | WRITE_OWNER | GENERIC_WRITE | GENERIC_ALL;
                if ((allowed->Mask & writes) != 0 && !EqualSid(&allowed->SidStart, sid)) result = false;
            } else if (header->AceType != ACCESS_DENIED_ACE_TYPE) result = false;
        }
    }
    LocalFree(descriptor);
    return result;
}

bool signature(const std::wstring& path, const std::wstring& pin) {
    if (pin.size() != 64) return false;
    Handle image;
    image.value = CreateFileW(path.c_str(), GENERIC_READ, FILE_SHARE_READ, nullptr, OPEN_EXISTING,
        FILE_FLAG_OPEN_REPARSE_POINT, nullptr);
    if (image.value == INVALID_HANDLE_VALUE) return false;
    IMAGE_DOS_HEADER dos{};
    DWORD read = 0;
    LARGE_INTEGER image_size{};
    if (!GetFileSizeEx(image.value, &image_size) || image_size.QuadPart > 1024LL * 1024LL * 1024LL ||
        !ReadFile(image.value, &dos, sizeof(dos), &read, nullptr) || read != sizeof(dos) ||
        dos.e_magic != IMAGE_DOS_SIGNATURE || dos.e_lfanew < 0) return false;
    LARGE_INTEGER offset{};
    offset.QuadPart = dos.e_lfanew;
    IMAGE_NT_HEADERS64 headers{};
    if (!SetFilePointerEx(image.value, offset, nullptr, FILE_BEGIN) ||
        !ReadFile(image.value, &headers, sizeof(headers), &read, nullptr) || read != sizeof(headers) ||
        headers.Signature != IMAGE_NT_SIGNATURE || headers.FileHeader.Machine != IMAGE_FILE_MACHINE_AMD64 ||
        headers.OptionalHeader.Magic != IMAGE_NT_OPTIONAL_HDR64_MAGIC ||
        headers.OptionalHeader.DataDirectory[IMAGE_DIRECTORY_ENTRY_COM_DESCRIPTOR].VirtualAddress != 0 ||
        headers.OptionalHeader.DataDirectory[IMAGE_DIRECTORY_ENTRY_COM_DESCRIPTOR].Size != 0) return false;
    WINTRUST_FILE_INFO file{};
    file.cbStruct = sizeof(file);
    file.pcwszFilePath = path.c_str();
    WINTRUST_SIGNATURE_SETTINGS settings{};
    settings.cbStruct = sizeof(settings);
    settings.dwFlags = WSS_VERIFY_SPECIFIC;
    WINTRUST_DATA trust{};
    trust.cbStruct = sizeof(trust);
    trust.dwUIChoice = WTD_UI_NONE;
    trust.fdwRevocationChecks = WTD_REVOKE_WHOLECHAIN;
    trust.dwUnionChoice = WTD_CHOICE_FILE;
    trust.pFile = &file;
    trust.dwStateAction = WTD_STATEACTION_VERIFY;
    trust.dwProvFlags = WTD_REVOCATION_CHECK_CHAIN_EXCLUDE_ROOT | WTD_CACHE_ONLY_URL_RETRIEVAL | WTD_DISABLE_MD2_MD4;
    trust.pSignatureSettings = &settings;
    GUID action = WINTRUST_ACTION_GENERIC_VERIFY_V2;
    const LONG status = WinVerifyTrust(nullptr, &action, &trust);
    trust.dwStateAction = WTD_STATEACTION_CLOSE;
    WinVerifyTrust(nullptr, &action, &trust);
    if (status != ERROR_SUCCESS) return false;
    HCERTSTORE store = nullptr;
    HCRYPTMSG message = nullptr;
    if (!CryptQueryObject(CERT_QUERY_OBJECT_FILE, path.c_str(), CERT_QUERY_CONTENT_FLAG_PKCS7_SIGNED_EMBED,
        CERT_QUERY_FORMAT_FLAG_BINARY, 0, nullptr, nullptr, nullptr, &store, &message, nullptr)) return false;
    bool result = false;
    DWORD count = 0, length = sizeof(count);
    if (CryptMsgGetParam(message, CMSG_SIGNER_COUNT_PARAM, 0, &count, &length) && count == 1) {
        length = 0;
        if (CryptMsgGetParam(message, CMSG_SIGNER_CERT_INFO_PARAM, 0, nullptr, &length) && length <= 65536) {
            std::vector<BYTE> info(length);
            if (CryptMsgGetParam(message, CMSG_SIGNER_CERT_INFO_PARAM, 0, info.data(), &length)) {
                const PCCERT_CONTEXT certificate = CertFindCertificateInStore(store,
                    X509_ASN_ENCODING | PKCS_7_ASN_ENCODING, 0, CERT_FIND_SUBJECT_CERT, info.data(), nullptr);
                if (certificate != nullptr) {
                    BYTE hash[32]{};
                    DWORD size = sizeof(hash);
                    if (CryptHashCertificate2(BCRYPT_SHA256_ALGORITHM, 0, nullptr, certificate->pbCertEncoded,
                        certificate->cbCertEncoded, hash, &size)) {
                        const wchar_t* digits = L"0123456789abcdef";
                        std::wstring actual;
                        for (BYTE byte : hash) { actual.push_back(digits[byte >> 4]); actual.push_back(digits[byte & 15]); }
                        result = actual == pin;
                    }
                    CertFreeCertificateContext(certificate);
                }
            }
        }
    }
    CryptMsgClose(message);
    CertCloseStore(store, 0);
    return result;
}

int wmain(int argc, wchar_t** argv) {
    if (argc < 3) return 2;
    const std::wstring verb = argv[1], path = argv[2];
    bool result = false;
    if (verb == L"root-access" && argc == 3) result = root_access(path, false);
    else if (verb == L"isolate-empty-root" && argc == 3) result = root_access(path, true);
    else if (verb == L"verify-pe" && argc == 4) {
        std::vector<std::unique_ptr<Handle>> held;
        result = path_chain(path, held) && signature(path, argv[3]);
    } else if (verb == L"replace" && argc == 5 && leaf(argv[3]) && leaf(argv[4])) {
        std::vector<std::unique_ptr<Handle>> held;
        if (root_access(path, false) && path_chain(path, held)) {
            const auto source = path + L"\\" + argv[3], target = path + L"\\" + argv[4];
            const DWORD source_attributes = GetFileAttributesW(source.c_str());
            const DWORD target_attributes = GetFileAttributesW(target.c_str());
            if (source_attributes != INVALID_FILE_ATTRIBUTES &&
                (source_attributes & (FILE_ATTRIBUTE_DIRECTORY | FILE_ATTRIBUTE_REPARSE_POINT)) == 0 &&
                (target_attributes == INVALID_FILE_ATTRIBUTES ||
                (target_attributes & (FILE_ATTRIBUTE_DIRECTORY | FILE_ATTRIBUTE_REPARSE_POINT)) == 0)) {
                result = MoveFileExW(source.c_str(), target.c_str(), MOVEFILE_REPLACE_EXISTING | MOVEFILE_WRITE_THROUGH) != FALSE;
            }
        }
    }
    std::cout << (result ? "ok" : "closed") << std::endl;
    return result ? 0 : 1;
}
