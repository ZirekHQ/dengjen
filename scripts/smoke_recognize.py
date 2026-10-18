import ctypes
import os
import sys

directory, lib_path = sys.argv[1], sys.argv[2]
if hasattr(os, "add_dll_directory"):
    os.add_dll_directory(directory)


class ExternError(ctypes.Structure):
    _fields_ = [("code", ctypes.c_int32), ("message", ctypes.c_void_p)]


stt = ctypes.CDLL(lib_path)
stt.libdengjenSttLoad.restype = ctypes.c_void_p
stt.libdengjenSttLoad.argtypes = [ctypes.c_char_p, ctypes.c_void_p, ctypes.c_char_p, ctypes.POINTER(ExternError)]
stt.libdengjenSttRecognize.restype = ctypes.c_void_p
stt.libdengjenSttRecognize.argtypes = [ctypes.c_void_p, ctypes.POINTER(ctypes.c_float), ctypes.c_size_t, ctypes.c_int32, ctypes.POINTER(ExternError)]
stt.libdengjenSttUnload.argtypes = [ctypes.c_void_p]
stt.libdengjenSttFreeString.argtypes = [ctypes.c_void_p]

err = ExternError()
handle = stt.libdengjenSttLoad(os.environb[b"DENGJEN_STT_MODEL_DIR"], None, os.environb[b"DENGJEN_STT_MODEL_SHA256"], ctypes.byref(err))
assert err.code == 0, f"load failed: code {err.code}"
samples = (ctypes.c_float * 16000)()
text = stt.libdengjenSttRecognize(handle, samples, 16000, 16000, ctypes.byref(err))
assert err.code == 0, f"recognize failed: code {err.code}"
if text:
    stt.libdengjenSttFreeString(text)
stt.libdengjenSttUnload(handle)
print("recognized silence without error")
