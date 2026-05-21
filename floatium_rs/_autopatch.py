"""Autopatch hook — imported by floatium_rs.pth at interpreter startup.

Default behavior is to install floatium-rs. Opt out via:
  * env var: ``FLOATIUM_RS_AUTOPATCH=0`` (or false/no/off)
  * CLI:     ``python -m floatium_rs disable`` (writes a marker file)

The env var, when explicitly set, wins over the marker file.

Note: floatium-rs and its C++ sibling floatium both patch
``PyFloat_Type``. Do not autopatch both in the same environment — pick
one. ``FLOATIUM_RS_AUTOPATCH`` / the marker are independent of
floatium's own knobs.
"""

from __future__ import annotations

import os

_MARKER_NAME = "floatium-rs-autopatch.disabled"


def _env_override() -> bool | None:
    v = os.environ.get("FLOATIUM_RS_AUTOPATCH")
    if v is None:
        return None
    s = v.strip().lower()
    if s in {"1", "true", "yes", "on"}:
        return True
    if s in {"0", "false", "no", "off"}:
        return False
    return None


def _marker_present() -> bool:
    try:
        sp = os.path.dirname(os.path.dirname(os.path.realpath(__file__)))
        return os.path.isfile(os.path.join(sp, _MARKER_NAME))
    except Exception:  # noqa: BLE001 — never break startup
        return False


def _should_autopatch() -> bool:
    explicit = _env_override()
    if explicit is not None:
        return explicit
    return not _marker_present()


def _run() -> None:
    if not _should_autopatch():
        return
    try:
        from floatium_rs import install
    except ImportError:
        return
    fmt_backend = os.environ.get("FLOATIUM_RS_FORMAT_BACKEND") or None
    parse_backend = os.environ.get("FLOATIUM_RS_PARSE_BACKEND") or None
    try:
        install(format_backend=fmt_backend, parse_backend=parse_backend)
    except Exception:  # noqa: BLE001 — never break interpreter startup
        if os.environ.get("FLOATIUM_RS_AUTOPATCH_DEBUG", "").strip().lower() in {
            "1",
            "true",
            "yes",
            "on",
        }:
            import traceback

            traceback.print_exc()


_run()
