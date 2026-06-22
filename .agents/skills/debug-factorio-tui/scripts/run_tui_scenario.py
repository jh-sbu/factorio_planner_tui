#!/usr/bin/env python3
"""Run the Factorio Planner TUI in a PTY and print its final screen."""

from __future__ import annotations

import argparse
import fcntl
import os
import pty
import re
import select
import signal
import struct
import sys
import termios
import time


CSI_RE = re.compile(r"\x1b\[([0-?]*)([ -/]*)([@-~])")
KEYS = {
    "enter": b"\r",
    "esc": b"\x1b",
    "tab": b"\t",
    "backtab": b"\x1b[Z",
    "up": b"\x1b[A",
    "down": b"\x1b[B",
    "right": b"\x1b[C",
    "left": b"\x1b[D",
    "backspace": b"\x7f",
    "delete": b"\x1b[3~",
    "ctrl-c": b"\x03",
}


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument(
        "--binary", default="target/debug/factorio_planner_tui", help="binary to run"
    )
    parser.add_argument("--data-home", required=True, help="writable XDG_DATA_HOME")
    parser.add_argument("--key", action="append", default=[], help="named key or text:VALUE")
    parser.add_argument("--rows", type=int, default=40)
    parser.add_argument("--cols", type=int, default=120)
    parser.add_argument("--wait", type=float, default=0.35, help="seconds after each key")
    parser.add_argument("--startup-wait", type=float, default=1.0)
    return parser.parse_args()


def key_bytes(value: str) -> bytes:
    if value.startswith("text:"):
        return value[5:].encode()
    if value.lower() in KEYS:
        return KEYS[value.lower()]
    if len(value) == 1:
        return value.encode()
    raise ValueError(f"unknown key {value!r}; use a named key or text:VALUE")


def csi_numbers(parameters: str) -> list[int]:
    clean = parameters.lstrip("?")
    if not clean or re.fullmatch(r"[0-9;]*", clean) is None:
        return []
    return [int(value) if value else 0 for value in clean.split(";")]


def render_screen(data: bytes, rows: int, cols: int) -> str:
    text = data.decode("utf-8", "replace")
    grid = [[" "] * cols for _ in range(rows)]
    row = col = saved_row = saved_col = 0
    index = 0

    while index < len(text):
        match = CSI_RE.match(text, index)
        if match:
            numbers = csi_numbers(match.group(1))
            final = match.group(3)
            amount = numbers[0] if numbers and numbers[0] else 1
            if final in "Hf":
                row = (numbers[0] if numbers else 1) - 1
                col = (numbers[1] if len(numbers) > 1 else 1) - 1
            elif final == "G":
                col = amount - 1
            elif final == "d":
                row = amount - 1
            elif final == "A":
                row = max(0, row - amount)
            elif final == "B":
                row = min(rows - 1, row + amount)
            elif final == "C":
                col = min(cols - 1, col + amount)
            elif final == "D":
                col = max(0, col - amount)
            elif final == "s":
                saved_row, saved_col = row, col
            elif final == "u":
                row, col = saved_row, saved_col
            elif final == "J" and (not numbers or numbers[0] in (2, 3)):
                grid = [[" "] * cols for _ in range(rows)]
                row = col = 0
            elif final == "K":
                mode = numbers[0] if numbers else 0
                if mode == 0:
                    grid[row][col:] = [" "] * (cols - col)
                elif mode == 1:
                    grid[row][: col + 1] = [" "] * (col + 1)
                elif mode == 2:
                    grid[row] = [" "] * cols
            index = match.end()
            continue

        char = text[index]
        if char == "\x1b":
            if index + 1 < len(text) and text[index + 1] == "]":
                end = text.find("\x07", index + 2)
                index = len(text) if end < 0 else end + 1
            elif index + 1 < len(text) and text[index + 1] in "()":
                index += 3
            else:
                index += 2
            continue
        if char == "\r":
            col = 0
        elif char == "\n":
            row = min(rows - 1, row + 1)
        elif char == "\b":
            col = max(0, col - 1)
        elif ord(char) >= 32:
            if 0 <= row < rows and 0 <= col < cols:
                grid[row][col] = char
            col += 1
        index += 1

    return "\n".join("".join(line).rstrip() for line in grid).rstrip()


def main() -> int:
    args = parse_args()
    binary = os.path.abspath(args.binary)
    if not os.path.isfile(binary):
        print(f"binary not found: {binary}; run cargo build first", file=sys.stderr)
        return 2

    pid, fd = pty.fork()
    if pid == 0:
        os.environ["XDG_DATA_HOME"] = os.path.abspath(args.data_home)
        os.execv(binary, [binary])

    output = bytearray()
    query_count = 0

    def drain(duration: float) -> None:
        nonlocal query_count
        deadline = time.monotonic() + duration
        while time.monotonic() < deadline:
            readable, _, _ = select.select([fd], [], [], 0.05)
            if not readable:
                continue
            try:
                chunk = os.read(fd, 65536)
            except OSError:
                return
            output.extend(chunk)
            total_queries = output.count(b"\x1b[6n")
            while query_count < total_queries:
                os.write(fd, b"\x1b[1;1R")
                query_count += 1

    try:
        fcntl.ioctl(
            fd,
            termios.TIOCSWINSZ,
            struct.pack("HHHH", args.rows, args.cols, 0, 0),
        )
        drain(args.startup_wait)
        for value in args.key:
            os.write(fd, key_bytes(value))
            drain(args.wait)
        drain(args.wait)
        print(render_screen(bytes(output), args.rows, args.cols))
        return 0
    except (OSError, ValueError) as error:
        print(f"TUI scenario failed: {error}", file=sys.stderr)
        return 1
    finally:
        try:
            os.kill(pid, signal.SIGTERM)
        except ProcessLookupError:
            pass
        try:
            os.close(fd)
        except OSError:
            pass
        try:
            os.waitpid(pid, 0)
        except ChildProcessError:
            pass


if __name__ == "__main__":
    raise SystemExit(main())
