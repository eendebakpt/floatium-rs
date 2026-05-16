"""CLI for floatium-rs.

  python -m floatium_rs enable    Enable autopatch in this environment.
  python -m floatium_rs disable   Disable autopatch in this environment.
  python -m floatium_rs status    Report the current autopatch state.
  python -m floatium_rs info      Show backend / version info.

enable/disable toggle a marker file next to floatium-rs-autopatch.pth. With no
marker and no env-var override, autopatch is ON by default. The env var
FLOATIUM_RS_AUTOPATCH (0/1) wins over the marker.
"""

from __future__ import annotations

import argparse
import os
import site
import sys
from pathlib import Path

MARKER_NAME = "floatium-rs-autopatch.disabled"


def _find_pth_dir() -> Path:
    import floatium_rs

    pkg_dir = Path(floatium_rs.__file__).resolve().parent
    candidate = pkg_dir.parent
    if (candidate / "floatium-rs-autopatch.pth").is_file():
        return candidate
    candidates: list[str] = []
    try:
        candidates.extend(site.getsitepackages())
    except Exception:  # noqa: BLE001
        pass
    try:
        candidates.append(site.getusersitepackages())
    except Exception:  # noqa: BLE001
        pass
    for p in candidates:
        if p and (Path(p) / "floatium-rs-autopatch.pth").is_file():
            return Path(p)
    return candidate


def _marker_path() -> Path:
    return _find_pth_dir() / MARKER_NAME


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


def cmd_enable(_a: argparse.Namespace) -> int:
    m = _marker_path()
    if m.exists():
        try:
            m.unlink()
        except OSError as e:
            print(f"floatium-rs: failed to remove marker at {m}: {e}", file=sys.stderr)
            return 1
        print(f"floatium-rs autopatch: enabled (removed {m})")
    else:
        print(f"floatium-rs autopatch: already enabled (no marker at {m})")
    return 0


def cmd_disable(_a: argparse.Namespace) -> int:
    m = _marker_path()
    if m.exists():
        print(f"floatium-rs autopatch: already disabled (marker at {m})")
        return 0
    try:
        m.write_text("# Presence of this file disables floatium-rs autopatch.\n")
    except OSError as e:
        print(
            f"floatium-rs: failed to write marker at {m}: {e}\n"
            "  (try a venv, or set FLOATIUM_RS_AUTOPATCH=0 instead)",
            file=sys.stderr,
        )
        return 1
    print(f"floatium-rs autopatch: disabled (created {m})")
    return 0


def cmd_status(_a: argparse.Namespace) -> int:
    m = _marker_path()
    env = _env_override()
    marker = m.exists()
    if env is True:
        active, reason = True, "FLOATIUM_RS_AUTOPATCH set truthy"
    elif env is False:
        active, reason = False, "FLOATIUM_RS_AUTOPATCH set falsey"
    elif marker:
        active, reason = False, f"marker present at {m}"
    else:
        active, reason = True, "default (no marker, no env override)"
    print(
        f"floatium-rs autopatch: {'ENABLED' if active else 'DISABLED'}\n"
        f"  reason:        {reason}\n"
        f"  marker path:   {m}\n"
        f"  marker exists: {marker}\n"
        f"  env override:  {os.environ.get('FLOATIUM_RS_AUTOPATCH', '(unset)')}"
    )
    return 0


def cmd_info(_a: argparse.Namespace) -> int:
    import floatium_rs

    print(f"floatium-rs version: {floatium_rs.__version__}")
    for k, v in floatium_rs.info().items():
        print(f"  {k}: {v}")
    return 0


def main(argv: list[str] | None = None) -> int:
    p = argparse.ArgumentParser(
        prog="python -m floatium_rs",
        description="Manage floatium-rs autopatch state (ENABLED by default).",
        epilog="FLOATIUM_RS_AUTOPATCH=0/1 overrides the marker for one process.",
    )
    sub = p.add_subparsers(dest="cmd", metavar="{enable,disable,status,info}")
    sub.add_parser("enable", help="Enable autopatch.").set_defaults(fn=cmd_enable)
    sub.add_parser("disable", help="Disable autopatch.").set_defaults(fn=cmd_disable)
    sub.add_parser("status", help="Report autopatch state.").set_defaults(fn=cmd_status)
    sub.add_parser("info", help="Show backend / version info.").set_defaults(fn=cmd_info)
    args = p.parse_args(argv)
    if not hasattr(args, "fn"):
        p.print_help()
        return 0
    return args.fn(args)
