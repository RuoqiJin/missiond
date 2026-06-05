#!/usr/bin/env python3
"""
Open a macOS serial console and automatically send keys during boot.

Use this for U-Boot prompts that show "Hit any key to stop autoboot".
It configures the port as 8N1/no-flow-control with IOSSIOSPEED, prints incoming
bytes to stdout, and sends the requested break key repeatedly for the first few
seconds after arming.
"""

from __future__ import annotations

import argparse
import errno
import fcntl
import os
import select
import signal
import struct
import sys
import termios
import time
from datetime import datetime
from pathlib import Path


IOSSIOSPEED = 0x80045402


def configure_serial(fd: int, baud: int) -> None:
    attrs = termios.tcgetattr(fd)
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
    attrs[4] = getattr(termios, "B9600", 9600)
    attrs[5] = getattr(termios, "B9600", 9600)
    termios.tcsetattr(fd, termios.TCSANOW, attrs)
    fcntl.ioctl(fd, IOSSIOSPEED, struct.pack("I", baud))
    termios.tcflush(fd, termios.TCIOFLUSH)


def print_bytes(data: bytes) -> None:
    text = data.decode("utf-8", errors="replace")
    sys.stdout.write(text)
    sys.stdout.flush()


def resolve_log_path(value: str | None) -> Path | None:
    if value is None or value == "":
        return None
    if value == "auto":
        stamp = datetime.now().strftime("%Y%m%d-%H%M%S")
        return Path(f"/tmp/r5c-serial-{stamp}.log")
    return Path(value).expanduser()


def parse_key_sequence(value: str) -> bytes:
    aliases = {
        "ctrl-c": b"\x03",
        "^c": b"\x03",
        "ctrl-m": b"\r",
        "^m": b"\r",
        "enter": b"\r",
        "return": b"\r",
        "space": b" ",
        "esc": b"\x1b",
    }
    chunks: list[bytes] = []
    for part in value.split(","):
        token = part.strip().lower()
        if not token:
            continue
        if token.startswith("raw:"):
            chunks.append(token[4:].encode("utf-8"))
        elif token in aliases:
            chunks.append(aliases[token])
        else:
            chunks.append(part.encode("utf-8"))
    return b"".join(chunks) or b"\x03"


def main() -> int:
    parser = argparse.ArgumentParser(description="Auto-break into U-Boot over serial.")
    parser.add_argument("port", nargs="?", default="/dev/cu.usbserial-210")
    parser.add_argument("--baud", type=int, default=1500000)
    parser.add_argument("--spam-seconds", type=float, default=8.0)
    parser.add_argument("--spam-interval", type=float, default=0.05)
    parser.add_argument(
        "--arm-after-enter",
        action="store_true",
        help="Wait for local Enter before starting the key-spam timer. Useful before power-cycling a board.",
    )
    parser.add_argument(
        "--key",
        default="ctrl-c",
        help="Key sequence sent repeatedly. Aliases: ctrl-c, enter, space, ctrl-m, esc. Comma-separated is allowed.",
    )
    parser.add_argument(
        "--log-file",
        default=None,
        help="Write raw serial bytes to this file. Use 'auto' for /tmp/r5c-serial-<timestamp>.log.",
    )
    parser.add_argument(
        "--interactive",
        action="store_true",
        help="Forward local stdin lines to the serial port after arming. Newlines are sent as carriage returns.",
    )
    args = parser.parse_args()

    fd = os.open(args.port, os.O_RDWR | os.O_NOCTTY | os.O_NONBLOCK)
    log_path = resolve_log_path(args.log_file)
    log_file = None
    stop = False

    def handle_sigint(_signum: int, _frame: object) -> None:
        nonlocal stop
        stop = True

    signal.signal(signal.SIGINT, handle_sigint)

    try:
        if log_path is not None:
            log_path.parent.mkdir(parents=True, exist_ok=True)
            log_file = log_path.open("ab")
            header = (
                f"\n--- serial capture start {datetime.now().isoformat(timespec='seconds')} "
                f"port={args.port} baud={args.baud} key={args.key} ---\n"
            ).encode("utf-8")
            log_file.write(header)
            log_file.flush()
            print(f"logging raw serial bytes to {log_path}", flush=True)

        configure_serial(fd, args.baud)
        spam_bytes = parse_key_sequence(args.key)
        print(f"listening on {args.port} at {args.baud} 8N1", flush=True)
        if args.arm_after_enter:
            input(
                "Power off/reset the board, then press Enter here and immediately power it on... "
            )
        start = time.monotonic()
        next_send = start
        print(
            f"sending {args.key!r} for {args.spam_seconds:.1f}s. "
            "Power on or press RESET now.",
            flush=True,
        )

        stdin_fd = sys.stdin.fileno() if args.interactive and sys.stdin.isatty() else None
        if args.interactive and stdin_fd is None:
            print("stdin is not a TTY; interactive input disabled.", file=sys.stderr, flush=True)

        while not stop:
            now = time.monotonic()
            if now - start <= args.spam_seconds and now >= next_send:
                os.write(fd, spam_bytes)
                next_send = now + args.spam_interval

            read_fds = [fd]
            if stdin_fd is not None:
                read_fds.append(stdin_fd)
            readable, _, _ = select.select(read_fds, [], [], 0.1)

            if stdin_fd is not None and stdin_fd in readable:
                user_input = os.read(stdin_fd, 4096)
                if not user_input:
                    stop = True
                else:
                    os.write(fd, user_input.replace(b"\n", b"\r"))

            if fd in readable:
                try:
                    chunk = os.read(fd, 4096)
                except OSError as exc:
                    if exc.errno in (errno.ENXIO, errno.EIO, errno.ENODEV):
                        print(
                            "\nserial device disconnected or reset by macOS. "
                            "Unplug/replug the USB-TTL adapter and rerun the command.",
                            file=sys.stderr,
                            flush=True,
                        )
                        break
                    raise
                except BlockingIOError:
                    chunk = b""
                if chunk:
                    if log_file is not None:
                        log_file.write(chunk)
                        log_file.flush()
                    print_bytes(chunk)
    finally:
        if log_file is not None:
            footer = f"\n--- serial capture end {datetime.now().isoformat(timespec='seconds')} ---\n".encode(
                "utf-8"
            )
            log_file.write(footer)
            log_file.close()
        os.close(fd)
        print("\nserial closed", flush=True)

    return 0


if __name__ == "__main__":
    raise SystemExit(main())
