import argparse
import ctypes
from ctypes import wintypes
import hashlib
import json
import os
from pathlib import Path
import re


class Guid(ctypes.Structure):
    _fields_ = [("data1", wintypes.DWORD), ("data2", wintypes.WORD), ("data3", wintypes.WORD),
                ("data4", ctypes.c_ubyte * 8)]


class FileInfo(ctypes.Structure):
    _fields_ = [("size", wintypes.DWORD), ("path", wintypes.LPCWSTR), ("file", wintypes.HANDLE),
                ("subject", ctypes.POINTER(Guid))]


class TrustData(ctypes.Structure):
    _fields_ = [("size", wintypes.DWORD), ("policy", ctypes.c_void_p), ("sip", ctypes.c_void_p),
                ("ui", wintypes.DWORD), ("revocation", wintypes.DWORD), ("choice", wintypes.DWORD),
                ("file_info", ctypes.POINTER(FileInfo)), ("action", wintypes.DWORD),
                ("state", wintypes.HANDLE), ("url", wintypes.LPCWSTR), ("flags", wintypes.DWORD),
                ("context", wintypes.DWORD), ("settings", ctypes.c_void_p)]


class CertificateContext(ctypes.Structure):
    _fields_ = [("encoding", wintypes.DWORD), ("bytes", ctypes.POINTER(ctypes.c_ubyte)),
                ("length", wintypes.DWORD), ("info", ctypes.c_void_p), ("store", wintypes.HANDLE)]


class SignatureSettings(ctypes.Structure):
    _fields_ = [("size", wintypes.DWORD), ("index", wintypes.DWORD), ("flags", wintypes.DWORD),
                ("secondary", wintypes.DWORD), ("verified", wintypes.DWORD), ("policy", ctypes.c_void_p)]


def verify_authenticode(path, certificate_sha256):
    if os.name != "nt" or not re.fullmatch(r"[0-9a-f]{64}", certificate_sha256) or certificate_sha256 == "0" * 64:
        raise ValueError("Windows and an explicit trusted publisher certificate SHA-256 are required")
    path = Path(path).resolve(strict=True)
    wintrust = ctypes.WinDLL("wintrust", use_last_error=True)
    crypto = ctypes.WinDLL("crypt32", use_last_error=True)
    wintrust.WinVerifyTrust.argtypes = [wintypes.HWND, ctypes.POINTER(Guid), ctypes.POINTER(TrustData)]
    wintrust.WinVerifyTrust.restype = wintypes.LONG
    action = Guid(0x00AAC56B, 0xCD44, 0x11D0, (ctypes.c_ubyte * 8)(0x8C, 0xC2, 0x00, 0xC0, 0x4F, 0xC2, 0x95, 0xEE))
    file_info = FileInfo(ctypes.sizeof(FileInfo), str(path), None, None)
    trust = TrustData()
    trust.size = ctypes.sizeof(TrustData)
    trust.ui = 2
    trust.revocation = 1
    trust.choice = 1
    trust.file_info = ctypes.pointer(file_info)
    trust.action = 1
    trust.flags = 0x80 | 0x1000 | 0x2000
    settings = SignatureSettings()
    settings.size = ctypes.sizeof(SignatureSettings)
    settings.index = 0
    settings.flags = 1
    trust.settings = ctypes.cast(ctypes.pointer(settings), ctypes.c_void_p)
    try:
        status = wintrust.WinVerifyTrust(wintypes.HWND(-1), ctypes.byref(action), ctypes.byref(trust))
        if status != 0:
            raise ValueError(f"Authenticode trust failed closed: 0x{status & 0xffffffff:08x}")
    finally:
        trust.action = 2
        wintrust.WinVerifyTrust(wintypes.HWND(-1), ctypes.byref(action), ctypes.byref(trust))

    crypto.CryptQueryObject.argtypes = [wintypes.DWORD, ctypes.c_void_p, wintypes.DWORD, wintypes.DWORD,
                                       wintypes.DWORD, ctypes.POINTER(wintypes.DWORD),
                                       ctypes.POINTER(wintypes.DWORD), ctypes.POINTER(wintypes.DWORD),
                                       ctypes.POINTER(wintypes.HANDLE), ctypes.POINTER(wintypes.HANDLE),
                                       ctypes.POINTER(ctypes.c_void_p)]
    crypto.CryptQueryObject.restype = wintypes.BOOL
    crypto.CryptMsgGetParam.argtypes = [wintypes.HANDLE, wintypes.DWORD, wintypes.DWORD,
                                       ctypes.c_void_p, ctypes.POINTER(wintypes.DWORD)]
    crypto.CryptMsgGetParam.restype = wintypes.BOOL
    crypto.CertFindCertificateInStore.argtypes = [wintypes.HANDLE, wintypes.DWORD, wintypes.DWORD,
                                                wintypes.DWORD, ctypes.c_void_p, ctypes.c_void_p]
    crypto.CertFindCertificateInStore.restype = ctypes.POINTER(CertificateContext)
    crypto.CertFreeCertificateContext.argtypes = [ctypes.POINTER(CertificateContext)]
    crypto.CertCloseStore.argtypes = [wintypes.HANDLE, wintypes.DWORD]
    crypto.CryptMsgClose.argtypes = [wintypes.HANDLE]
    store, message = wintypes.HANDLE(), wintypes.HANDLE()
    certificate = None
    path_buffer = ctypes.create_unicode_buffer(str(path))
    try:
        if not crypto.CryptQueryObject(1, ctypes.cast(path_buffer, ctypes.c_void_p), 0x400, 2, 0,
                                      None, None, None, ctypes.byref(store), ctypes.byref(message), None):
            raise ValueError("Embedded Authenticode signer unavailable")
        count, size = wintypes.DWORD(), wintypes.DWORD(ctypes.sizeof(wintypes.DWORD))
        if not crypto.CryptMsgGetParam(message, 5, 0, ctypes.byref(count), ctypes.byref(size)) or count.value != 1:
            raise ValueError("Exactly one embedded publisher signature required")
        size = wintypes.DWORD()
        if not crypto.CryptMsgGetParam(message, 7, 0, None, ctypes.byref(size)) or not 0 < size.value <= 65536:
            raise ValueError("Authenticode signer info unavailable")
        info = ctypes.create_string_buffer(size.value)
        if not crypto.CryptMsgGetParam(message, 7, 0, info, ctypes.byref(size)):
            raise ValueError("Authenticode signer info invalid")
        certificate = crypto.CertFindCertificateInStore(store, 0x10001, 0, 11 << 16, info, None)
        if not certificate:
            raise ValueError("Authenticode publisher certificate unavailable")
        encoded = ctypes.string_at(certificate.contents.bytes, certificate.contents.length)
        actual = hashlib.sha256(encoded).hexdigest()
        if actual != certificate_sha256:
            raise ValueError("Authenticode publisher certificate pin mismatch")
        return {"authenticode": "trusted", "certificateSha256": actual,
                "revocation": "whole-chain-excluding-root; cache-only; fail-closed"}
    finally:
        if certificate:
            crypto.CertFreeCertificateContext(certificate)
        if message:
            crypto.CryptMsgClose(message)
        if store:
            crypto.CertCloseStore(store, 0)


def main():
    parser = argparse.ArgumentParser(description="Offline fail-closed native Authenticode verifier; never signs")
    parser.add_argument("file", type=Path)
    parser.add_argument("--certificate-sha256", required=True)
    args = parser.parse_args()
    try:
        print(json.dumps(verify_authenticode(args.file, args.certificate_sha256), indent=2))
    except (ValueError, OSError) as error:
        parser.exit(1, str(error) + "\n")


if __name__ == "__main__":
    main()
