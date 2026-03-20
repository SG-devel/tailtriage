#!/usr/bin/env python3
"""Compatibility wrapper for downstream demo validation."""

from __future__ import annotations

import sys

from demo_tool import main


if __name__ == "__main__":
    main(["validate", "downstream", *sys.argv[1:]])
