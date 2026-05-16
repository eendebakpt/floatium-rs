"""Entrypoint for python -m floatium_rs."""

from __future__ import annotations

import sys

from floatium_rs._cli import main

if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
