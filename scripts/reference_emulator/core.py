"""Small ctypes binding to the pinned Mupen64Plus public ABI, not game logic.

Calls execute a separately built trusted emulator in a bounded child process.
No debugger memory-write, cheat, or ROM-patching API is bound.
"""
from __future__ import annotations

import ctypes as C
from pathlib import Path
import struct
import sys

VOID = C.CFUNCTYPE(None)
UPDATE = C.CFUNCTYPE(None, C.c_uint32)
LOG = C.CFUNCTYPE(None, C.c_void_p, C.c_int, C.c_char_p)
STATE = C.CFUNCTYPE(None, C.c_void_p, C.c_int, C.c_int)
POLL = C.CFUNCTYPE(None, C.c_uint64)


class Breakpoint(C.Structure):
    _fields_ = [("address", C.c_uint32), ("end", C.c_uint32), ("flags", C.c_uint32)]


class Core:
    def __init__(self, library: Path, rsp: Path, bridge: Path, video: Path):
        self.lib = C.CDLL(str(library))
        self.rsp = C.CDLL(str(rsp))
        self.bridge = C.CDLL(str(bridge))
        self.video = C.CDLL(str(video))
        self.keepalive: list[object] = []
        self.started = False
        self.opened = False
        self.attached: list[int] = []
        self.plugins: list[C.CDLL] = []
        self.callback_error: BaseException | None = None
        self._bind("CoreStartup", C.c_int, C.c_int, C.c_char_p, C.c_char_p,
                   C.c_void_p, LOG, C.c_void_p, STATE)
        self._bind("CoreShutdown", C.c_int)
        self._bind("CoreDoCommand", C.c_int, C.c_int, C.c_int, C.c_void_p)
        self._bind("CoreAttachPlugin", C.c_int, C.c_int, C.c_void_p)
        self._bind("CoreDetachPlugin", C.c_int, C.c_int)
        self._bind("CoreErrorMessage", C.c_char_p, C.c_int)
        self._bind("ConfigOpenSection", C.c_int, C.c_char_p, C.POINTER(C.c_void_p))
        self._bind("ConfigSetParameter", C.c_int, C.c_void_p, C.c_char_p, C.c_int, C.c_void_p)
        self._bind("DebugSetCallbacks", C.c_int, VOID, UPDATE, VOID)
        self._bind("DebugSetRunState", C.c_int, C.c_int)
        self._bind("DebugBreakpointCommand", C.c_int, C.c_int, C.c_uint, C.c_void_p)
        self._bind("DebugMemRead32", C.c_uint32, C.c_uint32)
        self._bind("DebugMemRead16", C.c_uint16, C.c_uint32)
        self._bind("DebugMemRead8", C.c_uint8, C.c_uint32)
        self._bind("DebugGetCPUDataPtr", C.c_void_p, C.c_int)
        self.bridge.CaptureSetInput.argtypes = [C.c_uint16, C.c_int8, C.c_int8]
        self.bridge.CaptureSetInput.restype = None
        self.bridge.CaptureSetPollHook.argtypes = [POLL]
        self.bridge.CaptureSetPollHook.restype = None
        self.bridge.CapturePollCount.argtypes = []
        self.bridge.CapturePollCount.restype = C.c_uint64

    def _bind(self, name, result, *args):
        function = getattr(self.lib, name)
        function.restype = result
        function.argtypes = list(args)

    def check(self, code: int, operation: str):
        if code != 0:
            raise RuntimeError(f"{operation}: {self.lib.CoreErrorMessage(code)!r} ({code})")

    def command(self, command: int, integer=0, pointer=None):
        self.check(self.lib.CoreDoCommand(command, integer, pointer), f"command {command}")

    def parameter(self, section: bytes, name: bytes, value):
        handle = C.c_void_p()
        self.check(self.lib.ConfigOpenSection(section, C.byref(handle)), "open config section")
        if isinstance(value, bool):
            kind, stored = 3, C.c_int(value)
        elif isinstance(value, int):
            kind, stored = 1, C.c_int(value)
        else:
            kind, stored = 4, C.create_string_buffer(str(value).encode())
        self.check(self.lib.ConfigSetParameter(handle, name, kind, C.byref(stored)), "set config")

    def startup(self, directory: Path, data: Path, rom: bytes, logger):
        directory.mkdir()
        (directory / "saves").mkdir()
        log = LOG(lambda context, level, message: logger(level, message.decode(errors="replace")))
        state = STATE(lambda context, parameter, value: None)
        self.keepalive.extend([log, state])
        self.check(self.lib.CoreStartup(0x020106, str(directory).encode(), str(data).encode(),
                                       None, log, None, state), "core startup")
        self.started = True
        for name, value in {
            b"R4300Emulator": 1, b"EnableDebugger": True, b"OnScreenDisplay": False,
            b"SaveSRAMPath": str(directory / "saves"),
            b"SaveStatePath": str(directory / "saves"),
            b"ScreenshotPath": str(directory), b"SharedDataPath": str(data),
            b"RandomizeInterrupt": False,
        }.items():
            self.parameter(b"Core", name, value)
        buffer = C.create_string_buffer(rom)
        self.keepalive.append(buffer)
        self.command(1, len(rom), buffer)
        self.opened = True
        for name, value in {b"Fullscreen": False, b"ScreenWidth": 320,
                            b"ScreenHeight": 240, b"VerticalSync": False}.items():
            self.parameter(b"Video-General", name, value)
        # Real GFX completion is essential: the dummy GFX plugin stalls SM64's
        # task pipeline. Audio output is discarded; HLE still processes tasks.
        for kind, plugin in [(2, self.video), (3, None), (4, self.bridge), (1, self.rsp)]:
            if plugin:
                plugin.PluginStartup.argtypes = [C.c_void_p, C.c_void_p, LOG]
                plugin.PluginStartup.restype = C.c_int
                plugin.PluginShutdown.argtypes = []
                plugin.PluginShutdown.restype = C.c_int
                self.check(plugin.PluginStartup(self.lib._handle, None, log), "plugin startup")
                self.plugins.append(plugin)
            self.check(self.lib.CoreAttachPlugin(kind, plugin._handle if plugin else None), "attach plugin")
            self.attached.append(kind)

    def guarded(self, callback):
        # Exceptions must never escape a ctypes callback (ctypes would ignore them).
        def wrapper(*args):
            if self.callback_error is not None:
                self.lib.CoreDoCommand(6, 0, None)
                self.lib.DebugSetRunState(2)
                return
            try:
                callback(*args)
            except BaseException as error:
                self.callback_error = error
                print(f"capture callback failed: {error}", file=sys.stderr, flush=True)
                self.lib.CoreDoCommand(6, 0, None)
                self.lib.DebugSetRunState(2)
        return wrapper

    def callbacks(self, init, update, vi, poll):
        callbacks = [VOID(self.guarded(init)), UPDATE(self.guarded(update)),
                     VOID(self.guarded(vi)), POLL(self.guarded(poll))]
        self.keepalive.extend(callbacks)
        self.check(self.lib.DebugSetCallbacks(*callbacks[:3]), "debug callbacks")
        self.bridge.CaptureSetPollHook(callbacks[3])

    def breakpoint(self, address: int) -> int:
        breakpoint = Breakpoint(address, address, 0x09)
        index = self.lib.DebugBreakpointCommand(2, 0, C.byref(breakpoint))
        if index < 0:
            raise RuntimeError("could not add execution breakpoint")
        return index

    def remove_breakpoint(self, index: int):
        if self.lib.DebugBreakpointCommand(5, index, None) < 0:
            raise RuntimeError("could not remove execution breakpoint")

    def resume(self):
        self.check(self.lib.DebugSetRunState(2), "resume debugger")

    def stop(self):
        self.command(6)
        self.resume()

    def execute(self):
        self.command(5)
        if self.callback_error is not None:
            raise RuntimeError(f"capture callback failed: {self.callback_error}") from self.callback_error

    def register(self, index: int) -> int:
        pointer = self.lib.DebugGetCPUDataPtr(2)
        if not pointer:
            raise RuntimeError("CPU registers unavailable")
        return C.cast(pointer, C.POINTER(C.c_uint64))[index]

    def pc(self) -> int:
        pointer = self.lib.DebugGetCPUDataPtr(1)
        if not pointer:
            raise RuntimeError("CPU PC unavailable")
        return C.cast(pointer, C.POINTER(C.c_uint32))[0]

    def u32(self, address):
        return int(self.lib.DebugMemRead32(address))

    def f32(self, address):
        return struct.unpack(">f", struct.pack(">I", self.u32(address)))[0]

    def i16(self, address):
        value = int(self.lib.DebugMemRead16(address))
        return value - 65536 if value >= 32768 else value

    def close(self):
        # Keep all callbacks alive until the stopped core has detached its plugins.
        for kind in reversed(self.attached):
            self.check(self.lib.CoreDetachPlugin(kind), "detach plugin")
        self.attached.clear()
        for plugin in reversed(self.plugins):
            self.check(plugin.PluginShutdown(), "plugin shutdown")
        self.plugins.clear()
        if self.opened:
            self.command(2)
            self.opened = False
        if self.started:
            self.check(self.lib.CoreShutdown(), "core shutdown")
            self.started = False
