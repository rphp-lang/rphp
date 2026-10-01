//! Cold boundary for process-global native state.
//!
//! Environment variables, locales, and GNU gettext catalogs are all backed by
//! process-global native state. Keeping their raw calls in one place makes the
//! single-threaded VM assumption explicit and prevents unsafe call sites from
//! spreading through otherwise safe standard-library handlers.

#[cfg(target_os = "linux")]
use std::cell::{Cell, RefCell};
use std::ffi::OsStr;
#[cfg(target_os = "linux")]
use std::sync::atomic::{AtomicU64, Ordering};

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

#[derive(Clone, Copy)]
pub(crate) enum NativeCtypeClass {
    Alnum,
    Alpha,
    Blank,
    Control,
    Digit,
    Graph,
    Lower,
    Print,
    Punctuation,
    Space,
    Upper,
    HexDigit,
}

// Process-global locale mutations invalidate every thread's byte projection.
// Native mutation retains this module's existing single-VM-thread contract.
#[cfg(target_os = "linux")]
static CTYPE_GENERATION: AtomicU64 = AtomicU64::new(1);

#[cfg(target_os = "linux")]
pub(super) struct NativeCtypeTables {
    generation: Cell<u64>,
    lowercase: [Cell<u8>; 256],
    classes: [Cell<u16>; 256],
}

#[cfg(target_os = "linux")]
thread_local! {
    static BYTE_CTYPE: NativeCtypeTables = const { NativeCtypeTables {
        generation: Cell::new(0),
        lowercase: [const { Cell::new(0) }; 256],
        classes: [const { Cell::new(0) }; 256],
    } };
}

/// Safe, fixed-width projection of the POSIX `rusage` fields exposed by PHP.
#[cfg(target_os = "linux")]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(super) struct NativeResourceUsage {
    pub(super) output_blocks: i64,
    pub(super) input_blocks: i64,
    pub(super) messages_sent: i64,
    pub(super) messages_received: i64,
    pub(super) maximum_resident_set: i64,
    pub(super) shared_memory: i64,
    pub(super) unshared_data: i64,
    pub(super) minor_page_faults: i64,
    pub(super) major_page_faults: i64,
    pub(super) signals: i64,
    pub(super) voluntary_context_switches: i64,
    pub(super) involuntary_context_switches: i64,
    pub(super) swaps: i64,
    pub(super) user_microseconds: i64,
    pub(super) user_seconds: i64,
    pub(super) system_microseconds: i64,
    pub(super) system_seconds: i64,
}

/// Owned projection of the POSIX password record. Native pointers never leave
/// `invoke_native`; every field is copied while the re-entrant lookup buffer is
/// alive.
#[cfg(target_os = "linux")]
pub(super) struct NativePasswd {
    pub(super) name: Vec<u8>,
    pub(super) password: Vec<u8>,
    pub(super) uid: u32,
    pub(super) gid: u32,
    pub(super) gecos: Vec<u8>,
    pub(super) directory: Vec<u8>,
    pub(super) shell: Vec<u8>,
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
    #[cfg(target_os = "linux")]
    FillCtypeTables {
        tables: &'a NativeCtypeTables,
        generation: u64,
    },
    #[cfg(target_os = "linux")]
    GetResourceUsage {
        children: bool,
        result: &'a Cell<Option<NativeResourceUsage>>,
    },
    #[cfg(target_os = "linux")]
    GetUserId {
        effective: bool,
        result: &'a Cell<u32>,
    },
    #[cfg(target_os = "linux")]
    GetPasswordByUserId {
        uid: u32,
        result: &'a RefCell<Option<NativePasswd>>,
    },
    #[cfg(target_os = "linux")]
    IsTerminal {
        descriptor: c_int,
        result: &'a Cell<bool>,
        error: &'a Cell<c_int>,
    },
    #[cfg(target_os = "linux")]
    StrError(c_int),
    #[cfg(target_os = "linux")]
    PcntlAlarm {
        seconds: u32,
        result: &'a Cell<u32>,
    },
    #[cfg(target_os = "linux")]
    PcntlSignal {
        signal: c_int,
        handler: usize,
        restart_syscalls: bool,
        result: &'a Cell<bool>,
    },
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
    // getrusage and getpwuid_r receive initialized-size out pointers and write
    // synchronously; password strings are copied before their scratch buffer
    // is dropped. RPHP executes process-global locale/catalog/environment
    // mutations on its single VM thread, so no concurrent Rust environment
    // access is possible. Byte tables pass only 0..255 to libc ctype functions
    // and finish filling without callbacks before publishing their generation.
    unsafe {
        match call {
            NativeCall::SetEnvironment(key, value) => {
                std::env::set_var(key, value);
                None
            }
            #[cfg(target_os = "linux")]
            call => {
                if let NativeCall::FillCtypeTables { tables, generation } = call {
                    for byte in 0..=u8::MAX {
                        let native_byte = c_int::from(byte);
                        let matches = [
                            libc::isalnum(native_byte),
                            libc::isalpha(native_byte),
                            libc::isblank(native_byte),
                            libc::iscntrl(native_byte),
                            libc::isdigit(native_byte),
                            libc::isgraph(native_byte),
                            libc::islower(native_byte),
                            libc::isprint(native_byte),
                            libc::ispunct(native_byte),
                            libc::isspace(native_byte),
                            libc::isupper(native_byte),
                            libc::isxdigit(native_byte),
                        ];
                        let mask = matches.iter().enumerate().fold(0u16, |mask, (bit, value)| {
                            mask | (u16::from(*value != 0) << bit)
                        });
                        tables.classes[usize::from(byte)].set(mask);
                        tables.lowercase[usize::from(byte)].set(libc::tolower(native_byte) as u8);
                    }
                    tables.generation.set(generation);
                    return Some(Vec::new());
                }
                if let NativeCall::GetResourceUsage { children, result } = call {
                    let mut usage = std::mem::MaybeUninit::<libc::rusage>::uninit();
                    let who = if children {
                        libc::RUSAGE_CHILDREN
                    } else {
                        libc::RUSAGE_SELF
                    };
                    if libc::getrusage(who, usage.as_mut_ptr()) != 0 {
                        result.set(None);
                        return None;
                    }
                    let usage = usage.assume_init();
                    result.set(Some(NativeResourceUsage {
                        output_blocks: usage.ru_oublock as i64,
                        input_blocks: usage.ru_inblock as i64,
                        messages_sent: usage.ru_msgsnd as i64,
                        messages_received: usage.ru_msgrcv as i64,
                        maximum_resident_set: usage.ru_maxrss as i64,
                        shared_memory: usage.ru_ixrss as i64,
                        unshared_data: usage.ru_idrss as i64,
                        minor_page_faults: usage.ru_minflt as i64,
                        major_page_faults: usage.ru_majflt as i64,
                        signals: usage.ru_nsignals as i64,
                        voluntary_context_switches: usage.ru_nvcsw as i64,
                        involuntary_context_switches: usage.ru_nivcsw as i64,
                        swaps: usage.ru_nswap as i64,
                        user_microseconds: usage.ru_utime.tv_usec as i64,
                        user_seconds: usage.ru_utime.tv_sec as i64,
                        system_microseconds: usage.ru_stime.tv_usec as i64,
                        system_seconds: usage.ru_stime.tv_sec as i64,
                    }));
                    return Some(Vec::new());
                }
                if let NativeCall::GetUserId { effective, result } = call {
                    result.set(if effective {
                        libc::geteuid()
                    } else {
                        libc::getuid()
                    });
                    return Some(Vec::new());
                }
                if let NativeCall::IsTerminal {
                    descriptor,
                    result,
                    error,
                } = call
                {
                    let terminal = libc::isatty(descriptor) == 1;
                    result.set(terminal);
                    if !terminal {
                        error.set(
                            std::io::Error::last_os_error()
                                .raw_os_error()
                                .unwrap_or(libc::EBADF),
                        );
                    }
                    return Some(Vec::new());
                }
                if let NativeCall::StrError(error) = call {
                    let message = libc::strerror(error);
                    return (!message.is_null())
                        .then(|| CStr::from_ptr(message).to_bytes().to_vec());
                }
                if let NativeCall::PcntlAlarm { seconds, result } = call {
                    result.set(libc::alarm(seconds));
                    return Some(Vec::new());
                }
                if let NativeCall::PcntlSignal {
                    signal,
                    handler,
                    restart_syscalls,
                    result,
                } = call
                {
                    let mut action = std::mem::zeroed::<libc::sigaction>();
                    action.sa_sigaction = handler;
                    action.sa_flags = if restart_syscalls {
                        libc::SA_RESTART
                    } else {
                        0
                    };
                    libc::sigemptyset(&mut action.sa_mask);
                    result.set(libc::sigaction(signal, &action, std::ptr::null_mut()) == 0);
                    return Some(Vec::new());
                }
                if let NativeCall::GetPasswordByUserId { uid, result } = call {
                    const MAX_PASSWORD_BUFFER: usize = 16 * 1024 * 1024;
                    let suggested = libc::sysconf(libc::_SC_GETPW_R_SIZE_MAX);
                    let initial = usize::try_from(suggested)
                        .ok()
                        .filter(|size| *size > 0)
                        .unwrap_or(16 * 1024)
                        .clamp(1024, MAX_PASSWORD_BUFFER);
                    let mut buffer = Vec::new();
                    if buffer.try_reserve_exact(initial).is_err() {
                        result.replace(None);
                        return None;
                    }
                    buffer.resize(initial, 0);
                    loop {
                        let mut record = std::mem::MaybeUninit::<libc::passwd>::uninit();
                        let mut found = std::ptr::null_mut::<libc::passwd>();
                        let status = libc::getpwuid_r(
                            uid,
                            record.as_mut_ptr(),
                            buffer.as_mut_ptr().cast(),
                            buffer.len(),
                            &mut found,
                        );
                        if status == libc::ERANGE {
                            let Some(next) = buffer
                                .len()
                                .checked_mul(2)
                                .filter(|next| *next <= MAX_PASSWORD_BUFFER)
                            else {
                                result.replace(None);
                                return None;
                            };
                            if buffer.try_reserve_exact(next - buffer.len()).is_err() {
                                result.replace(None);
                                return None;
                            }
                            buffer.resize(next, 0);
                            continue;
                        }
                        if status != 0 || found.is_null() {
                            result.replace(None);
                            return None;
                        }
                        let record = record.assume_init();
                        let copy = |pointer: *const c_char| {
                            if pointer.is_null() {
                                Vec::new()
                            } else {
                                CStr::from_ptr(pointer).to_bytes().to_vec()
                            }
                        };
                        result.replace(Some(NativePasswd {
                            name: copy(record.pw_name),
                            password: copy(record.pw_passwd),
                            uid: record.pw_uid,
                            gid: record.pw_gid,
                            gecos: copy(record.pw_gecos),
                            directory: copy(record.pw_dir),
                            shell: copy(record.pw_shell),
                        }));
                        return Some(Vec::new());
                    }
                }
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
                    NativeCall::SetLocale { category, locale } => {
                        let result = native_setlocale(
                            category,
                            locale.map_or(std::ptr::null(), NativeInput::as_ptr),
                        );
                        if !result.is_null()
                            && locale.is_some()
                            && matches!(category, libc::LC_CTYPE | libc::LC_ALL)
                        {
                            CTYPE_GENERATION.fetch_add(1, Ordering::Relaxed);
                        }
                        (result, None, None)
                    }
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
                    NativeCall::FillCtypeTables { .. } => unreachable!(),
                    NativeCall::GetResourceUsage { .. } => unreachable!(),
                    NativeCall::GetUserId { .. } => unreachable!(),
                    NativeCall::GetPasswordByUserId { .. } => unreachable!(),
                    NativeCall::IsTerminal { .. } => unreachable!(),
                    NativeCall::StrError(..) => unreachable!(),
                    NativeCall::PcntlAlarm { .. } => unreachable!(),
                    NativeCall::PcntlSignal { .. } => unreachable!(),
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
pub(super) fn user_id(effective: bool) -> u32 {
    let result = Cell::new(0);
    let _ = invoke_native(NativeCall::GetUserId {
        effective,
        result: &result,
    });
    result.get()
}

#[cfg(target_os = "linux")]
pub(super) fn password_by_user_id(uid: u32) -> Option<NativePasswd> {
    let result = RefCell::new(None);
    let _ = invoke_native(NativeCall::GetPasswordByUserId {
        uid,
        result: &result,
    });
    result.into_inner()
}

#[cfg(target_os = "linux")]
pub(super) fn descriptor_is_terminal(descriptor: c_int) -> (bool, c_int) {
    let result = Cell::new(false);
    let error = Cell::new(0);
    let _ = invoke_native(NativeCall::IsTerminal {
        descriptor,
        result: &result,
        error: &error,
    });
    (result.get(), error.get())
}

#[cfg(target_os = "linux")]
pub(super) fn error_message(error: c_int) -> Option<Vec<u8>> {
    invoke_native(NativeCall::StrError(error))
}

#[cfg(target_os = "linux")]
pub(super) fn pcntl_alarm(seconds: u32) -> u32 {
    let result = Cell::new(0);
    let _ = invoke_native(NativeCall::PcntlAlarm {
        seconds,
        result: &result,
    });
    result.get()
}

#[cfg(target_os = "linux")]
pub(super) fn pcntl_signal(signal: c_int, handler: usize, restart_syscalls: bool) -> bool {
    let result = Cell::new(false);
    let _ = invoke_native(NativeCall::PcntlSignal {
        signal,
        handler,
        restart_syscalls,
        result: &result,
    });
    result.get()
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

#[cfg(target_os = "linux")]
#[cfg_attr(target_os = "linux", unsafe(link_section = ".rphp_cold"))]
pub(super) fn resource_usage(children: bool) -> Option<NativeResourceUsage> {
    let result = Cell::new(None);
    let _ = invoke_native(NativeCall::GetResourceUsage {
        children,
        result: &result,
    });
    result.get()
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

#[cfg(target_os = "linux")]
impl NativeCtypeTables {
    #[inline]
    fn ensure_current(&self) {
        let generation = CTYPE_GENERATION.load(Ordering::Relaxed);
        if self.generation.get() != generation {
            let _ = invoke_native(NativeCall::FillCtypeTables {
                tables: self,
                generation,
            });
        }
    }
}

#[inline]
pub(crate) fn ctype_byte_matches(class: NativeCtypeClass, byte: u8) -> bool {
    #[cfg(target_os = "linux")]
    {
        return BYTE_CTYPE.with(|tables| {
            tables.ensure_current();
            tables.classes[usize::from(byte)].get() & (1 << class as u8) != 0
        });
    }
    #[cfg(not(target_os = "linux"))]
    match class {
        NativeCtypeClass::Alnum => byte.is_ascii_alphanumeric(),
        NativeCtypeClass::Alpha => byte.is_ascii_alphabetic(),
        NativeCtypeClass::Blank => matches!(byte, b'\t' | b' '),
        NativeCtypeClass::Control => byte.is_ascii_control(),
        NativeCtypeClass::Digit => byte.is_ascii_digit(),
        NativeCtypeClass::Graph => byte.is_ascii_graphic(),
        NativeCtypeClass::Lower => byte.is_ascii_lowercase(),
        NativeCtypeClass::Print => byte.is_ascii_graphic() || byte == b' ',
        NativeCtypeClass::Punctuation => byte.is_ascii_punctuation(),
        NativeCtypeClass::Space => byte.is_ascii_whitespace(),
        NativeCtypeClass::Upper => byte.is_ascii_uppercase(),
        NativeCtypeClass::HexDigit => byte.is_ascii_hexdigit(),
    }
}

#[inline]
pub(crate) fn ctype_lowercase_byte(byte: u8) -> u8 {
    #[cfg(target_os = "linux")]
    {
        return BYTE_CTYPE.with(|tables| {
            tables.ensure_current();
            tables.lowercase[usize::from(byte)].get()
        });
    }
    #[cfg(not(target_os = "linux"))]
    byte.to_ascii_lowercase()
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

    #[test]
    fn byte_tables_track_only_successful_character_locale_mutations() {
        let original = set_process_locale(i64::from(libc::LC_ALL), None).unwrap();
        struct RestoreLocale(Vec<u8>);
        impl Drop for RestoreLocale {
            fn drop(&mut self) {
                set_process_locale(i64::from(libc::LC_ALL), Some(&self.0));
            }
        }
        let _restore = RestoreLocale(original);
        assert!(set_process_locale(i64::from(libc::LC_ALL), Some(b"C")).is_some());
        for byte in 0..=u8::MAX {
            let expectations = [
                (NativeCtypeClass::Alnum, byte.is_ascii_alphanumeric()),
                (NativeCtypeClass::Alpha, byte.is_ascii_alphabetic()),
                (NativeCtypeClass::Blank, matches!(byte, b'\t' | b' ')),
                (NativeCtypeClass::Control, byte.is_ascii_control()),
                (NativeCtypeClass::Digit, byte.is_ascii_digit()),
                (NativeCtypeClass::Graph, byte.is_ascii_graphic()),
                (NativeCtypeClass::Lower, byte.is_ascii_lowercase()),
                (
                    NativeCtypeClass::Print,
                    byte.is_ascii_graphic() || byte == b' ',
                ),
                (NativeCtypeClass::Punctuation, byte.is_ascii_punctuation()),
                (
                    NativeCtypeClass::Space,
                    matches!(byte, b'\t'..=b'\r' | b' '),
                ),
                (NativeCtypeClass::Upper, byte.is_ascii_uppercase()),
                (NativeCtypeClass::HexDigit, byte.is_ascii_hexdigit()),
            ];
            for (class, expected) in expectations {
                assert_eq!(ctype_byte_matches(class, byte), expected, "byte {byte}");
            }
            assert_eq!(ctype_lowercase_byte(byte), byte.to_ascii_lowercase());
        }
        let generation = CTYPE_GENERATION.load(Ordering::Relaxed);
        for (category, name) in [
            (libc::LC_CTYPE, None),
            (libc::LC_ALL, None),
            (libc::LC_CTYPE, Some(b"invalid_RPHP".as_slice())),
            (libc::LC_ALL, Some(b"invalid_RPHP".as_slice())),
            (libc::LC_NUMERIC, Some(b"C".as_slice())),
        ] {
            set_process_locale(i64::from(category), name);
            assert_eq!(CTYPE_GENERATION.load(Ordering::Relaxed), generation);
        }
        for category in [libc::LC_CTYPE, libc::LC_ALL] {
            let old = CTYPE_GENERATION.load(Ordering::Relaxed);
            assert!(set_process_locale(i64::from(category), Some(b"C")).is_some());
            assert_ne!(CTYPE_GENERATION.load(Ordering::Relaxed), old);
            assert_eq!(BYTE_CTYPE.with(|tables| tables.generation.get()), old);
            assert_eq!(ctype_lowercase_byte(b'A'), b'a');
            assert_eq!(
                BYTE_CTYPE.with(|tables| tables.generation.get()),
                CTYPE_GENERATION.load(Ordering::Relaxed)
            );
        }
    }
}
