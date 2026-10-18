import ctypes
import os
import unittest

LIBRARY = os.environ.get("DENGJEN_STT_LIBRARY")


class ExternError(ctypes.Structure):
    _fields_ = [("code", ctypes.c_int32), ("message", ctypes.c_void_p)]


@unittest.skipUnless(LIBRARY, "set DENGJEN_STT_LIBRARY to the built libdengjen_stt")
class CAbiTest(unittest.TestCase):
    def setUp(self):
        directory = os.path.dirname(os.path.abspath(LIBRARY))
        if hasattr(os, "add_dll_directory"):
            os.add_dll_directory(directory)
        self.lib = ctypes.CDLL(LIBRARY)
        self.lib.libdengjenSttAbiVersion.restype = ctypes.c_uint32
        self.lib.libdengjenSttLoad.restype = ctypes.c_void_p
        self.lib.libdengjenSttLoad.argtypes = [
            ctypes.c_char_p,
            ctypes.c_void_p,
            ctypes.c_char_p,
            ctypes.POINTER(ExternError),
        ]

    def test_abi_version(self):
        self.assertEqual(self.lib.libdengjenSttAbiVersion(), 1)

    def test_unreadable_model_reports_code_41(self):
        err = ExternError()
        handle = self.lib.libdengjenSttLoad(b"/definitely/not/here", None, b"0" * 64, ctypes.byref(err))
        self.assertFalse(handle)
        self.assertEqual(err.code, 41)
