//! Packaging a `Dtoa` into CPython's exact textual form.
//!
//! A port of CPython's `Python/pystrtod.c:format_float_short` +
//! `PyOS_double_to_string`. The point of porting it (rather than
//! calling it) is to feed it a pluggable Rust backend instead of
//! `_Py_dg_dtoa`, while keeping output byte-identical to stock CPython
//! for every input where the backend's digits match `_Py_dg_dtoa`.
//!
//! Upstream license: PSF. Compatible with MIT redistribution.

use crate::dtoa::{Dtoa, FmtBackend};

// Flag bits — mirror CPython's Py_DTSF_* constants.
pub const FLAG_SIGN: u32 = 0x1; // always show a sign
pub const FLAG_ADD_DOT_0: u32 = 0x2; // add ".0" to integers (repr/str)
pub const FLAG_ALT: u32 = 0x4; // alternate form (printf '#')
pub const FLAG_NO_NEG_0: u32 = 0x8; // never emit "-0.0"

fn push_zeros(out: &mut Vec<u8>, n: i32) {
    if n > 0 {
        out.extend(std::iter::repeat(b'0').take(n as usize));
    }
}

/// `format_code`: one of `e E f F g G r`. `precision` as for printf
/// (ignored for `r`). Returns the finished string.
pub fn double_to_string(
    backend: FmtBackend,
    val: f64,
    format_code: u8,
    precision: i32,
    flags: u32,
) -> String {
    // --- mode mapping (CPython's PyOS_double_to_string) ---------------
    let mut uppercase = false;
    let mut code = format_code;
    let mode;
    let mut prec = precision;
    match format_code {
        b'E' => {
            uppercase = true;
            code = b'e';
            mode = 2;
            prec += 1;
        }
        b'e' => {
            mode = 2;
            prec += 1;
        }
        b'F' => {
            uppercase = true;
            code = b'f';
            mode = 3;
        }
        b'f' => {
            mode = 3;
        }
        b'G' => {
            uppercase = true;
            code = b'g';
            mode = 2;
            if prec == 0 {
                prec = 1;
            }
        }
        b'g' => {
            mode = 2;
            if prec == 0 {
                prec = 1;
            }
        }
        b'r' => {
            // repr: shortest. precision must be 0.
            mode = 0;
            prec = 0;
        }
        _ => {
            // Unreachable for our call sites; fall back to repr-shape.
            mode = 0;
            prec = 0;
        }
    }

    let result = backend.dtoa(val, mode, prec);
    format_float_short(result, code, prec, flags, uppercase)
}

fn format_float_short(
    result: Dtoa,
    format_code: u8,
    precision: i32,
    flags: u32,
    uppercase: bool,
) -> String {
    let always_add_sign = flags & FLAG_SIGN != 0;
    let add_dot_0 = flags & FLAG_ADD_DOT_0 != 0;
    let use_alt = flags & FLAG_ALT != 0;
    let no_neg_zero = flags & FLAG_NO_NEG_0 != 0;

    let (s_inf, s_nan, s_e): (&str, &str, u8) = if uppercase {
        ("INF", "NAN", b'E')
    } else {
        ("inf", "nan", b'e')
    };

    // --- special values ---------------------------------------------
    let (mut digits, mut decpt, mut sign): (Vec<u8>, i32, bool) = match result {
        Dtoa::Nan => {
            let mut out = String::new();
            if always_add_sign {
                out.push('+');
            }
            out.push_str(s_nan);
            return out;
        }
        Dtoa::Inf { sign } => {
            let mut out = String::new();
            if sign {
                out.push('-');
            } else if always_add_sign {
                out.push('+');
            }
            out.push_str(s_inf);
            return out;
        }
        Dtoa::Finite {
            digits,
            decpt,
            sign,
        } => (digits, decpt, sign),
    };

    let digits_len = digits.len() as i32;

    if no_neg_zero && sign && (digits_len == 0 || (digits_len == 1 && digits[0] == b'0')) {
        sign = false;
    }

    // --- decimal-point placement / exponent decision ----------------
    let mut vdigits_end = digits_len;
    let mut use_exp = false;
    match format_code {
        b'e' => {
            use_exp = true;
            vdigits_end = precision;
        }
        b'f' => {
            vdigits_end = decpt + precision;
        }
        b'g' => {
            let limit = if add_dot_0 { precision - 1 } else { precision };
            if decpt <= -4 || decpt > limit {
                use_exp = true;
            }
            if use_alt {
                vdigits_end = precision;
            }
        }
        b'r' => {
            if decpt <= -4 || decpt > 16 {
                use_exp = true;
            }
        }
        _ => {}
    }

    let mut exp = 0i32;
    if use_exp {
        exp = decpt - 1;
        decpt = 1;
    }

    let vdigits_start = if decpt <= 0 { decpt - 1 } else { 0 };
    vdigits_end = if !use_exp && add_dot_0 {
        vdigits_end.max(decpt + 1)
    } else {
        vdigits_end.max(decpt)
    };

    debug_assert!(vdigits_start <= 0 && 0 <= digits_len && digits_len <= vdigits_end);
    debug_assert!(vdigits_start < decpt && decpt <= vdigits_end);

    // --- emit --------------------------------------------------------
    let mut out: Vec<u8> = Vec::with_capacity(
        (3 + (vdigits_end - vdigits_start) + if use_exp { 5 } else { 0 }) as usize,
    );

    if sign {
        out.push(b'-');
    } else if always_add_sign {
        out.push(b'+');
    }

    // Leading zero padding.
    if decpt <= 0 {
        push_zeros(&mut out, decpt - vdigits_start);
        out.push(b'.');
        push_zeros(&mut out, -decpt);
    } else {
        push_zeros(&mut out, -vdigits_start);
    }

    // Significant digits, with the decimal point if it falls inside.
    if 0 < decpt && decpt <= digits_len {
        out.extend_from_slice(&digits[..decpt as usize]);
        out.push(b'.');
        out.extend_from_slice(&digits[decpt as usize..]);
    } else {
        out.extend_from_slice(&digits);
    }

    // Trailing zeros.
    if digits_len < decpt {
        push_zeros(&mut out, decpt - digits_len);
        out.push(b'.');
        push_zeros(&mut out, vdigits_end - decpt);
    } else {
        push_zeros(&mut out, vdigits_end - digits_len);
    }

    // Drop a dangling '.' unless alternate form is requested.
    if out.last() == Some(&b'.') && !use_alt {
        out.pop();
    }

    if use_exp {
        out.push(s_e);
        // CPython uses "%+.02d": always-signed, at least 2 exponent digits.
        out.extend_from_slice(format!("{:+03}", exp).as_bytes());
    }

    // The buffer is ASCII by construction.
    let _ = &mut digits;
    String::from_utf8(out).expect("format output is ASCII")
}
