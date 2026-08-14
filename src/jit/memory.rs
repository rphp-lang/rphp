//! Executable-memory boundary shared by native backends.
//!
//! Code is copied into a writable mapping and only then sealed read/execute.
//! No backend may retain a writable alias after publication.

use std::ffi::{c_int, c_void};
use std::io;
use std::ptr::NonNull;

use super::runtime::{MappingReservation, record_system_failure};

const PROT_READ: c_int = 0x01;
const PROT_WRITE: c_int = 0x02;
const PROT_EXEC: c_int = 0x04;
const MAP_PRIVATE: c_int = 0x0002;

#[cfg(target_os = "macos")]
const MAP_ANONYMOUS: c_int = 0x1000;
#[cfg(target_os = "linux")]
const MAP_ANONYMOUS: c_int = 0x0020;

unsafe extern "C" {
    fn mmap(
        address: *mut c_void,
        length: usize,
        protection: c_int,
        flags: c_int,
        file_descriptor: c_int,
        offset: i64,
    ) -> *mut c_void;
    fn mprotect(address: *mut c_void, length: usize, protection: c_int) -> c_int;
    fn munmap(address: *mut c_void, length: usize) -> c_int;
    fn getpagesize() -> c_int;
}

#[cfg(all(target_arch = "aarch64", target_os = "macos"))]
unsafe extern "C" {
    fn sys_icache_invalidate(start: *mut c_void, length: usize);
}

pub(crate) struct ExecutableMemory {
    address: NonNull<u8>,
    mapped_length: usize,
    _reservation: MappingReservation,
}

impl ExecutableMemory {
    pub(crate) fn from_code(code: &[u8]) -> io::Result<Self> {
        if code.is_empty() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "cannot execute an empty code buffer",
            ));
        }

        let page_size = unsafe { getpagesize() };
        if page_size <= 0 {
            record_system_failure();
            return Err(io::Error::last_os_error());
        }
        let page_size = page_size as usize;
        let mapped_length = code
            .len()
            .checked_next_multiple_of(page_size)
            .ok_or_else(|| {
                io::Error::new(io::ErrorKind::InvalidInput, "code buffer is too large")
            })?;
        let reservation = MappingReservation::acquire(mapped_length).ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::OutOfMemory,
                "native JIT executable-memory budget is unavailable",
            )
        })?;

        // Maintain W^X: populate a writable mapping, flush instruction state
        // where the architecture requires it, then permanently seal it RX.
        let raw = unsafe {
            mmap(
                std::ptr::null_mut(),
                mapped_length,
                PROT_READ | PROT_WRITE,
                MAP_PRIVATE | MAP_ANONYMOUS,
                -1,
                0,
            )
        };
        if raw == (-1_isize) as *mut c_void {
            record_system_failure();
            return Err(io::Error::last_os_error());
        }
        let Some(address) = NonNull::new(raw.cast::<u8>()) else {
            record_system_failure();
            unsafe {
                munmap(raw, mapped_length);
            }
            return Err(io::Error::other("mmap returned a null address"));
        };

        unsafe {
            std::ptr::copy_nonoverlapping(code.as_ptr(), address.as_ptr(), code.len());
            flush_instruction_cache(address.as_ptr(), code.len());
        }

        if unsafe { mprotect(raw, mapped_length, PROT_READ | PROT_EXEC) } != 0 {
            let error = io::Error::last_os_error();
            record_system_failure();
            unsafe {
                munmap(raw, mapped_length);
            }
            return Err(error);
        }

        reservation.record_created();

        Ok(Self {
            address,
            mapped_length,
            _reservation: reservation,
        })
    }

    #[inline]
    pub(crate) fn entry(&self) -> *const u8 {
        self.address.as_ptr()
    }
}

#[cfg(all(target_arch = "aarch64", target_os = "macos"))]
unsafe fn flush_instruction_cache(address: *mut u8, length: usize) {
    unsafe { sys_icache_invalidate(address.cast::<c_void>(), length) };
}

#[cfg(all(target_arch = "x86_64", target_os = "linux"))]
unsafe fn flush_instruction_cache(_address: *mut u8, _length: usize) {
    // x86 maintains coherent instruction and data caches. The transition to
    // executable memory still happens through mprotect after the copy.
}

impl Drop for ExecutableMemory {
    fn drop(&mut self) {
        unsafe {
            munmap(self.address.as_ptr().cast::<c_void>(), self.mapped_length);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sealed_mapping_executes_a_minimal_native_return() {
        #[cfg(all(target_arch = "aarch64", target_os = "macos"))]
        let code = 0xd65f_03c0u32.to_le_bytes().to_vec();
        #[cfg(all(target_arch = "x86_64", target_os = "linux"))]
        let code = vec![0xc3];

        let memory = ExecutableMemory::from_code(&code).unwrap();
        let function: unsafe extern "C" fn() = unsafe { std::mem::transmute(memory.entry()) };
        unsafe { function() };
    }
}
