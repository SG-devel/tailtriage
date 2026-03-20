#!/usr/bin/env python3
"""Compatibility wrapper for executor-pressure demo runs."""

from __future__ import annotations

import sys

from demo_tool import main


if __name__ == "__main__":
    main(["run", "executor", *sys.argv[1:]])
