#!/usr/bin/env python3
"""Overload control load for the trace-info visibility check (Issue #359).

One process per logical CPU spins on a cheap system call for a fixed time, so a
System Trace recording (which records BSD syscall kdebug events) produces far
more events than usual. It only tests whether the recorder surfaces lost
markers under pressure; it touches no framework code and no private SPI.

Usage: loss_control.py --seconds 10 [--processes N]
"""
from __future__ import annotations

import argparse
import multiprocessing
import os
import sys
import time


def _spin(deadline: float) -> None:
    getppid = os.getppid
    clock = time.monotonic
    while clock() < deadline:
        for _ in range(4096):
            getppid()


def main(argv: list[str]) -> int:
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument("--seconds", type=float, required=True)
    parser.add_argument("--processes", type=int, default=os.cpu_count() or 1)
    args = parser.parse_args(argv)
    if args.seconds <= 0 or args.processes <= 0:
        parser.error("--seconds and --processes must be positive")
    deadline = time.monotonic() + args.seconds
    workers = [multiprocessing.Process(target=_spin, args=(deadline,)) for _ in range(args.processes)]
    for worker in workers:
        worker.start()
    for worker in workers:
        worker.join()
    return max((worker.exitcode or 0) for worker in workers)


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
