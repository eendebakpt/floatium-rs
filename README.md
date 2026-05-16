# floatium-rs

Rust-backed drop-in replacement for CPython's float formatting and
parsing — the Rust sibling of [floatium](https://github.com/eendebakpt/floatium)
(which uses C++/{fmt}).

After `pip install floatium-rs`, every subsequent Python process uses
Rust crates for `repr(float)` / `str(float)` / `float.__format__` and
`float("...")` instead of CPython's `dtoa.c`. Existing code, existing
tests, existing output — just faster. Works with an unmodified stock
CPython; no interpreter rebuild required.

## Backends

| Side | Backend | Implementation |
|------|---------|----------------|
| float → string | `zmij` (default) | [`zmij`](https://crates.io/crates/zmij) — Rust port of {fmt}'s Schubfach formatter (shortest); `std` for `%e`/`%f`/`%g` |
| float → string | `std` | Rust standard library formatting (`flt2dec`) for every mode |
| string → float | `std` | Rust `str::parse::<f64>()` — already Eisel–Lemire, correctly rounded |

All backends produce output bit-identical to stock CPython.

## Usage

```python
import floatium_rs
floatium_rs.install()           # patch PyFloat_Type slots
assert repr(0.1) == "0.1"
floatium_rs.uninstall()         # restore

with floatium_rs.enabled():     # scoped
    ...
```

Autopatch runs at interpreter startup by default. Opt out per
environment with `python -m floatium_rs disable`, or per process with
`FLOATIUM_RS_AUTOPATCH=0`.

> Do not autopatch both `floatium` and `floatium-rs` in the same
> environment — they both patch `PyFloat_Type`. Pick one.

## License

MIT. Bundles the Rust crates `pyo3` (MIT/Apache-2.0) and `zmij` (MIT).
The `format_short` logic is ported from CPython `Python/pystrtod.c`
(PSF License).
