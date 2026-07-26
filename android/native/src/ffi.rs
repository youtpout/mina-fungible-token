//! C interface for the Apple targets, next to the JNI one used on Android.
//!
//! Both wrap the same three functions (`transfer`, `token_balance`, and the
//! backend info); only the calling convention differs, so the prover, the
//! embedded assets and the witness solver are shared byte for byte between the
//! Android app, an iOS app and a macOS build.
//!
//! Every entry point takes a JSON string and returns a JSON string, exactly as
//! the JNI side does. The returned pointer is owned by Rust and must be handed
//! back to [`mina_string_free`]; anything else leaks. A panic inside the
//! prover is caught and turned into a JSON error rather than crossing the
//! language boundary, where it would be undefined behaviour.
//!
//! # Swift
//!
//! ```swift
//! func minaCall(_ f: (UnsafePointer<CChar>?) -> UnsafeMutablePointer<CChar>?,
//!               _ request: String) -> String {
//!     guard let raw = request.withCString({ f($0) }) else { return "{}" }
//!     defer { mina_string_free(raw) }
//!     return String(cString: raw)
//! }
//! ```

use std::{
    ffi::{c_char, CStr, CString},
    panic::catch_unwind,
};

/// Renders a JSON string for the caller. The empty pointer is never returned:
/// a failure is a JSON payload the UI can display like any other response.
fn into_c_string(value: String) -> *mut c_char {
    match CString::new(value) {
        Ok(owned) => owned.into_raw(),
        // A NUL inside the payload should be impossible (serde_json escapes),
        // so report it rather than silently truncating.
        Err(_) => CString::new(r#"{"status":"error","message":"the response contained a NUL byte"}"#)
            .expect("static payload")
            .into_raw(),
    }
}

/// Reads a request, or `None` when the pointer is null or not UTF-8.
///
/// # Safety
///
/// `request` must be null or a NUL-terminated C string that stays valid for
/// the duration of the call.
unsafe fn request_str(request: *const c_char) -> Option<&'static str> {
    if request.is_null() {
        return None;
    }
    CStr::from_ptr(request).to_str().ok()
}

fn error_json(message: &str) -> String {
    serde_json::json!({ "status": "error", "message": message }).to_string()
}

/// The backend versions and the git revisions this binary was built from.
#[no_mangle]
pub extern "C" fn mina_backend_info() -> *mut c_char {
    let value = catch_unwind(|| {
        serde_json::to_string_pretty(&crate::backend().info()).unwrap_or_else(|error| error.to_string())
    })
    .unwrap_or_else(|_| error_json("the native Rust backend panicked during initialization"));
    into_c_string(value)
}

/// Proves a `FungibleToken.transfer` and submits it. Same request and response
/// shape as the Android entry point, timings included.
///
/// # Safety
///
/// `request` must be a NUL-terminated UTF-8 C string.
#[no_mangle]
pub unsafe extern "C" fn mina_transfer(request: *const c_char) -> *mut c_char {
    let Some(request) = request_str(request) else {
        return into_c_string(error_json("the transfer request is not valid UTF-8"));
    };
    let value = catch_unwind(|| crate::transfer(request))
        .unwrap_or_else(|_| error_json("the native prover panicked"));
    into_c_string(value)
}

/// Reads an address's token balance from the configured node.
///
/// # Safety
///
/// `request` must be a NUL-terminated UTF-8 C string.
#[no_mangle]
pub unsafe extern "C" fn mina_token_balance(request: *const c_char) -> *mut c_char {
    let Some(request) = request_str(request) else {
        return into_c_string(error_json("the balance request is not valid UTF-8"));
    };
    let value = catch_unwind(|| crate::token_balance(request))
        .unwrap_or_else(|_| error_json("the native backend panicked"));
    into_c_string(value)
}

/// Releases a string returned by any of the functions above.
///
/// # Safety
///
/// `value` must come from this module and must not be used afterwards.
/// Passing null is allowed and does nothing.
#[no_mangle]
pub unsafe extern "C" fn mina_string_free(value: *mut c_char) {
    if !value.is_null() {
        drop(CString::from_raw(value));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The round trip a Swift caller makes: pass a C string in, read the JSON
    /// out, hand the pointer back.
    #[test]
    fn reports_an_error_for_an_invalid_request() {
        let request = CString::new("not json").expect("request");
        let raw = unsafe { mina_token_balance(request.as_ptr()) };
        let response = unsafe { CStr::from_ptr(raw) }
            .to_str()
            .expect("utf-8 response")
            .to_owned();
        unsafe { mina_string_free(raw) };

        let json: serde_json::Value = serde_json::from_str(&response).expect("json response");
        assert_eq!(json["status"], "error");
    }

    #[test]
    fn rejects_a_null_request_without_dereferencing_it() {
        let raw = unsafe { mina_transfer(std::ptr::null()) };
        let response = unsafe { CStr::from_ptr(raw) }.to_str().expect("utf-8");
        assert!(response.contains("error"));
        unsafe { mina_string_free(raw) };
    }
}
