#!/usr/bin/env python3
"""Run an offline Lua check (no simulator or network is started).

Use --lua with a Lua 5.1 executable, or --dll with DCS's lua.dll on Windows.
Only run trusted Lua sources. This is a developer tool, never used by setup.
"""
import argparse
import ctypes
import os
from pathlib import Path
import subprocess


def run(source, dll=None, lua=None):
    if lua:
        subprocess.run([str(lua), '-'], input=source, text=True, check=True)
        return
    search = os.add_dll_directory(str(Path(dll).resolve().parent))
    runtime = ctypes.CDLL(str(Path(dll).resolve()))
    runtime.luaL_newstate.restype = ctypes.c_void_p
    runtime.luaL_openlibs.argtypes = [ctypes.c_void_p]
    runtime.luaL_loadbuffer.argtypes = [ctypes.c_void_p, ctypes.c_char_p, ctypes.c_size_t, ctypes.c_char_p]
    runtime.lua_pcall.argtypes = [ctypes.c_void_p, ctypes.c_int, ctypes.c_int, ctypes.c_int]
    runtime.lua_tolstring.argtypes = [ctypes.c_void_p, ctypes.c_int, ctypes.c_void_p]
    runtime.lua_tolstring.restype = ctypes.c_char_p
    runtime.lua_close.argtypes = [ctypes.c_void_p]
    state = runtime.luaL_newstate()
    try:
        assert state
        runtime.luaL_openlibs(state)
        data = source.encode('utf-8')
        result = runtime.luaL_loadbuffer(state, data, len(data), b'@osg-offline-check')
        if result == 0:
            result = runtime.lua_pcall(state, 0, 0, 0)
        if result:
            raise RuntimeError(runtime.lua_tolstring(state, -1, None).decode('utf-8', errors='replace'))
    finally:
        runtime.lua_close(state)
        search.close()


if __name__ == '__main__':
    parser = argparse.ArgumentParser(description=__doc__)
    runtime = parser.add_mutually_exclusive_group(required=True)
    runtime.add_argument('--lua', type=Path)
    runtime.add_argument('--dll', type=Path)
    parser.add_argument('script', type=Path)
    args = parser.parse_args()
    run(args.script.read_text(encoding='utf-8'), args.dll, args.lua)
