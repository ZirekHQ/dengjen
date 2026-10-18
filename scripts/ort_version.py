"""Prints the version string an ONNX Runtime shared library reports about itself."""
import ctypes
import sys


def runtime_version(path: str) -> str:
    lib = ctypes.CDLL(path)
    lib.OrtGetApiBase.restype = ctypes.POINTER(ctypes.c_void_p * 2)
    base = lib.OrtGetApiBase()
    get_version = ctypes.CFUNCTYPE(ctypes.c_char_p)(base.contents[1])
    return get_version().decode()


if __name__ == "__main__":
    print(runtime_version(sys.argv[1]))
