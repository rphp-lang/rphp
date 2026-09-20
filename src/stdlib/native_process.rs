//! Cold boundary for process-global native state.
//!
//! Environment variables, locales, and GNU gettext catalogs are all backed by
//! process-global native state. Keeping their raw calls in one place makes the
//! single-threaded VM assumption explicit and prevents unsafe call sites from
//! spreading through otherwise safe standard-library handlers.

#[cfg(target_os = "linux")]
use std::cell::Cell;
use std::ffi::OsStr;

#[cfg(target_os = "linux")]
use std::ffi::CStr;
#[cfg(target_os = "linux")]
use std::ffi::c_void;
#[cfg(target_os = "linux")]
use std::os::raw::{c_char, c_int, c_ulong};

#[cfg(target_os = "linux")]
unsafe extern "C" {
    #[link_name = "setlocale"]
    fn native_setlocale(category: c_int, locale: *const c_char) -> *mut c_char;
    #[link_name = "nl_langinfo"]
    fn native_nl_langinfo(item: c_int) -> *mut c_char;
    #[link_name = "strcoll"]
    fn native_strcoll(left: *const c_char, right: *const c_char) -> c_int;
    #[link_name = "textdomain"]
    fn native_textdomain(domain: *const c_char) -> *mut c_char;
    #[link_name = "gettext"]
    fn native_gettext(message: *const c_char) -> *mut c_char;
    #[link_name = "dgettext"]
    fn native_dgettext(domain: *const c_char, message: *const c_char) -> *mut c_char;
    #[link_name = "dcgettext"]
    fn native_dcgettext(
        domain: *const c_char,
        message: *const c_char,
        category: c_int,
    ) -> *mut c_char;
    #[link_name = "bindtextdomain"]
    fn native_bindtextdomain(domain: *const c_char, directory: *const c_char) -> *mut c_char;
    #[link_name = "ngettext"]
    fn native_ngettext(
        singular: *const c_char,
        plural: *const c_char,
        count: c_ulong,
    ) -> *mut c_char;
    #[link_name = "dngettext"]
    fn native_dngettext(
        domain: *const c_char,
        singular: *const c_char,
        plural: *const c_char,
        count: c_ulong,
    ) -> *mut c_char;
    #[link_name = "dcngettext"]
    fn native_dcngettext(
        domain: *const c_char,
        singular: *const c_char,
        plural: *const c_char,
        count: c_ulong,
        category: c_int,
    ) -> *mut c_char;
    #[link_name = "bind_textdomain_codeset"]
    fn native_bind_textdomain_codeset(domain: *const c_char, codeset: *const c_char)
    -> *mut c_char;
    #[link_name = "iconv_open"]
    fn native_iconv_open(to: *const c_char, from: *const c_char) -> *mut c_void;
    #[link_name = "iconv"]
    fn native_iconv(
        descriptor: *mut c_void,
        input: *mut *mut c_char,
        input_left: *mut usize,
        output: *mut *mut c_char,
        output_left: *mut usize,
    ) -> usize;
    #[link_name = "iconv_close"]
    fn native_iconv_close(descriptor: *mut c_void) -> c_int;
    #[cfg(target_env = "gnu")]
    #[link_name = "gnu_get_libc_version"]
    fn native_gnu_get_libc_version() -> *const c_char;
}

#[cfg(target_os = "linux")]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(super) enum NativeIconvError {
    #[default]
    None,
    UnknownEncoding,
    IllegalSequence,
    IncompleteSequence,
    Other(i32),
}

#[cfg(target_os = "linux")]
pub(super) struct NativeInput<'a> {
    original: &'a [u8],
    terminated: Vec<u8>,
}

#[cfg(target_os = "linux")]
impl<'a> NativeInput<'a> {
    pub(super) fn new(original: &'a [u8]) -> Self {
        let mut terminated = Vec::with_capacity(original.len() + 1);
        terminated.extend_from_slice(original);
        terminated.push(0);
        Self {
            original,
            terminated,
        }
    }

    fn as_ptr(&self) -> *const c_char {
        self.terminated.as_ptr().cast()
    }
}

pub(super) enum NativeCall<'a> {
    SetEnvironment(&'a OsStr, &'a OsStr),
    #[cfg(target_os = "linux")]
    SetLocale {
        category: c_int,
        locale: Option<&'a NativeInput<'a>>,
    },
    #[cfg(target_os = "linux")]
    NlLangInfo(c_int),
    #[cfg(target_os = "linux")]
    StrColl {
        left: &'a NativeInput<'a>,
        right: &'a NativeInput<'a>,
        result: &'a Cell<c_int>,
    },
    #[cfg(target_os = "linux")]
    TextDomain(Option<&'a NativeInput<'a>>),
    #[cfg(target_os = "linux")]
    Gettext(&'a NativeInput<'a>),
    #[cfg(target_os = "linux")]
    DGettext(&'a NativeInput<'a>, &'a NativeInput<'a>),
    #[cfg(target_os = "linux")]
    DcGettext(&'a NativeInput<'a>, &'a NativeInput<'a>, c_int),
    #[cfg(target_os = "linux")]
    BindTextDomain(&'a NativeInput<'a>, Option<&'a NativeInput<'a>>),
    #[cfg(target_os = "linux")]
    NGettext(&'a NativeInput<'a>, &'a NativeInput<'a>, c_ulong),
    #[cfg(target_os = "linux")]
    DnGettext(
        &'a NativeInput<'a>,
        &'a NativeInput<'a>,
        &'a NativeInput<'a>,
        c_ulong,
    ),
    #[cfg(target_os = "linux")]
    DcnGettext(
        &'a NativeInput<'a>,
        &'a NativeInput<'a>,
        &'a NativeInput<'a>,
        c_ulong,
        c_int,
    ),
    #[cfg(target_os = "linux")]
    BindCodeset(&'a NativeInput<'a>, Option<&'a NativeInput<'a>>),
    #[cfg(target_os = "linux")]
    Iconv {
        from: &'a NativeInput<'a>,
        to: &'a NativeInput<'a>,
        input: &'a [u8],
        error: &'a Cell<NativeIconvError>,
    },
    #[cfg(target_os = "linux")]
    IconvVersion,
}

/// All process-global native mutations, raw calls, and returned-pointer reads
/// stay in this one cold boundary. Message calls retain the original PHP byte
/// string when libc reports an untranslated result by returning the supplied
/// pointer; this preserves embedded NUL bytes in untranslated messages.
#[cold]
#[inline(never)]
#[cfg_attr(target_os = "linux", unsafe(link_section = ".rphp_cold"))]
pub(super) fn invoke_native(call: NativeCall<'_>) -> Option<Vec<u8>> {
    // SAFETY: NativeInput owns a NUL-terminated buffer for every name pointer
    // for the full duration of its call, and returned C strings are copied
    // before another native state change can invalidate them. iconv receives
    // the exact input length, advances only its local pointer and writes into
    // a live Vec spare region; every resize rebuilds that output pointer.
    // RPHP executes process-global locale/catalog/environment mutations on its
    // single VM thread, so no concurrent Rust environment access is possible.
    unsafe {
        match call {
            NativeCall::SetEnvironment(key, value) => {
                std::env::set_var(key, value);
                None
            }
            #[cfg(target_os = "linux")]
            call => {
                if let NativeCall::IconvVersion = &call {
                    #[cfg(target_env = "gnu")]
                    {
                        let result = native_gnu_get_libc_version();
                        if result.is_null() {
                            return None;
                        }
                        return Some(CStr::from_ptr(result).to_bytes().to_vec());
                    }
                    #[cfg(not(target_env = "gnu"))]
                    return None;
                }
                if let NativeCall::Iconv {
                    from,
                    to,
                    input,
                    error,
                } = call
                {
                    let descriptor = native_iconv_open(to.as_ptr(), from.as_ptr());
                    if descriptor as usize == usize::MAX {
                        error.set(NativeIconvError::UnknownEncoding);
                        return None;
                    }

                    let mut input_left = input.len();
                    let mut input_pointer = input.as_ptr().cast_mut().cast::<c_char>();
                    let mut output = vec![0_u8; input.len().saturating_mul(4).max(64)];
                    let mut written = 0_usize;
                    loop {
                        let mut output_pointer = output.as_mut_ptr().add(written).cast::<c_char>();
                        let mut output_left = output.len() - written;
                        let result = native_iconv(
                            descriptor,
                            &mut input_pointer,
                            &mut input_left,
                            &mut output_pointer,
                            &mut output_left,
                        );
                        written = output.len() - output_left;
                        if result != usize::MAX {
                            break;
                        }
                        let errno = std::io::Error::last_os_error().raw_os_error().unwrap_or(0);
                        if errno == 7 {
                            output.resize(output.len().saturating_mul(2).max(64), 0);
                            continue;
                        }
                        if errno == 84
                            && to
                                .original
                                .windows(8)
                                .any(|suffix| suffix.eq_ignore_ascii_case(b"//IGNORE"))
                        {
                            if input_left == 0 {
                                break;
                            }
                            input_pointer = input_pointer.add(1);
                            input_left -= 1;
                            continue;
                        }
                        error.set(match errno {
                            84 => NativeIconvError::IllegalSequence,
                            22 => NativeIconvError::IncompleteSequence,
                            other => NativeIconvError::Other(other),
                        });
                        let _ = native_iconv_close(descriptor);
                        return None;
                    }

                    loop {
                        let mut output_pointer = output.as_mut_ptr().add(written).cast::<c_char>();
                        let mut output_left = output.len() - written;
                        let result = native_iconv(
                            descriptor,
                            std::ptr::null_mut(),
                            std::ptr::null_mut(),
                            &mut output_pointer,
                            &mut output_left,
                        );
                        written = output.len() - output_left;
                        if result != usize::MAX {
                            break;
                        }
                        let errno = std::io::Error::last_os_error().raw_os_error().unwrap_or(0);
                        if errno == 7 {
                            output.resize(output.len().saturating_mul(2).max(64), 0);
                            continue;
                        }
                        error.set(NativeIconvError::Other(errno));
                        let _ = native_iconv_close(descriptor);
                        return None;
                    }
                    let _ = native_iconv_close(descriptor);
                    output.truncate(written);
                    return Some(output);
                }
                if let NativeCall::StrColl {
                    left,
                    right,
                    result,
                } = call
                {
                    result.set(native_strcoll(left.as_ptr(), right.as_ptr()));
                    return Some(Vec::new());
                }
                let (result, first_fallback, second_fallback) = match call {
                    NativeCall::SetLocale { category, locale } => (
                        native_setlocale(
                            category,
                            locale.map_or(std::ptr::null(), NativeInput::as_ptr),
                        ),
                        None,
                        None,
                    ),
                    NativeCall::NlLangInfo(item) => (native_nl_langinfo(item), None, None),
                    NativeCall::TextDomain(domain) => (
                        native_textdomain(domain.map_or(std::ptr::null(), NativeInput::as_ptr)),
                        None,
                        None,
                    ),
                    NativeCall::Gettext(message) => {
                        (native_gettext(message.as_ptr()), Some(message), None)
                    }
                    NativeCall::DGettext(domain, message) => (
                        native_dgettext(domain.as_ptr(), message.as_ptr()),
                        Some(message),
                        None,
                    ),
                    NativeCall::DcGettext(domain, message, category) => (
                        native_dcgettext(domain.as_ptr(), message.as_ptr(), category),
                        Some(message),
                        None,
                    ),
                    NativeCall::BindTextDomain(domain, directory) => (
                        native_bindtextdomain(
                            domain.as_ptr(),
                            directory.map_or(std::ptr::null(), NativeInput::as_ptr),
                        ),
                        None,
                        None,
                    ),
                    NativeCall::NGettext(singular, plural, count) => (
                        native_ngettext(singular.as_ptr(), plural.as_ptr(), count),
                        Some(singular),
                        Some(plural),
                    ),
                    NativeCall::DnGettext(domain, singular, plural, count) => (
                        native_dngettext(
                            domain.as_ptr(),
                            singular.as_ptr(),
                            plural.as_ptr(),
                            count,
                        ),
                        Some(singular),
                        Some(plural),
                    ),
                    NativeCall::DcnGettext(domain, singular, plural, count, category) => (
                        native_dcngettext(
                            domain.as_ptr(),
                            singular.as_ptr(),
                            plural.as_ptr(),
                            count,
                            category,
                        ),
                        Some(singular),
                        Some(plural),
                    ),
                    NativeCall::BindCodeset(domain, codeset) => (
                        native_bind_textdomain_codeset(
                            domain.as_ptr(),
                            codeset.map_or(std::ptr::null(), NativeInput::as_ptr),
                        ),
                        None,
                        None,
                    ),
                    NativeCall::Iconv { .. } => unreachable!(),
                    NativeCall::IconvVersion => unreachable!(),
                    NativeCall::StrColl { .. } => unreachable!(),
                    NativeCall::SetEnvironment(..) => unreachable!(),
                };
                if result.is_null() {
                    return None;
                }
                for fallback in [first_fallback, second_fallback].into_iter().flatten() {
                    if std::ptr::eq(result.cast_const(), fallback.as_ptr()) {
                        return Some(fallback.original.to_vec());
                    }
                }
                Some(CStr::from_ptr(result).to_bytes().to_vec())
            }
        }
    }
}

#[cfg(target_os = "linux")]
#[cfg_attr(target_os = "linux", unsafe(link_section = ".rphp_cold"))]
pub(super) fn convert_encoding(
    from: &[u8],
    to: &[u8],
    input: &[u8],
) -> Result<Vec<u8>, NativeIconvError> {
    if from.contains(&0) || to.contains(&0) {
        return Err(NativeIconvError::UnknownEncoding);
    }
    let from = NativeInput::new(from);
    let to = NativeInput::new(to);
    let error = Cell::new(NativeIconvError::None);
    invoke_native(NativeCall::Iconv {
        from: &from,
        to: &to,
        input,
        error: &error,
    })
    .ok_or_else(|| error.get())
}

#[cfg(target_os = "linux")]
#[cfg_attr(target_os = "linux", unsafe(link_section = ".rphp_cold"))]
pub(super) fn iconv_version() -> Option<Vec<u8>> {
    invoke_native(NativeCall::IconvVersion)
}

#[cfg_attr(target_os = "linux", unsafe(link_section = ".rphp_cold"))]
pub(super) fn set_environment_variable(key: &OsStr, value: &OsStr) {
    invoke_native(NativeCall::SetEnvironment(key, value));
}

#[cfg(target_os = "linux")]
#[cfg_attr(target_os = "linux", unsafe(link_section = ".rphp_cold"))]
pub(super) fn set_process_locale(category: i64, locale: Option<&[u8]>) -> Option<Vec<u8>> {
    let locale = locale.map(NativeInput::new);
    invoke_native(NativeCall::SetLocale {
        category: category as c_int,
        locale: locale.as_ref(),
    })
}

#[cfg(target_os = "linux")]
#[cfg_attr(target_os = "linux", unsafe(link_section = ".rphp_cold"))]
pub(super) fn locale_info(item: i64) -> Option<Vec<u8>> {
    let item = c_int::try_from(item).ok()?;
    invoke_native(NativeCall::NlLangInfo(item))
}

#[cfg(target_os = "linux")]
#[cfg_attr(target_os = "linux", unsafe(link_section = ".rphp_cold"))]
pub(super) fn compare_locale_strings(left: &[u8], right: &[u8]) -> i64 {
    let left = NativeInput::new(left);
    let right = NativeInput::new(right);
    let result = Cell::new(0);
    let _ = invoke_native(NativeCall::StrColl {
        left: &left,
        right: &right,
        result: &result,
    });
    i64::from(result.get())
}

#[cfg(test)]
#[cfg(target_os = "linux")]
mod tests {
    use super::*;

    #[test]
    fn native_input_preserves_php_bytes_beyond_the_first_nul() {
        let input = NativeInput::new(b"a\0b");
        assert_eq!(input.original, b"a\0b");
        assert_eq!(input.terminated, b"a\0b\0");
    }
}
