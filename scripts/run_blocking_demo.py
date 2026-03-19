#!/usr/bin/env python3
"""Compatibility wrapper for blocking demo runs."""

from __future__ import annotations

import sys

from demo_tool import main


if __name__ == "__main__":
    main(["run", "blocking", *sys.argv[1:]])
