"""floatium-rs — Rust-backed drop-in replacement for CPython float I/O.

Sibling of `floatium` (the C++/{fmt} package): same user contract, but
the conversion core is Rust — `zmij` (a port of {fmt}'s Schubfach
formatter) and the Rust standard library.

Basic usage::

    import floatium_rs
    floatium_rs.install()           # patch PyFloat_Type slots
    repr(0.1)                       # -> '0.1', same as stock
    floatium_rs.uninstall()         # restore

    with floatium_rs.enabled():         # scoped patching
        ...
    with floatium_rs.enabled(False):    # scoped UN-patching
        ...

Autopatch runs at interpreter startup by default. Opt out per
environment with ``python -m floatium_rs disable``, or temporarily with
``FLOATIUM_RS_AUTOPATCH=0``.
"""

from __future__ import annotations

from contextlib import contextmanager
from typing import Iterator

from floatium_rs import _ext

__all__ = [
    "install",
    "uninstall",
    "is_patched",
    "info",
    "enabled",
    "__version__",
]

__version__ = "0.1.0"


def install(
    format_backend: str | None = None,
    parse_backend: str | None = None,
) -> None:
    """Install floatium-rs's replacement slots on PyFloat_Type.

    ``format_backend``: ``"std"`` or ``"zmij"`` (default ``"zmij"``).
    ``parse_backend``:  ``"std"``.

    Idempotent: a second call while already installed is a no-op.
    """
    kwargs = {}
    if format_backend is not None:
        kwargs["format_backend"] = format_backend
    if parse_backend is not None:
        kwargs["parse_backend"] = parse_backend
    _ext.install(**kwargs)


def uninstall() -> None:
    """Restore the original PyFloat_Type slots."""
    _ext.uninstall()


def is_patched() -> bool:
    """Return True if floatium-rs is currently installed."""
    return bool(_ext.is_patched())


def info() -> dict:
    """Return a dict describing current state and available backends."""
    return _ext.info()


@contextmanager
def enabled(
    active: bool = True,
    format_backend: str | None = None,
    parse_backend: str | None = None,
) -> Iterator[None]:
    """Scoped patching / unpatching, restoring the entry state on exit.

    ``enabled(True)`` (default) ensures floatium-rs is installed within
    the block; ``enabled(False)`` ensures it is *not*. Either way the
    state at block entry is restored on exit.
    """
    was_patched = is_patched()
    if active and not was_patched:
        install(format_backend=format_backend, parse_backend=parse_backend)
    elif not active and was_patched:
        uninstall()
    try:
        yield
    finally:
        if was_patched and not is_patched():
            install(format_backend=format_backend, parse_backend=parse_backend)
        elif not was_patched and is_patched():
            uninstall()
