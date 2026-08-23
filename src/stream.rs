use std::fs::{File, OpenOptions};
use std::io::{self, Cursor, Read, Seek, SeekFrom, Write};
#[cfg(test)]
use std::path::Path;

#[cfg(feature = "stream-context")]
use crate::value::PhpArray;

// Keep the established Linux translation-unit layout. On Apple targets the
// CSV code is included here so its cold custom section cannot perturb the hot
// quick-dispatch function layout measured by the performance admission gate.
#[cfg(any(feature = "stream-contents", feature = "file-contents"))]
#[path = "stream/contents.rs"]
mod contents;
#[cfg(not(target_vendor = "apple"))]
#[path = "stream/csv.rs"]
mod csv;
#[cfg(feature = "stream-line")]
#[path = "stream/get_line.rs"]
mod get_line;
#[path = "stream/temp.rs"]
mod temp;
#[cfg(feature = "stream-truncate")]
#[path = "stream/truncate.rs"]
mod truncate;

#[cfg(not(target_vendor = "apple"))]
use csv::CsvParser;
use temp::{TempStream, memory_limit as temp_memory_limit};

#[cfg(target_vendor = "apple")]
include!("stream/csv.rs");

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
    Standard(StandardStream),
}

#[derive(Clone, Copy)]
enum StandardStream {
    Input,
    Output,
    Error,
}

/// Request-owned context data shared by a context resource and streams opened
/// from it. This type is absent from builds that do not expose stream contexts.
#[cfg(feature = "stream-context")]
#[derive(Clone)]
pub(crate) struct StreamContext {
    pub(crate) options: PhpArray,
    pub(crate) params: PhpArray,
}

/// Initial standard-library stream backend.
///
/// The resource wrapper owns identity and lifecycle; this type owns byte I/O,
/// access policy, position and EOF state. Additional built-in backends can be
/// added without changing the 16-byte PHP `Value` representation.
pub struct PhpStream {
    backend: StreamBackend,
    mode: StreamMode,
    reported_mode: String,
    uri: String,
    eof: bool,
    #[cfg(feature = "stream-truncate")]
    memory_append_after_truncate: bool,
    #[cfg(feature = "stream-context")]
    context: Option<Box<StreamContext>>,
}

/// Stable metadata exposed by the currently admitted seekable backends.
/// Optional status fields mirror PHP's backend-specific key set: plain files
/// and memory streams publish blocking/EOF state, while `php://temp` exposes
/// only its wrapper identity and seekability fields.
pub struct StreamMetadata<'a> {
    pub timed_out: Option<bool>,
    pub blocked: Option<bool>,
    pub eof: Option<bool>,
    pub wrapper_type: &'static str,
    pub stream_type: &'static str,
    pub mode: &'a str,
    pub unread_bytes: usize,
    pub seekable: bool,
    pub uri: &'a str,
}

impl PhpStream {
    pub(crate) fn standard_input() -> Self {
        Self::standard(StandardStream::Input)
    }

    pub(crate) fn standard_output() -> Self {
        Self::standard(StandardStream::Output)
    }

    pub(crate) fn standard_error() -> Self {
        Self::standard(StandardStream::Error)
    }

    fn standard(stream: StandardStream) -> Self {
        let (mode, reported_mode, uri) = match stream {
            StandardStream::Input => (
                StreamMode {
                    read: true,
                    write: false,
                    append: false,
                    create: false,
                    truncate: false,
                    exclusive: false,
                },
                "rb",
                "php://stdin",
            ),
            StandardStream::Output => (
                StreamMode {
                    read: false,
                    write: true,
                    append: false,
                    create: false,
                    truncate: false,
                    exclusive: false,
                },
                "wb",
                "php://stdout",
            ),
            StandardStream::Error => (
                StreamMode {
                    read: false,
                    write: true,
                    append: false,
                    create: false,
                    truncate: false,
                    exclusive: false,
                },
                "wb",
                "php://stderr",
            ),
        };
        Self {
            backend: StreamBackend::Standard(stream),
            mode,
            reported_mode: reported_mode.to_string(),
            uri: uri.to_string(),
            eof: false,
            #[cfg(feature = "stream-truncate")]
            memory_append_after_truncate: false,
            #[cfg(feature = "stream-context")]
            context: None,
        }
    }

    pub fn open(path: &str, mode: &str) -> io::Result<Self> {
        let requested_mode = mode;
        let mode = StreamMode::parse(requested_mode)
            .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "invalid stream mode"))?;

        if path == "php://memory" {
            return Ok(Self {
                backend: StreamBackend::Memory(Cursor::new(Vec::new())),
                mode,
                reported_mode: php_memory_mode(mode).to_string(),
                uri: path.to_string(),
                eof: false,
                #[cfg(feature = "stream-truncate")]
                memory_append_after_truncate: false,
                #[cfg(feature = "stream-context")]
                context: None,
            });
        }
        if let Some(max_memory) = temp_memory_limit(path) {
            return Ok(Self {
                backend: StreamBackend::Temp(TempStream::new(max_memory)),
                mode,
                reported_mode: php_memory_mode(mode).to_string(),
                uri: path.to_string(),
                eof: false,
                #[cfg(feature = "stream-truncate")]
                memory_append_after_truncate: false,
                #[cfg(feature = "stream-context")]
                context: None,
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
            reported_mode: requested_mode.to_string(),
            uri: path.to_string(),
            eof: false,
            #[cfg(feature = "stream-truncate")]
            memory_append_after_truncate: false,
            #[cfg(feature = "stream-context")]
            context: None,
        })
    }

    #[cfg(feature = "stream-context")]
    pub(crate) fn context_mut(&mut self) -> &mut StreamContext {
        self.context
            .get_or_insert_with(|| {
                Box::new(StreamContext {
                    options: PhpArray::new(),
                    params: PhpArray::new(),
                })
            })
            .as_mut()
    }

    #[cfg(feature = "stream-context")]
    pub(crate) fn context(&self) -> Option<&StreamContext> {
        self.context.as_deref()
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

    fn read_backend(&mut self, buffer: &mut [u8]) -> io::Result<usize> {
        loop {
            let result = match &mut self.backend {
                StreamBackend::File(file) => file.read(buffer),
                StreamBackend::Memory(memory) => memory.read(buffer),
                StreamBackend::Temp(temp) => temp.read(buffer),
                StreamBackend::Standard(StandardStream::Input) => io::stdin().lock().read(buffer),
                StreamBackend::Standard(_) => Err(io::Error::new(
                    io::ErrorKind::PermissionDenied,
                    "standard stream is not readable",
                )),
            };
            match result {
                Err(error) if error.kind() == io::ErrorKind::Interrupted => continue,
                result => return result,
            }
        }
    }

    fn seek_backend(&mut self, position: SeekFrom) -> io::Result<u64> {
        match &mut self.backend {
            StreamBackend::File(file) => file.seek(position),
            StreamBackend::Memory(memory) => memory.seek(position),
            StreamBackend::Temp(temp) => temp.seek(position),
            StreamBackend::Standard(_) => Err(io::Error::new(
                io::ErrorKind::Unsupported,
                "standard stream does not support seeking",
            )),
        }
    }

    pub fn read(&mut self, buffer: &mut [u8]) -> io::Result<usize> {
        if !self.is_readable() {
            return Err(io::Error::new(
                io::ErrorKind::PermissionDenied,
                "stream is not readable",
            ));
        }
        let read = self.read_backend(buffer)?;
        self.eof = read == 0;
        Ok(read)
    }

    /// Read one line without retaining a hidden userspace buffer. A stack
    /// chunk amortizes file reads; bytes beyond the first newline are returned
    /// to the seekable backend so `ftell`, writes and later reads observe the
    /// exact PHP cursor. `length` includes PHP's reserved terminator byte.
    pub fn read_line(
        &mut self,
        buffer: &mut Vec<u8>,
        length: Option<usize>,
    ) -> io::Result<Option<usize>> {
        let maximum = match length {
            Some(length) => match length.checked_sub(1) {
                Some(maximum) if maximum > 0 => maximum,
                _ => return Ok(None),
            },
            None => usize::MAX,
        };
        self.read_line_max(buffer, maximum)
    }

    /// Read and parse one CSV record. A positive length bounds only the first
    /// physical read, matching PHP: an enclosure left open at that boundary
    /// continues through the remainder of the physical record. Zero/None is
    /// represented by `None` and therefore reads without a byte ceiling.
    #[cold]
    #[inline(never)]
    #[cfg_attr(target_vendor = "apple", unsafe(link_section = "__TEXT,__rphp_csv"))]
    pub fn read_csv_record(
        &mut self,
        length: Option<usize>,
        separator: u8,
        enclosure: u8,
        escape: Option<u8>,
    ) -> io::Result<Option<Vec<Option<Vec<u8>>>>> {
        let mut segment = Vec::new();
        let maximum = length.unwrap_or(usize::MAX);
        let initial = match self.read_line_max(&mut segment, maximum) {
            Ok(initial) => initial,
            Err(error) => {
                self.rewind_csv_record(segment.len());
                return Err(error);
            }
        };
        let Some(_) = initial else {
            return Ok(None);
        };

        let mut consumed = segment.len();
        let mut parser = CsvParser::new(separator, enclosure, escape);
        if let Err(error) = parser.push_segment(&segment) {
            self.rewind_csv_record(consumed);
            return Err(error);
        }

        while parser.needs_continuation() && !self.eof {
            let next = match self.read_line_max(&mut segment, usize::MAX) {
                Ok(next) => next,
                Err(error) => {
                    self.rewind_csv_record(consumed.saturating_add(segment.len()));
                    return Err(error);
                }
            };
            let Some(_) = next else {
                break;
            };
            let Some(next_consumed) = consumed.checked_add(segment.len()) else {
                self.rewind_csv_record(consumed);
                return Err(io::Error::new(
                    io::ErrorKind::OutOfMemory,
                    "CSV record length overflow",
                ));
            };
            consumed = next_consumed;
            if let Err(error) = parser.push_segment(&segment) {
                self.rewind_csv_record(consumed);
                return Err(error);
            }
        }

        match parser.finish(self.eof) {
            Ok(fields) => Ok(Some(fields)),
            Err(error) => {
                self.rewind_csv_record(consumed);
                Err(error)
            }
        }
    }

    fn read_line_max(&mut self, buffer: &mut Vec<u8>, maximum: usize) -> io::Result<Option<usize>> {
        if !self.is_readable() {
            return Err(io::Error::new(
                io::ErrorKind::PermissionDenied,
                "stream is not readable",
            ));
        }
        buffer.clear();
        if maximum == 0 {
            return Ok(None);
        }
        let mut chunk = [0u8; 8 * 1024];

        while buffer.len() < maximum {
            let requested = chunk.len().min(maximum - buffer.len());
            let read = self.read_backend(&mut chunk[..requested])?;
            if read == 0 {
                self.eof = true;
                return Ok((!buffer.is_empty()).then_some(buffer.len()));
            }
            self.eof = false;

            let consumed = chunk[..read]
                .iter()
                .position(|byte| *byte == b'\n')
                .map_or(read, |newline| newline + 1);
            if buffer.try_reserve(consumed).is_err() {
                let rewind = i64::try_from(read).expect("line chunk fits in i64");
                self.seek_backend(SeekFrom::Current(-rewind))?;
                return Err(io::Error::new(
                    io::ErrorKind::OutOfMemory,
                    "line buffer allocation failed",
                ));
            }
            buffer.extend_from_slice(&chunk[..consumed]);

            if consumed < read {
                let unread = i64::try_from(read - consumed).expect("line chunk fits in i64");
                self.seek_backend(SeekFrom::Current(-unread))?;
            }
            if consumed < read || chunk[consumed - 1] == b'\n' {
                return Ok(Some(buffer.len()));
            }
        }
        Ok(Some(buffer.len()))
    }

    #[cold]
    #[inline(never)]
    #[cfg_attr(target_vendor = "apple", unsafe(link_section = "__TEXT,__rphp_csv"))]
    fn rewind_csv_record(&mut self, consumed: usize) {
        let Ok(consumed) = i64::try_from(consumed) else {
            return;
        };
        if self.seek_backend(SeekFrom::Current(-consumed)).is_ok() {
            self.eof = false;
        }
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
                    #[cfg(feature = "stream-truncate")]
                    if self.memory_append_after_truncate && !self.mode.append {
                        let logical_position = memory.position();
                        memory.set_position(memory.get_ref().len() as u64);
                        let result = memory.write(buffer);
                        if let Ok(written) = result {
                            memory.set_position(logical_position.saturating_add(written as u64));
                        } else {
                            memory.set_position(logical_position);
                        }
                        result
                    } else {
                        memory.write(buffer)
                    }
                    #[cfg(not(feature = "stream-truncate"))]
                    memory.write(buffer)
                }
                StreamBackend::Temp(temp) => temp.write(buffer, self.mode.append),
                StreamBackend::Standard(StandardStream::Output) => {
                    io::stdout().lock().write(buffer)
                }
                StreamBackend::Standard(StandardStream::Error) => io::stderr().lock().write(buffer),
                StreamBackend::Standard(StandardStream::Input) => Err(io::Error::new(
                    io::ErrorKind::PermissionDenied,
                    "standard stream is not writable",
                )),
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
            StreamBackend::Standard(StandardStream::Input) => Ok(()),
            StreamBackend::Standard(StandardStream::Output) => io::stdout().lock().flush(),
            StreamBackend::Standard(StandardStream::Error) => io::stderr().lock().flush(),
        }
    }

    /// Apply PHP flock operation bits to a regular file stream. Locks are
    /// released by explicit LOCK_UN or automatically when the stream closes.
    pub fn lock(&self, operation: i64) -> io::Result<()> {
        let nonblocking = operation & 4 != 0;
        let operation = operation & !4;
        let StreamBackend::File(file) = &self.backend else {
            return Err(io::Error::new(
                io::ErrorKind::Unsupported,
                "file locks require a regular file",
            ));
        };
        match (operation, nonblocking) {
            (1, false) => file.lock_shared(),
            (1, true) => Ok(file.try_lock_shared()?),
            (2, false) => file.lock(),
            (2, true) => Ok(file.try_lock()?),
            (3, _) => file.unlock(),
            _ => Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "invalid file lock operation",
            )),
        }
    }

    /// Lock a regular file for the duration of the owning stream. PHP's
    /// `LOCK_EX` flag rejects memory and temporary wrappers.
    #[cfg(feature = "file-write")]
    pub fn lock_exclusive(&self) -> io::Result<()> {
        match &self.backend {
            StreamBackend::File(file) => file.lock(),
            StreamBackend::Memory(_) | StreamBackend::Temp(_) | StreamBackend::Standard(_) => {
                Err(io::Error::new(
                    io::ErrorKind::Unsupported,
                    "exclusive locks require a regular file",
                ))
            }
        }
    }

    /// Truncate a regular file after acquiring `LOCK_EX`, avoiding the race
    /// caused by opening it in truncating mode before the lock is held.
    #[cfg(feature = "file-write")]
    pub fn truncate_file(&mut self) -> io::Result<()> {
        match &mut self.backend {
            StreamBackend::File(file) => {
                file.set_len(0)?;
                file.seek(SeekFrom::Start(0))?;
                self.eof = false;
                Ok(())
            }
            StreamBackend::Memory(_) | StreamBackend::Temp(_) | StreamBackend::Standard(_) => {
                Err(io::Error::new(
                    io::ErrorKind::Unsupported,
                    "truncate after locking requires a regular file",
                ))
            }
        }
    }

    pub fn seek(&mut self, position: SeekFrom) -> io::Result<u64> {
        let position = self.seek_backend(position)?;
        self.eof = false;
        #[cfg(feature = "stream-truncate")]
        {
            self.memory_append_after_truncate = false;
        }
        Ok(position)
    }

    pub fn position(&mut self) -> io::Result<u64> {
        match &mut self.backend {
            StreamBackend::File(file) => file.stream_position(),
            StreamBackend::Memory(memory) => Ok(memory.position()),
            StreamBackend::Temp(temp) => temp.position(),
            StreamBackend::Standard(_) => Err(io::Error::new(
                io::ErrorKind::Unsupported,
                "standard stream does not expose a position",
            )),
        }
    }

    pub fn metadata(&self) -> StreamMetadata<'_> {
        let (timed_out, blocked, eof, wrapper_type, stream_type) = match &self.backend {
            StreamBackend::File(_) => (
                Some(false),
                Some(true),
                Some(self.eof),
                "plainfile",
                "STDIO",
            ),
            StreamBackend::Memory(_) => (Some(false), Some(true), Some(self.eof), "PHP", "MEMORY"),
            StreamBackend::Temp(_) => (None, None, None, "PHP", "TEMP"),
            StreamBackend::Standard(_) => (Some(false), Some(true), Some(self.eof), "PHP", "STDIO"),
        };
        StreamMetadata {
            timed_out,
            blocked,
            eof,
            wrapper_type,
            stream_type,
            mode: &self.reported_mode,
            unread_bytes: 0,
            seekable: !matches!(self.backend, StreamBackend::Standard(_)),
            uri: &self.uri,
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

fn php_memory_mode(mode: StreamMode) -> &'static str {
    if mode.append {
        "a+b"
    } else if mode.read && !mode.write {
        "rb"
    } else {
        "w+b"
    }
}

#[cfg(test)]
#[path = "stream/tests.rs"]
mod tests;
