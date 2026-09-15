//! PHP's salt and callable policy over independent hashing implementations.
//! Legacy hashes are compatibility APIs, not a recommendation for new secrets.
//! Hashing state is local to one call; no password is retained in engine state.

use std::ffi::{CStr, CString, c_char, c_int, c_void};
use std::sync::OnceLock;

use super::{
    ExecuteData, ExecutorGlobals, FunctionCommon, InternalFunction, ParamTypeHint, Value, VmError,
    make_internal_function, php_byte_result, typed_internal_string_value_argument_expected,
    write_return_value,
};

#[cold]
#[inline(never)]
pub(super) fn register(eg: &mut ExecutorGlobals) -> Box<InternalFunction> {
    let mut function = Box::new(
        make_internal_function(crypt, 2, 2, vec![])
            .with_static_parameter_names(&["string", "salt"]),
    );
    function.common.sig.param_type_hints = vec![ParamTypeHint::String, ParamTypeHint::String];
    function.common.sig.return_type_hint = ParamTypeHint::String;
    function.handler_validates_types = true;
    let pointer = &function.common as *const FunctionCommon;
    eg.register_function("crypt", pointer).unwrap();
    eg.register_internal_function_extension(pointer, "standard");
    eg.register_internal_sensitive_parameters(pointer, &[0]);
    function
}

fn crypt(ed: *mut ExecuteData, rv: *mut Value, eg: &mut ExecutorGlobals) -> Result<(), VmError> {
    let Some(input) =
        typed_internal_string_value_argument_expected(ed, eg, "crypt", 0, "string", "string")?
    else {
        return Ok(());
    };
    let Some(salt) =
        typed_internal_string_value_argument_expected(ed, eg, "crypt", 1, "salt", "string")?
    else {
        return Ok(());
    };
    let input = input.php_string_bytes().unwrap_or_default();
    let salt = salt.php_string_bytes().unwrap_or_default();
    write_return_value(rv, php_byte_result(hash(&input, &salt), true));
    Ok(())
}

fn nul_prefix(bytes: &[u8]) -> &[u8] {
    &bytes[..bytes
        .iter()
        .position(|byte| *byte == 0)
        .unwrap_or(bytes.len())]
}

fn salt_prefix(bytes: &[u8], maximum: usize) -> &[u8] {
    &bytes[..bytes
        .iter()
        .position(|byte| *byte == b'$')
        .unwrap_or(bytes.len())
        .min(maximum)]
}

#[cold]
#[inline(never)]
fn hash(input: &[u8], salt: &[u8]) -> Vec<u8> {
    let input = nul_prefix(input);
    let salt = nul_prefix(salt);
    let failure = || {
        if salt.starts_with(b"*0") {
            b"*1".to_vec()
        } else {
            b"*0".to_vec()
        }
    };
    if let Some(body) = salt.strip_prefix(b"$1$") {
        return md5crypt::md5crypt(input, salt_prefix(body, 8));
    }
    if salt.starts_with(b"$5$") || salt.starts_with(b"$6$") {
        return sha_hash(input, salt).unwrap_or_else(failure);
    }
    if salt.starts_with(b"$2") {
        // PHP exposes only these bcrypt variants, even if the native library
        // supports more schemes. Truncation precedes its general input limit.
        if !matches!(salt.get(..4), Some(b"$2a$" | b"$2b$" | b"$2x$" | b"$2y$")) {
            return failure();
        }
        return native_hash(&input[..input.len().min(72)], salt).unwrap_or_else(failure);
    }
    if salt.starts_with(b"_") {
        let Some(setting) = salt
            .get(..9)
            .and_then(|bytes| std::str::from_utf8(bytes).ok())
        else {
            return failure();
        };
        // Unlike crypt_ra's general passphrase buffer, this byte-oriented
        // implementation can fold extended-DES inputs beyond 511 bytes.
        #[allow(deprecated)]
        return pwhash::bsdi_crypt::hash_with(setting, input)
            .map(String::into_bytes)
            .unwrap_or_else(|_| failure());
    }
    let Some(setting) = salt
        .get(..2)
        .and_then(|bytes| std::str::from_utf8(bytes).ok())
    else {
        return failure();
    };
    #[allow(deprecated)]
    pwhash::unix_crypt::hash_with(setting, input)
        .map(String::into_bytes)
        .unwrap_or_else(|_| failure())
}

/// Return a recognized, valid rounds field, a literal salt, or an invalid
/// numeric rounds request. PHP admits leading C whitespace/sign/zeroes but
/// only consumes the numeric field when a dollar immediately follows it.
fn sha_rounds(body: &[u8]) -> Result<(Option<u32>, &[u8]), ()> {
    let Some(field) = body.strip_prefix(b"rounds=") else {
        return Ok((None, body));
    };
    let Some(end) = field.iter().position(|byte| *byte == b'$') else {
        return Ok((None, body));
    };
    let number = &field[..end];
    if number.is_empty() {
        return Err(());
    }
    let start = number
        .iter()
        .position(|byte| !matches!(byte, b' ' | b'\t' | b'\n' | b'\r' | 11 | 12))
        .unwrap_or(number.len());
    let signed = &number[start..];
    let negative = signed.first() == Some(&b'-');
    let digits = match signed.first() {
        Some(b'+' | b'-') => &signed[1..],
        _ => signed,
    };
    if digits.is_empty() || !digits.iter().all(u8::is_ascii_digit) {
        return Ok((None, body));
    }
    if negative {
        return Err(());
    }
    let value = digits
        .iter()
        .try_fold(0u32, |value, byte| {
            value.checked_mul(10)?.checked_add(u32::from(byte - b'0'))
        })
        .ok_or(())?;
    if !(1000..=999_999_999).contains(&value) {
        return Err(());
    }
    Ok((Some(value), &field[end + 1..]))
}

fn sha_hash(input: &[u8], setting: &[u8]) -> Option<Vec<u8>> {
    let (rounds, salt) = sha_rounds(&setting[3..]).ok()?;
    let salt = salt_prefix(salt, 16);
    let params = sha_crypt::Params::new(rounds.unwrap_or(5000)).ok()?;
    let mut output = Vec::with_capacity(123);
    output.extend_from_slice(&setting[..3]);
    if let Some(rounds) = rounds {
        output.extend_from_slice(format!("rounds={rounds}$").as_bytes());
    }
    output.extend_from_slice(salt);
    output.push(b'$');
    if setting[1] == b'5' {
        encode_sha_digest(&mut output, &sha_crypt::sha256_crypt(input, salt, params));
    } else {
        encode_sha_digest(&mut output, &sha_crypt::sha512_crypt(input, salt, params));
    }
    Some(output)
}

/// Encoding only, following step 22 of the public SHA-crypt specification
/// (https://www.akkadia.org/drepper/SHA-crypt.txt). Digest computation stays in
/// the RustCrypto dependency. The modular index walk avoids a copied table.
fn encode_sha_digest(output: &mut Vec<u8>, digest: &[u8]) {
    let groups = digest.len() / 3;
    let modulus = groups * 3;
    let stride = if digest.len() == 32 { 21 } else { 22 };
    for group in 0..groups {
        let high = group * stride % modulus;
        let middle = (high + groups) % modulus;
        let low = (middle + groups) % modulus;
        encode_word(
            output,
            u32::from(digest[low])
                | (u32::from(digest[middle]) << 8)
                | (u32::from(digest[high]) << 16),
            4,
        );
    }
    let mut tail = 0u32;
    for (index, byte) in digest[modulus..].iter().enumerate() {
        tail |= u32::from(*byte) << (index * 8);
    }
    encode_word(output, tail, (digest.len() - modulus) + 1);
}

fn encode_word(output: &mut Vec<u8>, mut word: u32, count: usize) {
    const ALPHABET: &[u8; 64] = b"./0123456789ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz";
    for _ in 0..count {
        output.push(ALPHABET[(word & 63) as usize]);
        word >>= 6;
    }
}

// The system allocator remains linked normally. crypt_ra owns an opaque
// malloc allocation; no struct layout from a C implementation is used.
unsafe extern "C" {
    fn free(pointer: *mut c_void);
}

type CryptRa =
    unsafe extern "C" fn(*const c_char, *const c_char, *mut *mut c_void, *mut c_int) -> *mut c_char;
type CheckSalt = unsafe extern "C" fn(*const c_char) -> c_int;

struct NativeApi {
    // Keep code and its copied function pointer live together. This owner is
    // installed once and never unloaded, including during another thread's
    // in-flight crypt_ra call. No PHP Value or password is stored here.
    _library: libloading::Library,
    crypt_ra: CryptRa,
    bcrypt_available: bool,
}

#[cold]
#[inline(never)]
fn native_api() -> Option<&'static NativeApi> {
    static API: OnceLock<Option<NativeApi>> = OnceLock::new();
    API.get_or_init(|| {
        // SAFETY: build.rs validates the libxcrypt >=4.4 installation and
        // embeds its absolute shared-library path; PHP cannot select code.
        // Its initializers and the two documented C functions are safe to
        // invoke concurrently and do not unwind into Rust. Symbol types are
        // the public crypt_ra/checksalt ABI. The owning Library is retained
        // beside the copied pointer in this process-lifetime OnceLock, so it
        // cannot unload while any call runs. The static capability setting
        // is terminated and lives for the entire read-only query. On a load
        // or symbol error, the local Library closes without publishing an API.
        unsafe {
            let library = libloading::Library::new(env!("RPHP_CRYPT_LIBRARY")).ok()?;
            let crypt_ra = *library.get::<CryptRa>(b"crypt_ra\0").ok()?;
            let checksalt = *library.get::<CheckSalt>(b"crypt_checksalt\0").ok()?;
            let bcrypt_available = matches!(
                checksalt(c"$2y$04$abcdefghijklmnopqrstuu".as_ptr()),
                0 | 3 | 4
            );
            Some(NativeApi {
                _library: library,
                crypt_ra,
                bcrypt_available,
            })
        }
    })
    .as_ref()
}

struct NativeState {
    pointer: *mut c_void,
    size: c_int,
}

impl Drop for NativeState {
    fn drop(&mut self) {
        // SAFETY: pointer starts null and is assigned only by crypt_ra's
        // documented malloc/realloc ownership API. This non-Clone local owner
        // is the sole releaser, including null/error/unwind paths. No borrowed
        // output escapes native_hash. C free accepts null and cannot unwind.
        unsafe { free(self.pointer) };
    }
}

fn native_hash(input: &[u8], salt: &[u8]) -> Option<Vec<u8>> {
    let input = CString::new(input).ok()?;
    let salt = CString::new(salt).ok()?;
    let api = native_api()?;
    let mut state = NativeState {
        pointer: std::ptr::null_mut(),
        size: 0,
    };
    // SAFETY: both input pointers are live immutable NUL-terminated CStrings.
    // The writable pointer/size belong exclusively to this call and initially
    // request allocation with (null, 0). crypt_ra is reentrant and retains no
    // caller pointer. Its nonnull result is a NUL-terminated string inside the
    // allocation owned by state; copy it before state drops. Null is checked
    // before reading, and the C ABI does not unwind through Rust.
    unsafe {
        let result = (api.crypt_ra)(
            input.as_ptr(),
            salt.as_ptr(),
            &mut state.pointer,
            &mut state.size,
        );
        if result.is_null() {
            return None;
        }
        let result = CStr::from_ptr(result).to_bytes();
        if result.starts_with(b"*") {
            None
        } else {
            Some(result.to_vec())
        }
    }
}

pub(crate) fn bcrypt_available() -> bool {
    native_api().is_some_and(|api| api.bcrypt_available)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reentrant_native_calls_copy_results_before_releasing_state() {
        let rows: Vec<serde_json::Value> =
            include_str!("../../tests/fixtures/crypt/hash-contracts.out")
                .lines()
                .map(|line| serde_json::from_str(line).unwrap())
                .collect();
        let bytes = |value: &serde_json::Value| {
            value
                .as_str()
                .unwrap()
                .as_bytes()
                .chunks_exact(2)
                .map(|pair| u8::from_str_radix(std::str::from_utf8(pair).unwrap(), 16).unwrap())
                .collect::<Vec<_>>()
        };
        let cases: Vec<_> = rows
            .iter()
            .filter(|row| row["name"].as_str().unwrap().starts_with("bcrypt-"))
            .map(|row| {
                (
                    bytes(&row["input"]),
                    bytes(&row["salt"]),
                    bytes(&row["output"]),
                )
            })
            .collect();
        std::thread::scope(|scope| {
            for _ in 0..8 {
                let cases = &cases;
                scope.spawn(move || {
                    for _ in 0..3 {
                        for (input, salt, expected) in cases {
                            assert_eq!(hash(input, salt), *expected);
                            assert_eq!(native_hash(b"synthetic", b"*0"), None);
                            assert_eq!(native_hash(b"synthetic\0", salt), None);
                        }
                    }
                });
            }
        });
    }
}
