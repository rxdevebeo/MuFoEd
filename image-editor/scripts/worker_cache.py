"""Process-local model/embedding cache controlled exclusively by the Rust host."""
import contextlib
import gc
import hashlib
import os
from pathlib import Path

MODELS = {}
EMBEDDINGS = {}
LOADS = 0
HITS = 0
EMBED_HITS = 0
PHASE_FILE = None


def stage(phase, detail):
    if PHASE_FILE is not None:
        import json
        pending = PHASE_FILE.with_suffix(".pending")
        pending.write_text(json.dumps({"phase": phase, "detail": detail}), encoding="utf-8")
        pending.replace(PHASE_FILE)


def key(path):
    path = Path(path).resolve()
    return (str(path), path.stat().st_size, path.stat().st_mtime_ns)


def cached(identity, load):
    global LOADS, HITS
    if identity in MODELS:
        HITS += 1
        return MODELS[identity]
    value = load()
    MODELS[identity] = value
    LOADS += 1
    return value


def offload():
    stage("offloading", "owned PyTorch models: GPU → CPU")
    import torch
    for value in MODELS.values():
        values = value if isinstance(value, tuple) else [value]
        for item in values:
            if hasattr(item, "upsampler"):
                item.upsampler.to("cpu")
            elif hasattr(item, "to"):
                item.to("cpu")
            elif hasattr(item, "get_providers") and item.get_providers()[0] != "CPUExecutionProvider":
                return False
    if torch.cuda.is_initialized():
        torch.cuda.synchronize()
        torch.cuda.empty_cache()
    gc.collect()
    return True


def metrics():
    # Actual process working set, not weight file size. No extra Python dependency.
    ram = None
    if os.name == "nt":
        import ctypes
        from ctypes import wintypes
        class Counters(ctypes.Structure):
            _fields_ = [("cb", wintypes.DWORD), ("PageFaultCount", wintypes.DWORD)] + [(name, ctypes.c_size_t) for name in ["PeakWorkingSetSize", "WorkingSetSize", "QuotaPeakPagedPoolUsage", "QuotaPagedPoolUsage", "QuotaPeakNonPagedPoolUsage", "QuotaNonPagedPoolUsage", "PagefileUsage", "PeakPagefileUsage"]]
        counter = Counters()
        counter.cb = ctypes.sizeof(counter)
        kernel = ctypes.WinDLL("kernel32")
        kernel.GetCurrentProcess.restype = wintypes.HANDLE
        psapi = ctypes.WinDLL("psapi")
        psapi.GetProcessMemoryInfo.argtypes = [wintypes.HANDLE, ctypes.POINTER(Counters), wintypes.DWORD]
        if psapi.GetProcessMemoryInfo(kernel.GetCurrentProcess(), ctypes.byref(counter), counter.cb):
            ram = counter.WorkingSetSize
    else:
        try:
            ram = int(Path("/proc/self/statm").read_text().split()[1]) * os.sysconf("SC_PAGE_SIZE")
        except (OSError, ValueError):
            pass
    vram = 0
    import sys
    if "torch" in sys.modules:
        import torch
        if torch.cuda.is_initialized():
            torch.cuda.synchronize()
            vram = torch.cuda.memory_reserved()
    if any(hasattr(value, "get_providers") and value.get_providers()[0] != "CPUExecutionProvider" for value in MODELS.values()):
        vram = None  # ORT has no per-session CUDA allocator measurement here.
    return {"ram": ram, "vram": vram, "loads": LOADS, "hits": HITS, "embedding_hits": EMBED_HITS, "pid": os.getpid()}


# Opt-in diagnostics returned in the existing worker acknowledgement.
TIMINGS = []

@contextlib.contextmanager
def timed(name):
    if not os.environ.get("IMAGE_STUDIO_TIMINGS"):
        yield
        return
    import time
    import sys
    torch = sys.modules.get("torch")
    def synchronize():
        if torch is not None and torch.cuda.is_initialized():
            torch.cuda.synchronize()
    synchronize()
    at = time.time_ns() // 1_000_000
    began = time.perf_counter()
    try:
        yield
    finally:
        synchronize()
        TIMINGS.append({"stage": name, "start_ms": at,
                        "elapsed_ms": (time.perf_counter() - began) * 1000,
                        "pid": os.getpid(), "source": "python"})
