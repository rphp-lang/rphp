use std::fs::{File, OpenOptions};
use std::io::{self, Cursor, Read, Seek, SeekFrom, Write};
#[cfg(test)]
use std::path::Path;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};

const DEFAULT_MAX_MEMORY: usize = 2 * 1024 * 1024;

static NEXT_FILE: AtomicU64 = AtomicU64::new(1);

pub(super) fn memory_limit(path: &str) -> Option<usize> {
    if path == "php://temp" {
        return Some(DEFAULT_MAX_MEMORY);
    }
    path.strip_prefix("php://temp/maxmemory:")?.parse().ok()
}

struct TemporaryFile {
    file: Option<File>,
    path: PathBuf,
}

impl TemporaryFile {
    fn create() -> io::Result<Self> {
        for _ in 0..128 {
            let sequence = NEXT_FILE.fetch_add(1, Ordering::Relaxed);
            let timestamp = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_nanos();
            let mut path = std::env::temp_dir();
            path.push(format!(
                ".rphp-stream-{}-{timestamp}-{sequence}.tmp",
                std::process::id()
            ));

            let mut options = OpenOptions::new();
            options.read(true).write(true).create_new(true);
            #[cfg(unix)]
            {
                use std::os::unix::fs::OpenOptionsExt;
                options.mode(0o600);
            }
            match options.open(&path) {
                Ok(file) => {
                    return Ok(Self {
                        file: Some(file),
                        path,
                    });
                }
                Err(error) if error.kind() == io::ErrorKind::AlreadyExists => continue,
                Err(error) => return Err(error),
            }
        }
        Err(io::Error::new(
            io::ErrorKind::AlreadyExists,
            "could not allocate a unique php://temp file",
        ))
    }

    #[inline]
    fn file_mut(&mut self) -> &mut File {
        self.file.as_mut().expect("temporary stream file is open")
    }

    #[inline]
    fn file(&self) -> &File {
        self.file.as_ref().expect("temporary stream file is open")
    }

    #[cfg(test)]
    fn path(&self) -> &Path {
        &self.path
    }
}

impl Drop for TemporaryFile {
    fn drop(&mut self) {
        drop(self.file.take());
        let _ = std::fs::remove_file(&self.path);
    }
}

enum Storage {
    Memory(Cursor<Vec<u8>>),
    File(TemporaryFile),
}

pub(super) struct TempStream {
    storage: Storage,
    max_memory: usize,
    #[cfg(feature = "stream-truncate")]
    append_after_truncate: bool,
}

impl TempStream {
    pub(super) fn new(max_memory: usize) -> Self {
        Self {
            storage: Storage::Memory(Cursor::new(Vec::new())),
            max_memory,
            #[cfg(feature = "stream-truncate")]
            append_after_truncate: false,
        }
    }

    fn spill(&mut self) -> io::Result<()> {
        let Storage::Memory(memory) = &self.storage else {
            return Ok(());
        };
        let position = memory.position();
        let mut file = TemporaryFile::create()?;
        file.file_mut().write_all(memory.get_ref())?;
        file.file_mut().seek(SeekFrom::Start(position))?;
        self.storage = Storage::File(file);
        Ok(())
    }

    pub(super) fn read(&mut self, buffer: &mut [u8]) -> io::Result<usize> {
        match &mut self.storage {
            Storage::Memory(memory) => memory.read(buffer),
            Storage::File(file) => file.file_mut().read(buffer),
        }
    }

    pub(super) fn write(&mut self, buffer: &[u8], append: bool) -> io::Result<usize> {
        if append {
            self.seek(SeekFrom::End(0))?;
        }
        #[cfg(feature = "stream-truncate")]
        if self.append_after_truncate && !append {
            return self.write_after_memory_truncate(buffer);
        }
        if let Storage::Memory(memory) = &self.storage {
            let position = usize::try_from(memory.position()).map_err(|_| {
                io::Error::new(io::ErrorKind::InvalidInput, "stream position is too large")
            })?;
            let required = position.checked_add(buffer.len()).ok_or_else(|| {
                io::Error::new(io::ErrorKind::OutOfMemory, "stream size overflow")
            })?;
            if required.max(memory.get_ref().len()) > self.max_memory {
                self.spill()?;
            }
        }
        match &mut self.storage {
            Storage::Memory(memory) => memory.write(buffer),
            Storage::File(file) => file.file_mut().write(buffer),
        }
    }

    pub(super) fn flush(&mut self) -> io::Result<()> {
        match &mut self.storage {
            Storage::Memory(memory) => memory.flush(),
            Storage::File(file) => file.file_mut().flush(),
        }
    }

    pub(super) fn stat(&self, writable: bool) -> io::Result<super::StreamStat> {
        match &self.storage {
            Storage::Memory(memory) => Ok(super::StreamStat::Memory {
                size: u64::try_from(memory.get_ref().len()).unwrap_or(u64::MAX),
                writable,
            }),
            Storage::File(file) => file.file().metadata().map(super::StreamStat::Filesystem),
        }
    }

    pub(super) fn seek(&mut self, position: SeekFrom) -> io::Result<u64> {
        #[cfg(feature = "stream-truncate")]
        {
            self.append_after_truncate = false;
            return self.seek_without_reset(position);
        }
        #[cfg(not(feature = "stream-truncate"))]
        match &mut self.storage {
            Storage::Memory(memory) => memory.seek(position),
            Storage::File(file) => file.file_mut().seek(position),
        }
    }

    pub(super) fn position(&mut self) -> io::Result<u64> {
        #[cfg(feature = "stream-truncate")]
        {
            return self.position_without_reset();
        }
        #[cfg(not(feature = "stream-truncate"))]
        match &mut self.storage {
            Storage::Memory(memory) => Ok(memory.position()),
            Storage::File(file) => file.file_mut().stream_position(),
        }
    }

    #[cfg(feature = "stream-truncate")]
    pub(super) fn truncate(&mut self, length: u64) -> io::Result<()> {
        match &mut self.storage {
            Storage::Memory(memory) => {
                let length = usize::try_from(length).map_err(|_| {
                    io::Error::new(io::ErrorKind::InvalidInput, "stream size is too large")
                })?;
                if length > self.max_memory {
                    self.spill()?;
                    let Storage::File(file) = &mut self.storage else {
                        unreachable!("temporary stream spill must create a file")
                    };
                    file.file_mut().set_len(length as u64)?;
                    self.append_after_truncate = false;
                    return Ok(());
                }
                super::truncate::resize_memory(memory, length)?;
                self.append_after_truncate = memory.position() > length as u64;
                Ok(())
            }
            Storage::File(file) => {
                file.file_mut().set_len(length)?;
                self.append_after_truncate = false;
                Ok(())
            }
        }
    }

    #[cfg(feature = "stream-truncate")]
    fn write_after_memory_truncate(&mut self, buffer: &[u8]) -> io::Result<usize> {
        let logical_position = self.position_without_reset()?;
        let current_length = self.length()?;
        let required = current_length
            .checked_add(buffer.len() as u64)
            .ok_or_else(|| io::Error::new(io::ErrorKind::OutOfMemory, "stream size overflow"))?;
        if matches!(self.storage, Storage::Memory(_)) && required > self.max_memory as u64 {
            self.spill()?;
        }
        self.seek_without_reset(SeekFrom::End(0))?;
        let result = match &mut self.storage {
            Storage::Memory(memory) => memory.write(buffer),
            Storage::File(file) => file.file_mut().write(buffer),
        };
        match result {
            Ok(written) => {
                self.seek_without_reset(SeekFrom::Start(
                    logical_position.saturating_add(written as u64),
                ))?;
                Ok(written)
            }
            Err(error) => {
                self.seek_without_reset(SeekFrom::Start(logical_position))?;
                Err(error)
            }
        }
    }

    #[cfg(feature = "stream-truncate")]
    fn length(&mut self) -> io::Result<u64> {
        match &mut self.storage {
            Storage::Memory(memory) => Ok(memory.get_ref().len() as u64),
            Storage::File(file) => Ok(file.file_mut().metadata()?.len()),
        }
    }

    #[cfg(feature = "stream-truncate")]
    fn position_without_reset(&mut self) -> io::Result<u64> {
        match &mut self.storage {
            Storage::Memory(memory) => Ok(memory.position()),
            Storage::File(file) => file.file_mut().stream_position(),
        }
    }

    #[cfg(feature = "stream-truncate")]
    fn seek_without_reset(&mut self, position: SeekFrom) -> io::Result<u64> {
        match &mut self.storage {
            Storage::Memory(memory) => memory.seek(position),
            Storage::File(file) => file.file_mut().seek(position),
        }
    }

    #[cfg(test)]
    pub(super) fn spill_path(&self) -> Option<&Path> {
        match &self.storage {
            Storage::Memory(_) => None,
            Storage::File(file) => Some(file.path()),
        }
    }
}
