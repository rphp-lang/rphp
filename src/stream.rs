use std::fs::{File, OpenOptions};
use std::io::{self, Cursor, Read, Seek, SeekFrom, Write};
#[cfg(test)]
use std::path::Path;

#[path = "stream/temp.rs"]
mod temp;

use temp::{TempStream, memory_limit as temp_memory_limit};

/// Parsed PHP stream mode. Binary/text and close-on-exec suffixes do not
/// change Rust's byte-oriented file operations, but are accepted for PHP
/// source compatibility.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StreamMode {
    pub read: bool,
    pub write: bool,
    pub append: bool,
    pub create: bool,
    pub truncate: bool,
    pub exclusive: bool,
}

impl StreamMode {
    pub fn parse(mode: &str) -> Option<Self> {
        let mut chars = mode.chars();
        let leading = chars.next()?;
        let mut plus = false;
        for suffix in chars {
            match suffix {
                '+' if !plus => plus = true,
                'b' | 't' | 'e' => {}
                _ => return None,
            }
        }

        let (base_read, base_write, append, create, truncate, exclusive) = match leading {
            'r' => (true, false, false, false, false, false),
            'w' => (false, true, false, true, true, false),
            'a' => (false, true, true, true, false, false),
            'x' => (false, true, false, true, false, true),
            'c' => (false, true, false, true, false, false),
            _ => return None,
        };
        Some(Self {
            read: base_read || plus,
            write: base_write || plus,
            append,
            create,
            truncate,
            exclusive,
        })
    }
}

enum StreamBackend {
    File(File),
    Memory(Cursor<Vec<u8>>),
    Temp(TempStream),
}

/// Initial standard-library stream backend.
///
/// The resource wrapper owns identity and lifecycle; this type owns byte I/O,
/// access policy, position and EOF state. Additional built-in backends can be
/// added without changing the 16-byte PHP `Value` representation.
pub struct PhpStream {
    backend: StreamBackend,
    mode: StreamMode,
    eof: bool,
}

impl PhpStream {
    pub fn open(path: &str, mode: &str) -> io::Result<Self> {
        let mode = StreamMode::parse(mode)
            .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "invalid stream mode"))?;

        if path == "php://memory" {
            return Ok(Self {
                backend: StreamBackend::Memory(Cursor::new(Vec::new())),
                mode,
                eof: false,
            });
        }
        if let Some(max_memory) = temp_memory_limit(path) {
            return Ok(Self {
                backend: StreamBackend::Temp(TempStream::new(max_memory)),
                mode,
                eof: false,
            });
        }

        let file_path = if let Some(path) = path.strip_prefix("file://") {
            path
        } else if path.contains("://") {
            return Err(io::Error::new(
                io::ErrorKind::Unsupported,
                "unsupported stream wrapper",
            ));
        } else {
            path
        };

        let mut options = OpenOptions::new();
        options
            .read(mode.read)
            .write(mode.write)
            .append(mode.append)
            .create(mode.create && !mode.exclusive)
            .create_new(mode.exclusive)
            .truncate(mode.truncate);
        let file = options.open(file_path)?;
        Ok(Self {
            backend: StreamBackend::File(file),
            mode,
            eof: false,
        })
    }

    #[inline]
    pub fn is_readable(&self) -> bool {
        self.mode.read
    }

    #[inline]
    pub fn is_writable(&self) -> bool {
        self.mode.write
    }

    #[inline]
    pub fn is_eof(&self) -> bool {
        self.eof
    }

    pub fn read(&mut self, buffer: &mut [u8]) -> io::Result<usize> {
        if !self.is_readable() {
            return Err(io::Error::new(
                io::ErrorKind::PermissionDenied,
                "stream is not readable",
            ));
        }
        let read = loop {
            let result = match &mut self.backend {
                StreamBackend::File(file) => file.read(buffer),
                StreamBackend::Memory(memory) => memory.read(buffer),
                StreamBackend::Temp(temp) => temp.read(buffer),
            };
            match result {
                Err(error) if error.kind() == io::ErrorKind::Interrupted => continue,
                result => break result?,
            }
        };
        self.eof = read == 0;
        Ok(read)
    }

    pub fn write(&mut self, buffer: &[u8]) -> io::Result<usize> {
        if !self.is_writable() {
            return Err(io::Error::new(
                io::ErrorKind::PermissionDenied,
                "stream is not writable",
            ));
        }
        self.eof = false;
        loop {
            let result = match &mut self.backend {
                StreamBackend::File(file) => file.write(buffer),
                StreamBackend::Memory(memory) => {
                    if self.mode.append {
                        memory.seek(SeekFrom::End(0))?;
                    }
                    memory.write(buffer)
                }
                StreamBackend::Temp(temp) => temp.write(buffer, self.mode.append),
            };
            match result {
                Err(error) if error.kind() == io::ErrorKind::Interrupted => continue,
                result => return result,
            }
        }
    }

    pub fn flush(&mut self) -> io::Result<()> {
        match &mut self.backend {
            StreamBackend::File(file) => file.flush(),
            StreamBackend::Memory(memory) => memory.flush(),
            StreamBackend::Temp(temp) => temp.flush(),
        }
    }

    pub fn seek(&mut self, position: SeekFrom) -> io::Result<u64> {
        let position = match &mut self.backend {
            StreamBackend::File(file) => file.seek(position),
            StreamBackend::Memory(memory) => memory.seek(position),
            StreamBackend::Temp(temp) => temp.seek(position),
        }?;
        self.eof = false;
        Ok(position)
    }

    pub fn position(&mut self) -> io::Result<u64> {
        match &mut self.backend {
            StreamBackend::File(file) => file.stream_position(),
            StreamBackend::Memory(memory) => Ok(memory.position()),
            StreamBackend::Temp(temp) => temp.position(),
        }
    }

    #[cfg(test)]
    fn temp_spill_path(&self) -> Option<&Path> {
        match &self.backend {
            StreamBackend::Temp(temp) => temp.spill_path(),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{PhpStream, StreamMode};
    use std::io::SeekFrom;

    #[test]
    fn parses_php_file_modes_without_platform_dependencies() {
        assert_eq!(
            StreamMode::parse("rb"),
            Some(StreamMode {
                read: true,
                write: false,
                append: false,
                create: false,
                truncate: false,
                exclusive: false,
            })
        );
        let append_update = StreamMode::parse("a+b").unwrap();
        assert!(append_update.read);
        assert!(append_update.write);
        assert!(append_update.append);
        assert!(append_update.create);
        assert!(StreamMode::parse("z").is_none());
        assert!(StreamMode::parse("r++").is_none());
        assert!(StreamMode::parse("").is_none());
    }

    #[test]
    fn memory_stream_preserves_position_eof_and_append_policy() {
        let mut stream = PhpStream::open("php://memory", "w+").unwrap();
        assert_eq!(stream.write(b"hello").unwrap(), 5);
        assert_eq!(stream.position().unwrap(), 5);
        stream.seek(SeekFrom::Start(0)).unwrap();
        let mut buffer = [0; 5];
        assert_eq!(stream.read(&mut buffer).unwrap(), 5);
        assert_eq!(&buffer, b"hello");
        assert!(!stream.is_eof());
        assert_eq!(stream.read(&mut buffer).unwrap(), 0);
        assert!(stream.is_eof());
        assert_eq!(stream.position().unwrap(), 5);
        assert!(stream.is_eof(), "position inspection must preserve EOF");
        stream.seek(SeekFrom::Start(1)).unwrap();
        assert!(!stream.is_eof());

        let mut append = PhpStream::open("php://memory", "a+").unwrap();
        append.write(b"ab").unwrap();
        append.seek(SeekFrom::Start(0)).unwrap();
        append.write(b"c").unwrap();
        append.seek(SeekFrom::Start(0)).unwrap();
        let mut buffer = [0; 3];
        assert_eq!(append.read(&mut buffer).unwrap(), 3);
        assert_eq!(&buffer, b"abc");
    }

    #[test]
    fn temporary_stream_spills_preserves_position_and_removes_its_file() {
        let mut in_memory = PhpStream::open("php://temp", "w+").unwrap();
        assert_eq!(in_memory.write(b"small").unwrap(), 5);
        assert!(in_memory.temp_spill_path().is_none());

        let mut stream = PhpStream::open("php://temp/maxmemory:4", "w+").unwrap();
        assert_eq!(stream.write(b"abcdef").unwrap(), 6);
        let path = stream.temp_spill_path().unwrap().to_path_buf();
        assert!(path.exists());
        assert_eq!(stream.position().unwrap(), 6);
        stream.seek(SeekFrom::Start(1)).unwrap();
        let mut buffer = [0; 4];
        assert_eq!(stream.read(&mut buffer).unwrap(), 4);
        assert_eq!(&buffer, b"bcde");
        drop(stream);
        assert!(!path.exists());
    }
}
