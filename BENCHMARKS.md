# floatium-rs benchmarks

Nanoseconds per operation for `repr(float)`, fixed-precision f-string
formatting, and `float(str)`, comparing stock CPython against the two
floatium-rs format backends (`std` and `zmij`).

## Methodology

- **CPython:** 3.14.3 (the `floatium-rs/.venv` interpreter).
- **Extension:** `floatium-rs` built in release mode (`maturin develop
  --release`).
- **Date:** 2026-05-16.
- **Corpora:** the five canonical corpora from the sibling `floatium`
  package (`bench/corpora.py`): `random_uniform`, `random_bits`,
  `financial`, `scientific`, `integer_valued`. 2000 values each.
- **Timing:** mirrors `floatium/bench/bench_ns_per_op.py`. Each cell runs
  9 outer samples; each sample times 5 inner loops over the whole corpus.
  The reported figure is the **median of the fastest third** of the outer
  samples (3 of 9), which is robust to background jitter without picking
  the cold-cache outlier.
- **Variants:** `stock` = `floatium_rs.uninstall()` (native CPython
  formatter); `std` = `install(format_backend="std")`; `zmij` =
  `install(format_backend="zmij")`. Install/uninstall is toggled within a
  single process. The parse backend is always `std` (only one exists), so
  the `std` and `zmij` columns measure the same parse path.
- **Parse inputs:** the string list for `float(s)` is built with
  floatium-rs uninstalled, so the strings are stock CPython reprs.

## Results

| Corpus | Operation | Stock (ns) | std (ns) | zmij (ns) | std speedup | zmij speedup |
|---|---|---:|---:|---:|---:|---:|
| random_uniform | `repr(x)` | 288.1 | 208.9 | 112.5 | 1.38x | 2.56x |
| random_uniform | `f"{x:.4f}"` | 119.3 | 332.9 | 333.2 | 0.36x | 0.36x |
| random_uniform | `float(s)` | 126.0 | 45.0 | 45.6 | 2.80x | 2.77x |
| random_bits | `repr(x)` | 819.7 | 295.0 | 162.5 | 2.78x | 5.04x |
| random_bits | `f"{x:.4f}"` | 1,936.8 | 6,117.1 | 6,115.5 | 0.32x | 0.32x |
| random_bits | `float(s)` | 276.7 | 64.0 | 64.0 | 4.33x | 4.32x |
| financial | `repr(x)` | 170.6 | 176.3 | 86.7 | 0.97x | 1.97x |
| financial | `f"{x:.4f}"` | 144.4 | 449.7 | 449.4 | 0.32x | 0.32x |
| financial | `float(s)` | 36.6 | 38.0 | 38.3 | 0.96x | 0.95x |
| scientific | `repr(x)` | 634.4 | 280.8 | 159.2 | 2.26x | 4.00x |
| scientific | `f"{x:.4f}"` | 1,085.7 | 3,193.1 | 3,191.2 | 0.34x | 0.34x |
| scientific | `float(s)` | 213.7 | 62.1 | 61.2 | 3.44x | 3.49x |
| integer_valued | `repr(x)` | 145.2 | 169.9 | 98.7 | 0.85x | 1.47x |
| integer_valued | `f"{x:.4f}"` | 167.7 | 200.3 | 201.8 | 0.84x | 0.83x |
| integer_valued | `float(s)` | 42.9 | 41.7 | 41.5 | 1.03x | 1.03x |

Speedup is `stock / variant`; values above 1.00x are faster than stock.

