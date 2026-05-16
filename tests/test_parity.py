"""Byte-for-byte parity with stock CPython float output.

floatium-rs must produce output bit-identical to stock CPython for
`repr`, `str`, `float.__format__`, and `float(str)`. These tests
compare a floatium-rs-patched interpreter against a stock subprocess
across a corpus of values, every simple format spec, and both format
backends (std and zmij).
"""

from __future__ import annotations

import os
import struct
import subprocess
import sys

import pytest

import floatium_rs

# --- corpus -------------------------------------------------------------

_CURATED = [
    0.0, -0.0, 1.0, -1.0, 0.1, 0.2, 0.3, 1.5, 2.5, 100.0,
    3.141592653589793, 2.718281828459045,
    1e16, 1e17, 1e-4, 1e-5, 2.5e-5, 9.999999999999999e22,
    1e100, 1e-100, 1e308, 1e-308, 1.7976931348623157e308,
    5e-324, 2.2250738585072014e-308,
    123456789.0, 0.000123456789, 9007199254740992.0,
    0.30000000000000004, 1234567890.1234567,
    float("inf"), float("-inf"), float("nan"),
]


def _random_doubles(n: int, seed: int = 12345) -> list[float]:
    import random

    rng = random.Random(seed)
    out: list[float] = []
    while len(out) < n:
        bits = rng.getrandbits(64)
        x = struct.unpack("<d", struct.pack("<Q", bits))[0]
        if x == x and abs(x) != float("inf"):  # finite, non-nan
            out.append(x)
    return out


_CORPUS = _CURATED + _random_doubles(400)

_FORMAT_SPECS = [
    "", ".0f", ".2f", ".5f", ".10f",
    ".0e", ".2e", ".5e", "e", "E",
    "f", "F", "g", "G", ".3g", ".6g", ".10g",
    ".1f", ".17g",
]

_BACKENDS = ["std", "zmij"]


# --- stock baseline -----------------------------------------------------

def _expr(x: float) -> str:
    """A Python expression that reconstructs x *exactly* in a subprocess.

    repr() of inf/nan is not a valid literal, and decimal reprs can be
    ambiguous mid-development; float.fromhex(hex) is exact and total.
    """
    if x != x:
        return "float('nan')"
    if x == float("inf"):
        return "float('inf')"
    if x == float("-inf"):
        return "float('-inf')"
    return f"float.fromhex({x.hex()!r})"


def _stock(expr: str) -> str:
    env = os.environ.copy()
    env["FLOATIUM_RS_AUTOPATCH"] = "0"  # stock subprocess
    proc = subprocess.run(
        [sys.executable, "-c", f"import sys; sys.stdout.write({expr})"],
        env=env,
        capture_output=True,
        text=True,
        check=True,
    )
    return proc.stdout


# --- tests --------------------------------------------------------------

@pytest.mark.parametrize("backend", _BACKENDS)
def test_repr_parity(backend):
    floatium_rs.uninstall()
    floatium_rs.install(format_backend=backend)
    try:
        for x in _CORPUS:
            ours = repr(x)
            stock = _stock(f"repr({_expr(x)})")
            assert ours == stock, f"[{backend}] repr({x!r}): {ours!r} != {stock!r}"
    finally:
        floatium_rs.uninstall()


@pytest.mark.parametrize("backend", _BACKENDS)
def test_str_parity(backend):
    floatium_rs.uninstall()
    floatium_rs.install(format_backend=backend)
    try:
        for x in _CORPUS:
            ours = str(x)
            stock = _stock(f"str({_expr(x)})")
            assert ours == stock, f"[{backend}] str({x!r}): {ours!r} != {stock!r}"
    finally:
        floatium_rs.uninstall()


@pytest.mark.parametrize("backend", _BACKENDS)
def test_format_parity(backend):
    floatium_rs.uninstall()
    floatium_rs.install(format_backend=backend)
    try:
        for x in _CORPUS:
            for spec in _FORMAT_SPECS:
                ours = format(x, spec)
                stock = _stock(f"format({_expr(x)}, {spec!r})")
                assert ours == stock, (
                    f"[{backend}] format({x!r}, {spec!r}): {ours!r} != {stock!r}"
                )
    finally:
        floatium_rs.uninstall()


def test_parse_roundtrip():
    """float(repr(x)) == x for every finite double in the corpus."""
    with floatium_rs.enabled():
        for x in _CORPUS:
            if x != x or abs(x) == float("inf"):
                continue
            assert float(repr(x)) == x, f"roundtrip failed for {x!r}"


def test_parse_parity():
    """float(s) matches stock for a range of decimal strings."""
    strings = [
        "1.5", "0.1", "-2.5", "+3", "1e10", "1.7976931348623155e308",
        "5e-324", "0.30000000000000004", "  1.5  ", "1234567890.1234567",
        "0.0", "-0.0", "1e-300", "9.999e99",
    ]
    with floatium_rs.enabled():
        for s in strings:
            ours = float(s)
            stock = float(_stock(f"repr(float({s!r}))"))
            assert repr(ours) == repr(stock), f"float({s!r}): {ours!r} != {stock!r}"


def test_str_subclass_with_float_dunder():
    """A str subclass with __float__ must dispatch to __float__."""
    class FooStr(str):
        def __float__(self):
            return float(str(self)) + 1

    with floatium_rs.enabled():
        assert float(FooStr("8")) == 9.0
        assert float("8") == 8.0


def test_underscores_and_specials_fall_through():
    """Underscored / inf / nan literals still parse (via the original)."""
    with floatium_rs.enabled():
        assert float("1_000.5") == 1000.5
        assert float("inf") == float("inf")
        assert float("Infinity") == float("inf")
        v = float("nan")
        assert v != v
