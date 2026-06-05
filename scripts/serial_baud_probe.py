#!/usr/bin/env python3
"""
Probe a USB-TTL serial console at multiple baud rates.

This is intentionally dependency-free for macOS. It configures the serial port
as 8N1/no-flow-control and uses the macOS IOSSIOSPEED ioctl for non-standard
rates such as 1500000.
"""

from __future__ import annotations

import argparse
import fcntl
import os
import select
import struct
import sys
import termios
import time
from dataclasses import dataclass


IOSSIOSPEED = 0x80045402  # macOS: _IOW('T', 2, speed_t)

DEFAULT_BAUDS = [
    1500000,
    1000000,
    921600,
    576000,
    500000,
    460800,
    230400,
    115200,
    57600,
    38400,
    9600,
]

BOOT_KEYWORDS = [
    b"U-Boot",
    b"DDR",
    b"Rockchip",
    b"rk35",
    b"RK35",
    b"Linux",
    b"Hit any key",
    b"serial",
    b"boot",
    b"Boot",
]


@dataclass
class ProbeResult:
    baud: int
    data: bytes
    printable_ratio: float
    keyword_hits: int

    @property
    def score(self) -> float:
        return self.printable_ratio + min(self.keyword_hits, 4) * 0.25


def parse_bauds(raw: str | None) -> list[int]:
    if not raw:
        return DEFAULT_BAUDS
    values: list[int] = []
    for part in raw.replace(",", " ").split():
        values.append(int(part))
    return values


def configure_serial(fd: int, baud: int) -> None:
    attrs = termios.tcgetattr(fd)

    # iflag, oflag, cflag, lflag, ispeed, ospeed, cc
    attrs[0] = 0
    attrs[1] = 0
    attrs[3] = 0

    cflag = attrs[2]
    cflag &= ~termios.CSIZE
    cflag |= termios.CS8 | termios.CREAD | termios.CLOCAL
    cflag &= ~termios.PARENB
    cflag &= ~termios.CSTOPB
    if hasattr(termios, "CRTSCTS"):
        cflag &= ~termios.CRTSCTS
    attrs[2] = cflag

    attrs[6][termios.VMIN] = 0
    attrs[6][termios.VTIME] = 1

    # Set a conventional base speed first, then override with IOSSIOSPEED.
    attrs[4] = getattr(termios, "B9600", 9600)
    attrs[5] = getattr(termios, "B9600", 9600)
    termios.tcsetattr(fd, termios.TCSANOW, attrs)
    fcntl.ioctl(fd, IOSSIOSPEED, struct.pack("I", baud))
    termios.tcflush(fd, termios.TCIOFLUSH)


def read_for(fd: int, seconds: float) -> bytes:
    deadline = time.monotonic() + seconds
    chunks: list[bytes] = []
    while time.monotonic() < deadline:
        timeout = max(0.0, min(0.2, deadline - time.monotonic()))
        readable, _, _ = select.select([fd], [], [], timeout)
        if not readable:
            continue
        try:
            chunk = os.read(fd, 4096)
        except BlockingIOError:
            continue
        if chunk:
            chunks.append(chunk)
    return b"".join(chunks)


def printable_ratio(data: bytes) -> float:
    if not data:
        return 0.0
    printable = 0
    for b in data:
        if b in (9, 10, 13) or 32 <= b <= 126:
            printable += 1
    return printable / len(data)


def keyword_hits(data: bytes) -> int:
    lowered = data.lower()
    return sum(1 for word in BOOT_KEYWORDS if word.lower() in lowered)


def sample_text(data: bytes, limit: int) -> str:
    if not data:
        return "<no data>"
    out: list[str] = []
    for b in data[:limit]:
        if b == 10:
            out.append("\\n")
        elif b == 13:
            out.append("\\r")
        elif b == 9:
            out.append("\\t")
        elif 32 <= b <= 126:
            out.append(chr(b))
        else:
            out.append(".")
    return "".join(out)


def probe_once(
    port: str,
    bauds: list[int],
    seconds: float,
    sample: int,
    prompt_each_baud: bool,
) -> list[ProbeResult]:
    fd = os.open(port, os.O_RDWR | os.O_NOCTTY | os.O_NONBLOCK)
    try:
        results: list[ProbeResult] = []
        for baud in bauds:
            configure_serial(fd, baud)
            if prompt_each_baud:
                input(f"\n{baud} baud is armed. Press RESET now, then Enter to capture...")
                termios.tcflush(fd, termios.TCIOFLUSH)
            data = read_for(fd, seconds)
            result = ProbeResult(
                baud=baud,
                data=data,
                printable_ratio=printable_ratio(data),
                keyword_hits=keyword_hits(data),
            )
            results.append(result)
            print(
                f"{baud:>8} baud | bytes={len(data):>5} | "
                f"printable={result.printable_ratio:0.2f} | "
                f"keywords={result.keyword_hits} | score={result.score:0.2f}"
            )
            print(f"  sample: {sample_text(data, sample)}")
        return results
    finally:
        os.close(fd)


def main() -> int:
    parser = argparse.ArgumentParser(description="Probe serial baud rates.")
    parser.add_argument("port", nargs="?", default="/dev/cu.usbserial-210")
    parser.add_argument("--bauds", help="Comma/space separated baud list.")
    parser.add_argument("--seconds", type=float, default=2.0, help="Read seconds per baud.")
    parser.add_argument("--sample", type=int, default=160, help="Sample characters to print.")
    parser.add_argument("--cycles", type=int, default=1, help="Repeat the baud list this many times.")
    parser.add_argument(
        "--prompt-reset",
        action="store_true",
        help="Pause before each cycle so you can press RESET or replug board power.",
    )
    parser.add_argument(
        "--prompt-each-baud",
        action="store_true",
        help="Pause before each baud rate; useful when the board only prints during boot.",
    )
    args = parser.parse_args()

    bauds = parse_bauds(args.bauds)
    best: ProbeResult | None = None

    print(f"port={args.port}")
    print(f"bauds={', '.join(str(b) for b in bauds)}")
    print("tip: for boot-only logs, press RESET right after a cycle starts.")

    for cycle in range(1, args.cycles + 1):
        if args.prompt_reset:
            input(f"\ncycle {cycle}/{args.cycles}: press RESET now, then Enter to scan...")
        else:
            print(f"\ncycle {cycle}/{args.cycles}")
        results = probe_once(args.port, bauds, args.seconds, args.sample, args.prompt_each_baud)
        for result in results:
            if best is None or result.score > best.score:
                best = result

    if best is not None:
        print(
            f"\nbest guess: {best.baud} baud "
            f"(bytes={len(best.data)}, printable={best.printable_ratio:0.2f}, "
            f"keywords={best.keyword_hits}, score={best.score:0.2f})"
        )
        if best.keyword_hits == 0 and best.printable_ratio < 0.65:
            print("warning: no clearly readable baud found; check GND/TX/RX and adapter support.")

    return 0


if __name__ == "__main__":
    raise SystemExit(main())
