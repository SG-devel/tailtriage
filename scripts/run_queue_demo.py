#!/usr/bin/env python3
"""Compatibility wrapper for queue demo runs."""

from __future__ import annotations

import sys

from demo_tool import main


if __name__ == "__main__":
    main(["run", "queue", *sys.argv[1:]])
