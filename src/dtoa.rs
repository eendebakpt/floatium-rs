//! Digit generation + string parsing.
//!
//! `Dtoa` is the digits/decpt/sign contract that mirrors `_Py_dg_dtoa`:
//! the value is `0.<digits> * 10^decpt`, digits has no sign / dot /
//! exponent and no leading or trailing zeros. `format_short` packages a
//! `Dtoa` into CPython's exact textual form.
//!
//! Three dtoa modes, matching CPython:
//!   mode 0 — shortest round-trip            (drives `repr`)
//!   mode 2 — N significant digits           (drives `%e` / `%g`)
//!   mode 3 — N digits after the point       (drives `%f`)
//!
//! Both format backends produce the same `Dtoa` contract, so output is
//! bit-identical regardless of which is active.

/// Result of a double -> digits conversion.
pub enum Dtoa {
    /// Finite value. `digits` is ASCII, no leading/trailing zeros
    /// (may be empty when a fixed-mode value rounds to zero).
    Finite {
        digits: Vec<u8>,
        decpt: i32,
        sign: bool,
    },
    Inf {
        sign: bool,
    },
    Nan,
}

/// Format backend selector.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum FmtBackend {
    /// Rust `std` formatting (`flt2dec`) for every mode.
    Std,
    /// zmij (Schubfach, a port of {fmt}'s formatter) for shortest;
    /// `std` for the fixed / scientific modes zmij does not expose.
    Zmij,
}

impl FmtBackend {
    pub fn name(self) -> &'static str {
        match self {
            FmtBackend::Std => "std",
            FmtBackend::Zmij => "zmij",
        }
    }

    pub fn from_name(s: &str) -> Option<FmtBackend> {
        match s {
            "std" => Some(FmtBackend::Std),
            "zmij" => Some(FmtBackend::Zmij),
            _ => None,
        }
    }

    /// double -> `Dtoa`. `mode` is 0/2/3; `ndigits` is the mode's
    /// digit count (significant digits for mode 2, digits-after-point
    /// for mode 3, ignored for mode 0).
    pub fn dtoa(self, d: f64, mode: i32, ndigits: i32) -> Dtoa {
        let sign = d.is_sign_negative();
        if d.is_nan() {
            return Dtoa::Nan;
        }
        if d.is_infinite() {
            return Dtoa::Inf { sign };
        }
        let abs = d.abs();

        match mode {
            0 => {
                // Shortest round-trip.
                let (digits, decpt) = match self {
                    FmtBackend::Zmij => {
                        let mut buf = zmij::Buffer::new();
                        parse_decimal(buf.format_finite(abs))
                    }
                    FmtBackend::Std => parse_decimal(&format!("{:e}", abs)),
                };
                Dtoa::Finite { digits, decpt, sign }
            }
            2 => {
                // N significant digits. Rust `{:.*e}` with N-1 fraction
                // digits gives N significant digits in scientific form.
                let n = ndigits.max(1);
                let frac = (n - 1) as usize;
                let s = format!("{:.*e}", frac, abs);
                let (digits, decpt) = parse_decimal(&s);
                Dtoa::Finite { digits, decpt, sign }
            }
            3 => {
                // N digits after the decimal point.
                let n = ndigits.max(0) as usize;
                let s = format!("{:.*}", n, abs);
                let (digits, decpt) = parse_decimal(&s);
                if digits.is_empty() {
                    // Value rounded to zero at this precision: dtoa
                    // convention is empty digits, decpt = -ndigits.
                    Dtoa::Finite {
                        digits: Vec::new(),
                        decpt: -ndigits,
                        sign,
                    }
                } else {
                    Dtoa::Finite { digits, decpt, sign }
                }
            }
            _ => Dtoa::Finite {
                digits: vec![b'0'],
                decpt: 1,
                sign,
            },
        }
    }
}

/// Parse a non-negative decimal string into the `(digits, decpt)`
/// contract. Accepts both scientific (`1.5e3`, `1e-1`) and plain
/// (`1234.5`, `0.001`, `12340`) forms. `digits` has leading and
/// trailing zeros stripped; an all-zero input yields empty `digits`.
fn parse_decimal(s: &str) -> (Vec<u8>, i32) {
    let bytes = s.as_bytes();

    // Split mantissa / exponent.
    let (mant, exp): (&[u8], i32) = match bytes.iter().position(|&c| c == b'e' || c == b'E') {
        Some(i) => {
            let e: i32 = s[i + 1..].parse().unwrap_or(0);
            (&bytes[..i], e)
        }
        None => (bytes, 0),
    };

    // Collect mantissa digits, record where the decimal point sits
    // (number of digits before it).
    let mut digits: Vec<u8> = Vec::with_capacity(mant.len());
    let mut int_len: i32 = -1;
    for &c in mant {
        if c == b'.' {
            int_len = digits.len() as i32;
        } else if c.is_ascii_digit() {
            digits.push(c);
        }
    }
    if int_len < 0 {
        int_len = digits.len() as i32; // no '.': all digits are integral
    }

    // decpt before zero-stripping: digits-before-point + exponent.
    let mut decpt = int_len + exp;

    // Strip leading zeros, shifting decpt down by each.
    let first_nz = digits.iter().position(|&c| c != b'0');
    match first_nz {
        None => {
            // All zeros.
            return (Vec::new(), decpt);
        }
        Some(nz) => {
            decpt -= nz as i32;
            digits.drain(..nz);
        }
    }
    // Strip trailing zeros (does not move decpt).
    while digits.len() > 1 && *digits.last().unwrap() == b'0' {
        digits.pop();
    }
    (digits, decpt)
}

/// `std` string-to-float parse backend.
///
/// Returns `Some(value)` only for a string that is a complete, finite
/// decimal float. Non-finite results (overflow, `inf`/`nan` literals)
/// and parse failures return `None` so the caller falls through to
/// CPython's original `tp_new`, which owns those edge cases exactly.
///
/// Rust's `f64::from_str` is correctly-rounded (Eisel-Lemire +
/// round-half-to-even) — the same contract as `_Py_dg_strtod` — so any
/// finite value it accepts is bit-identical to stock CPython's parse.
pub fn strtod_std(s: &str) -> Option<f64> {
    match s.parse::<f64>() {
        Ok(v) if v.is_finite() => Some(v),
        _ => None,
    }
}
