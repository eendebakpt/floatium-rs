//! floatium-rs — Rust-backed drop-in replacement for CPython float
//! formatting and parsing.
//!
//! The `_ext` module exposes install / uninstall / is_patched / info to
//! the Python layer, which patches `PyFloat_Type` slots so that
//! `repr(float)`, `float.__format__`, and `float("...")` route through
//! Rust crates (zmij / std) instead of CPython's `dtoa.c`.

mod dtoa;
mod format_short;
mod slots;

use pyo3::exceptions::{PyRuntimeError, PyValueError};
use pyo3::prelude::*;
use pyo3::types::PyDict;

use dtoa::FmtBackend;

/// Default format backend when `install()` is called with no argument.
const DEFAULT_FMT: FmtBackend = FmtBackend::Zmij;

/// Patch `PyFloat_Type` to route float formatting/parsing through Rust.
///
/// `format_backend`: "std" or "zmij" (default "zmij").
/// `parse_backend`:  "std" (the only parse backend).
/// Idempotent — a second call while patched is a no-op.
#[pyfunction]
#[pyo3(signature = (format_backend=None, parse_backend=None))]
fn install(format_backend: Option<String>, parse_backend: Option<String>) -> PyResult<()> {
    let fmt = match format_backend.as_deref() {
        None => DEFAULT_FMT,
        Some(name) => FmtBackend::from_name(name).ok_or_else(|| {
            PyValueError::new_err(format!(
                "unknown format backend: {name} (available: std, zmij)"
            ))
        })?,
    };
    if let Some(p) = parse_backend.as_deref() {
        if p != "std" {
            return Err(PyValueError::new_err(format!(
                "unknown parse backend: {p} (available: std)"
            )));
        }
    }
    unsafe { slots::install(fmt) }.map_err(PyRuntimeError::new_err)
}

/// Restore the original `PyFloat_Type` slots.
#[pyfunction]
fn uninstall() {
    unsafe { slots::uninstall() }
}

/// True if floatium-rs is currently installed.
#[pyfunction]
fn is_patched() -> bool {
    slots::is_patched()
}

/// A dict describing current state and available backends.
#[pyfunction]
fn info(py: Python<'_>) -> PyResult<Py<PyDict>> {
    let d = PyDict::new(py);
    d.set_item("patched", slots::is_patched())?;
    d.set_item("format_backend", slots::current_format_backend())?;
    d.set_item(
        "parse_backend",
        if slots::is_patched() { Some("std") } else { None },
    )?;
    d.set_item("available_format_backends", "std,zmij")?;
    d.set_item("available_parse_backends", "std")?;
    d.set_item("default_format_backend", DEFAULT_FMT.name())?;
    d.set_item("default_parse_backend", "std")?;
    Ok(d.into())
}

#[pymodule(gil_used = false)]
fn _ext(m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add_function(wrap_pyfunction!(install, m)?)?;
    m.add_function(wrap_pyfunction!(uninstall, m)?)?;
    m.add_function(wrap_pyfunction!(is_patched, m)?)?;
    m.add_function(wrap_pyfunction!(info, m)?)?;
    Ok(())
}
