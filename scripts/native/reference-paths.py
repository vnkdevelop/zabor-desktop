import ctypes
import json
import re
import sys

kernel = ctypes.WinDLL("kernel32", use_last_error=True)
attributes = kernel.GetFileAttributesW
attributes.argtypes = [ctypes.c_wchar_p]
attributes.restype = ctypes.c_uint32
paths = json.load(sys.stdin)
query = kernel.QueryDosDeviceW
query.argtypes = [ctypes.c_wchar_p, ctypes.c_wchar_p, ctypes.c_uint32]
query.restype = ctypes.c_uint32
for drive in {path[:2] for path in paths}:
    device = ctypes.create_unicode_buffer(32768)
    if not query(drive, device, len(device)):
        raise OSError(ctypes.get_last_error(), "Reference volume identity unavailable")
    if not re.fullmatch(r"\\Device\\HarddiskVolume[0-9]+", device.value):
        raise ValueError("Reference mapped/device volume rejected: " + drive)
for path in paths:
    flags = attributes(path)
    if flags == 0xFFFFFFFF:
        raise OSError(ctypes.get_last_error(), "Reference path attributes unavailable")
    if flags & 0x400:
        raise ValueError("Reference reparse point rejected: " + path)
