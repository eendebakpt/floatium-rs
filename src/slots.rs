//! Slot-level patching of `PyFloat_Type`, via raw `pyo3::ffi`.
//!
//! A transliteration of floatium's `slots.cc`. PyO3 is built for
//! *defining* extension types, not monkey-patching a builtin static
//! type, so this file is necessarily raw `unsafe` C-API:
//!
//!   tp_repr        -> floatium_float_repr      repr() / str() / f"{x}"
//!   __format__     -> floatium_float_format     format(), f"{x:spec}"
//!   tp_new         -> floatium_float_new        float("...")
//!   tp_vectorcall  -> floatium_float_vectorcall float("...") fast path
//!
//! Patching tp_vectorcall as well as tp_new is required on CPython
//! 3.13+: the specializing interpreter quickens `float(s)` to
//! `CALL_BUILTIN_CLASS`, which dispatches via tp_vectorcall and skips
//! tp_new entirely.
//!
//! Concurrency: install()/uninstall() run under the GIL; the per-call
//! reads are relaxed atomic loads. The user contract ("install once at
//! interpreter startup") makes this safe on free-threaded builds too.

use std::ffi::c_void;
use std::os::raw::{c_char, c_int};
use std::ptr;
use std::sync::atomic::{AtomicBool, AtomicPtr, AtomicU8, AtomicUsize, Ordering};

use pyo3::ffi;

use crate::dtoa::{strtod_std, FmtBackend};
use crate::format_short::{double_to_string, FLAG_ADD_DOT_0};

// --- saved original slots + active backend ------------------------------
// Function pointers are stored as usize (Option<fn> is null-niche, so 0
// == None). install() writes these under the GIL; the slot wrappers
// read them with relaxed ordering.
static SAVED_REPR: AtomicUsize = AtomicUsize::new(0);
static SAVED_NEW: AtomicUsize = AtomicUsize::new(0);
static SAVED_VECTORCALL: AtomicUsize = AtomicUsize::new(0);
static ORIG_FORMAT: AtomicPtr<ffi::PyObject> = AtomicPtr::new(ptr::null_mut());
static FMT_BACKEND: AtomicU8 = AtomicU8::new(0); // 0 = std, 1 = zmij
static PATCHED: AtomicBool = AtomicBool::new(false);

fn fmt_backend() -> FmtBackend {
    if FMT_BACKEND.load(Ordering::Relaxed) == 1 {
        FmtBackend::Zmij
    } else {
        FmtBackend::Std
    }
}

// --- the __format__ method descriptor -----------------------------------
struct SyncMethodDef(ffi::PyMethodDef);
unsafe impl Sync for SyncMethodDef {}

static FORMAT_METHOD_DEF: SyncMethodDef = SyncMethodDef(ffi::PyMethodDef {
    ml_name: c"__format__".as_ptr(),
    ml_meth: ffi::PyMethodDefPointer {
        PyCFunction: floatium_float_format,
    },
    ml_flags: ffi::METH_O,
    ml_doc: c"floatium-rs patched __format__".as_ptr(),
});

// --- helpers ------------------------------------------------------------

/// Borrow a Python `str` as a Rust `&str` (PyUnicode_AsUTF8 output is
/// valid UTF-8). Returns None if extraction fails.
unsafe fn pystr_as_str<'a>(obj: *mut ffi::PyObject) -> Option<&'a str> {
    let mut len: ffi::Py_ssize_t = 0;
    let p = ffi::PyUnicode_AsUTF8AndSize(obj, &mut len);
    if p.is_null() {
        ffi::PyErr_Clear();
        return None;
    }
    let bytes = std::slice::from_raw_parts(p as *const u8, len as usize);
    std::str::from_utf8(bytes).ok()
}

/// Build a Python `str` from a Rust `&str`.
unsafe fn pystr_from(s: &str) -> *mut ffi::PyObject {
    ffi::PyUnicode_FromStringAndSize(s.as_ptr() as *const c_char, s.len() as ffi::Py_ssize_t)
}

/// CPython's `float()` parse contract, conservatively: returns the
/// value only for a plain finite decimal. Underscores, embedded
/// whitespace, non-ASCII, inf/nan literals and overflow all return
/// None so the caller falls through to the original tp_new — which
/// reproduces stock CPython's edge-case behavior exactly.
fn try_parse_pyfloat(s: &str) -> Option<f64> {
    // Strip leading/trailing ASCII whitespace (matches PyFloat_FromString).
    let t = s.trim_matches(|c| matches!(c, ' ' | '\t' | '\n' | '\r' | '\x0b' | '\x0c'));
    if t.is_empty() {
        return None;
    }
    // Reject anything the std backend shouldn't own.
    for b in t.bytes() {
        if b == b'_' || b >= 0x80 {
            return None;
        }
    }
    strtod_std(t)
}

// --- replacement: tp_repr ----------------------------------------------
unsafe extern "C" fn floatium_float_repr(v: *mut ffi::PyObject) -> *mut ffi::PyObject {
    let d = ffi::PyFloat_AsDouble(v);
    let s = double_to_string(fmt_backend(), d, b'r', 0, FLAG_ADD_DOT_0);
    pystr_from(&s)
}

// --- replacement: float.__format__(spec) -------------------------------
//
// Handles the simple spec grammar inline ([.precision]{e,E,f,F,g,G,r}
// and empty); anything with fill/align/width/#/,/_ falls through to the
// saved original descriptor. The fallback is bit-identical because it
// invokes the real __format__.
unsafe extern "C" fn floatium_float_format(
    self_: *mut ffi::PyObject,
    spec: *mut ffi::PyObject,
) -> *mut ffi::PyObject {
    if ffi::PyFloat_Check(self_) == 0 {
        ffi::PyErr_SetString(
            ffi::PyExc_TypeError,
            c"float.__format__ requires float".as_ptr(),
        );
        return ptr::null_mut();
    }
    if ffi::PyUnicode_Check(spec) == 0 {
        ffi::PyErr_SetString(
            ffi::PyExc_TypeError,
            c"format_spec must be str".as_ptr(),
        );
        return ptr::null_mut();
    }
    let spec_str = match pystr_as_str(spec) {
        Some(s) => s,
        None => return ptr::null_mut(),
    };

    if let Some((code, precision, flags)) = parse_simple_spec(spec_str) {
        let d = ffi::PyFloat_AsDouble(self_);
        let s = double_to_string(fmt_backend(), d, code, precision, flags);
        return pystr_from(&s);
    }

    // Complex spec: delegate to the saved original __format__ descriptor.
    original_format_call(self_, spec)
}

/// Parse the simple spec forms: `""`/`"r"` -> repr, and
/// `"[.precision]{e,E,f,F,g,G}"`. Returns `(format_code, precision,
/// flags)`, or None to signal "fall back to the original".
fn parse_simple_spec(spec: &str) -> Option<(u8, i32, u32)> {
    let b = spec.as_bytes();
    if b.is_empty() {
        return Some((b'r', 0, FLAG_ADD_DOT_0));
    }
    let mut pos = 0usize;
    let mut precision: i32 = -1;
    if b[pos] == b'.' {
        pos += 1;
        if pos >= b.len() || !b[pos].is_ascii_digit() {
            return None;
        }
        precision = 0;
        while pos < b.len() && b[pos].is_ascii_digit() {
            precision = precision * 10 + (b[pos] - b'0') as i32;
            if precision > 1_000_000 {
                return None; // pathological precision: defer to original
            }
            pos += 1;
        }
    }
    if pos != b.len() - 1 {
        return None; // exactly one trailing type char expected
    }
    let t = b[pos];
    match t {
        b'e' | b'E' | b'f' | b'F' | b'g' | b'G' => {
            Some((t, if precision < 0 { 6 } else { precision }, 0))
        }
        b'r' => {
            if precision >= 0 {
                None
            } else {
                Some((b'r', 0, FLAG_ADD_DOT_0))
            }
        }
        _ => None,
    }
}

/// Call the saved original `__format__` descriptor as `descr(self,
/// spec)`. Goes straight to the saved descriptor (not via attribute
/// lookup, which would re-find our replacement and recurse).
unsafe fn original_format_call(
    self_: *mut ffi::PyObject,
    spec: *mut ffi::PyObject,
) -> *mut ffi::PyObject {
    let descr = ORIG_FORMAT.load(Ordering::Relaxed);
    if descr.is_null() {
        ffi::PyErr_SetString(ffi::PyExc_RuntimeError, c"floatium-rs: no saved __format__".as_ptr());
        return ptr::null_mut();
    }
    let args: [*mut ffi::PyObject; 2] = [self_, spec];
    ffi::PyObject_Vectorcall(descr, args.as_ptr(), 2, ptr::null_mut())
}

// --- replacement: tp_new -----------------------------------------------
type NewFn = unsafe extern "C" fn(
    *mut ffi::PyTypeObject,
    *mut ffi::PyObject,
    *mut ffi::PyObject,
) -> *mut ffi::PyObject;

unsafe fn saved_new() -> NewFn {
    std::mem::transmute::<usize, NewFn>(SAVED_NEW.load(Ordering::Relaxed))
}

unsafe extern "C" fn floatium_float_new(
    type_: *mut ffi::PyTypeObject,
    args: *mut ffi::PyObject,
    kwds: *mut ffi::PyObject,
) -> *mut ffi::PyObject {
    // Only the exact (float, single positional str) shape is ours.
    if type_ != ptr::addr_of_mut!(ffi::PyFloat_Type) {
        return saved_new()(type_, args, kwds);
    }
    if !kwds.is_null() && ffi::PyDict_Size(kwds) != 0 {
        return saved_new()(type_, args, kwds);
    }
    if args.is_null() || ffi::PyTuple_Size(args) != 1 {
        return saved_new()(type_, args, kwds);
    }
    let arg = ffi::PyTuple_GetItem(args, 0);
    // CheckExact, not Check: a str subclass may define __float__, which
    // the original tp_new honors and our shortcut would skip.
    if ffi::PyUnicode_CheckExact(arg) == 0 {
        return saved_new()(type_, args, kwds);
    }
    match pystr_as_str(arg).and_then(try_parse_pyfloat) {
        Some(v) => ffi::PyFloat_FromDouble(v),
        None => saved_new()(type_, args, kwds),
    }
}

// --- replacement: tp_vectorcall ----------------------------------------
type VectorcallFn = unsafe extern "C" fn(
    *mut ffi::PyObject,
    *const *mut ffi::PyObject,
    usize,
    *mut ffi::PyObject,
) -> *mut ffi::PyObject;

unsafe fn saved_vectorcall() -> VectorcallFn {
    std::mem::transmute::<usize, VectorcallFn>(SAVED_VECTORCALL.load(Ordering::Relaxed))
}

unsafe extern "C" fn floatium_float_vectorcall(
    callable: *mut ffi::PyObject,
    args: *const *mut ffi::PyObject,
    nargsf: usize,
    kwnames: *mut ffi::PyObject,
) -> *mut ffi::PyObject {
    let fallthrough =
        || saved_vectorcall()(callable, args, nargsf, kwnames);

    if callable != ptr::addr_of_mut!(ffi::PyFloat_Type) as *mut ffi::PyObject {
        return fallthrough();
    }
    if !kwnames.is_null() && ffi::PyTuple_Size(kwnames) != 0 {
        return fallthrough();
    }
    let nargs = ffi::PyVectorcall_NARGS(nargsf);
    if nargs != 1 {
        return fallthrough();
    }
    let arg = *args;
    if ffi::PyUnicode_CheckExact(arg) == 0 {
        return fallthrough();
    }
    match pystr_as_str(arg).and_then(try_parse_pyfloat) {
        Some(v) => ffi::PyFloat_FromDouble(v),
        None => fallthrough(),
    }
}

// --- install / uninstall ------------------------------------------------

/// Returns Err(message) on failure (caller raises the Python exception).
pub unsafe fn install(fmt_backend: FmtBackend) -> Result<(), &'static str> {
    if PATCHED.load(Ordering::Relaxed) {
        return Ok(());
    }
    let tp: *mut ffi::PyTypeObject = ptr::addr_of_mut!(ffi::PyFloat_Type);

    // tp_dict can be NULL on 3.12+ (lazy materialization); PyType_GetDict
    // returns a new strong reference.
    let type_dict = ffi::PyType_GetDict(tp);
    if type_dict.is_null() {
        return Err("PyType_GetDict(float) returned NULL");
    }

    // Save the original __format__ before overwriting.
    let orig_format = ffi::PyDict_GetItemString(type_dict, c"__format__".as_ptr());
    if orig_format.is_null() {
        ffi::Py_DECREF(type_dict);
        return Err("float has no __format__ descriptor");
    }
    ffi::Py_INCREF(orig_format);
    ORIG_FORMAT.store(orig_format, Ordering::Relaxed);

    // Install our __format__ descriptor.
    let func = ffi::PyDescr_NewMethod(
        tp,
        &FORMAT_METHOD_DEF.0 as *const ffi::PyMethodDef as *mut ffi::PyMethodDef,
    );
    if func.is_null() {
        ffi::Py_DECREF(type_dict);
        ffi::Py_CLEAR(ORIG_FORMAT.as_ptr() as *mut *mut ffi::PyObject as *mut _);
        ORIG_FORMAT.store(ptr::null_mut(), Ordering::Relaxed);
        return Err("PyDescr_NewMethod failed");
    }
    if ffi::PyDict_SetItemString(type_dict, c"__format__".as_ptr(), func) < 0 {
        ffi::Py_DECREF(func);
        ffi::Py_DECREF(type_dict);
        return Err("PyDict_SetItemString(__format__) failed");
    }
    ffi::Py_DECREF(func);
    ffi::Py_DECREF(type_dict);

    // Save originals, install replacements.
    SAVED_REPR.store(
        std::mem::transmute::<Option<ffi::reprfunc>, usize>((*tp).tp_repr),
        Ordering::Relaxed,
    );
    SAVED_NEW.store(
        std::mem::transmute::<Option<ffi::newfunc>, usize>((*tp).tp_new),
        Ordering::Relaxed,
    );
    SAVED_VECTORCALL.store(
        std::mem::transmute::<Option<ffi::vectorcallfunc>, usize>((*tp).tp_vectorcall),
        Ordering::Relaxed,
    );
    FMT_BACKEND.store(if fmt_backend == FmtBackend::Zmij { 1 } else { 0 }, Ordering::Relaxed);

    (*tp).tp_repr = Some(floatium_float_repr);
    (*tp).tp_new = Some(floatium_float_new);
    (*tp).tp_vectorcall = Some(floatium_float_vectorcall);

    ffi::PyType_Modified(tp);
    PATCHED.store(true, Ordering::Relaxed);
    Ok(())
}

pub unsafe fn uninstall() {
    if !PATCHED.load(Ordering::Relaxed) {
        return;
    }
    let tp: *mut ffi::PyTypeObject = ptr::addr_of_mut!(ffi::PyFloat_Type);

    (*tp).tp_repr =
        std::mem::transmute::<usize, Option<ffi::reprfunc>>(SAVED_REPR.load(Ordering::Relaxed));
    (*tp).tp_new =
        std::mem::transmute::<usize, Option<ffi::newfunc>>(SAVED_NEW.load(Ordering::Relaxed));
    (*tp).tp_vectorcall = std::mem::transmute::<usize, Option<ffi::vectorcallfunc>>(
        SAVED_VECTORCALL.load(Ordering::Relaxed),
    );

    let orig_format = ORIG_FORMAT.swap(ptr::null_mut(), Ordering::Relaxed);
    if !orig_format.is_null() {
        let type_dict = ffi::PyType_GetDict(tp);
        if !type_dict.is_null() {
            ffi::PyDict_SetItemString(type_dict, c"__format__".as_ptr(), orig_format);
            ffi::Py_DECREF(type_dict);
        }
        ffi::Py_DECREF(orig_format);
    }

    ffi::PyType_Modified(tp);
    PATCHED.store(false, Ordering::Relaxed);
}

pub fn is_patched() -> bool {
    PATCHED.load(Ordering::Relaxed)
}

pub fn current_format_backend() -> Option<&'static str> {
    if is_patched() {
        Some(fmt_backend().name())
    } else {
        None
    }
}

// Silence unused warnings for items referenced only across cfg/ffi.
const _: *const c_void = ptr::null();
const _: c_int = 0;
