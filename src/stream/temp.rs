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
}

impl TempStream {
    pub(super) fn new(max_memory: usize) -> Self {
        Self {
            storage: Storage::Memory(Cursor::new(Vec::new())),
            max_memory,
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

    pub(super) fn seek(&mut self, position: SeekFrom) -> io::Result<u64> {
        match &mut self.storage {
            Storage::Memory(memory) => memory.seek(position),
            Storage::File(file) => file.file_mut().seek(position),
        }
    }

    pub(super) fn position(&mut self) -> io::Result<u64> {
        match &mut self.storage {
            Storage::Memory(memory) => Ok(memory.position()),
            Storage::File(file) => file.file_mut().stream_position(),
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
