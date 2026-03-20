#!/usr/bin/env python3
"""Compatibility wrapper for cold-start demo runs."""

from __future__ import annotations

import sys

from demo_tool import main


if __name__ == "__main__":
    main(["run", "cold-start", *sys.argv[1:]])
