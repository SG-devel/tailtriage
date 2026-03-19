#!/usr/bin/env python3
"""Compatibility wrapper for downstream demo runs."""

from __future__ import annotations

import sys

from demo_tool import main


if __name__ == "__main__":
    argv = ["run", "downstream"]
    if len(sys.argv) > 1:
        argv.extend(["--artifact-path", *sys.argv[1:2]])
    main(argv)
