# coding: utf-8

import os

os.environ.setdefault(
    "DENGJEN_ESPEAKNG_DATA_DIRECTORY",
    os.path.abspath(os.path.dirname(__file__))
)

if hasattr(os, "add_dll_directory"):
    os.add_dll_directory(os.path.dirname(os.path.abspath(__file__)))

from .pydengjen import *

__doc__ = pydengjen.__doc__
if hasattr(pydengjen, "__all__"):
    __all__ = pydengjen.__all__
    