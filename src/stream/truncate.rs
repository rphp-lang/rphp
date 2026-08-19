//! Length changes for writable stream backends.

use std::io::{self, Cursor};

use super::{PhpStream, StreamBackend};

impl PhpStream {
    /// Resize the stream without moving its logical cursor or changing EOF.
    pub fn truncate(&mut self, length: u64) -> io::Result<()> {
        if !self.is_writable() {
            return Err(io::Error::new(
                io::ErrorKind::PermissionDenied,
                "stream is not writable",
            ));
        }
        match &mut self.backend {
            StreamBackend::File(file) => file.set_len(length),
            StreamBackend::Memory(memory) => {
                let length = usize::try_from(length).map_err(|_| {
                    io::Error::new(io::ErrorKind::InvalidInput, "stream size is too large")
                })?;
                resize_memory(memory, length)?;
                self.memory_append_after_truncate = memory.position() > length as u64;
                Ok(())
            }
            StreamBackend::Temp(temp) => temp.truncate(length),
            StreamBackend::Standard(_) => Err(io::Error::new(
                io::ErrorKind::Unsupported,
                "standard stream does not support truncation",
            )),
        }
    }
}

pub(super) fn resize_memory(memory: &mut Cursor<Vec<u8>>, length: usize) -> io::Result<()> {
    let contents = memory.get_mut();
    if length > contents.len() {
        contents
            .try_reserve_exact(length - contents.len())
            .map_err(|_| io::Error::new(io::ErrorKind::OutOfMemory, "stream allocation failed"))?;
        contents.resize(length, 0);
    } else {
        contents.truncate(length);
    }
    Ok(())
}
