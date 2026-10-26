#!/usr/bin/env python3
"""Overload control load for the trace-info visibility check (Issue #359).

One process per logical CPU spins for a fixed time, so a recording produces far
more kdebug events than usual. It only tests whether the recorder surfaces lost
markers under pressure; it touches no framework code and no private SPI.

Modes:
  syscall  cheap system calls; floods recordings that trace BSD syscalls
           (System Trace), invisible to recordings that do not.
  kdebug   kdebug_trace() point events in DBG_APPS / DBG_APP_SIGNPOST code
           0x359; floods recordings that add the control load sink
           (control/v1, decode.py record --load-sink). Points of Interest
           does not record them.

Usage: loss_control.py --seconds 10 [--mode syscall|kdebug] [--processes N]
"""
from __future__ import annotations

import argparse
import multiprocessing
import os
import sys
import time

# APPSDBG_CODE(DBG_APP_SIGNPOST, code): class 0x21, subclass 0x0a, DBG_FUNC_NONE.
SIGNPOST_DEBUGID = (0x21 << 24) | (0x0A << 16) | (0x359 << 2)


def _spin_syscall(deadline: float) -> None:
    getppid = os.getppid
    clock = time.monotonic
    while clock() < deadline:
        for _ in range(4096):
            getppid()


def _spin_kdebug(deadline: float) -> None:
    import ctypes
    import ctypes.util

    trace = ctypes.CDLL(ctypes.util.find_library("c"), use_errno=True).kdebug_trace
    trace.argtypes = [ctypes.c_uint32, ctypes.c_uint64, ctypes.c_uint64, ctypes.c_uint64, ctypes.c_uint64]
    trace.restype = ctypes.c_int
    clock = time.monotonic
    while clock() < deadline:
        for i in range(4096):
            if trace(SIGNPOST_DEBUGID, i, 0, 0, 0) != 0:
                raise SystemExit(f"kdebug_trace failed: errno {ctypes.get_errno()}")


SPINS = {"syscall": _spin_syscall, "kdebug": _spin_kdebug}


def main(argv: list[str]) -> int:
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument("--seconds", type=float, required=True)
    parser.add_argument("--mode", choices=sorted(SPINS), default="syscall")
    parser.add_argument("--processes", type=int, default=os.cpu_count() or 1)
    args = parser.parse_args(argv)
    if args.seconds <= 0 or args.processes <= 0:
        parser.error("--seconds and --processes must be positive")
    deadline = time.monotonic() + args.seconds
    spin = SPINS[args.mode]
    workers = [multiprocessing.Process(target=spin, args=(deadline,)) for _ in range(args.processes)]
    for worker in workers:
        worker.start()
    for worker in workers:
        worker.join()
    # A worker killed by a signal has a negative exit code; only all-zero is success.
    return 0 if all(worker.exitcode == 0 for worker in workers) else 1


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
