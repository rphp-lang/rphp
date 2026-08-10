//! Bounded stream reads terminated by an arbitrary byte sequence.

use std::io::{self, SeekFrom};

use super::PhpStream;

impl PhpStream {
    /// Read at most `maximum` bytes, stopping after `ending` when it is found.
    /// The ending is consumed but removed from `buffer`. `None` is returned
    /// only when EOF is reached before any byte is read.
    ///
    /// The KMP prefix table keeps matching linear for long or self-overlapping
    /// endings. Bytes read beyond the match are returned to the seekable
    /// backend so subsequent operations observe the exact PHP cursor.
    pub fn read_until(
        &mut self,
        buffer: &mut Vec<u8>,
        maximum: Option<usize>,
        ending: &[u8],
    ) -> io::Result<Option<usize>> {
        if !self.is_readable() {
            return Err(io::Error::new(
                io::ErrorKind::PermissionDenied,
                "stream is not readable",
            ));
        }
        buffer.clear();

        let prefix = prefix_table(ending)?;
        let maximum = maximum.unwrap_or(usize::MAX);
        let mut matched = 0;
        let mut chunk = [0u8; 8 * 1024];

        while buffer.len() < maximum {
            let requested = chunk.len().min(maximum - buffer.len());
            let read = self.read_backend(&mut chunk[..requested])?;
            if read == 0 {
                self.eof = true;
                return Ok((!buffer.is_empty()).then_some(buffer.len()));
            }
            self.eof = false;

            let ending_at = if ending.is_empty() {
                None
            } else {
                find_ending(&chunk[..read], ending, &prefix, &mut matched)
            };
            let consumed = ending_at.unwrap_or(read);
            if buffer.try_reserve(consumed).is_err() {
                rewind(self, read)?;
                return Err(io::Error::new(
                    io::ErrorKind::OutOfMemory,
                    "stream line buffer allocation failed",
                ));
            }
            buffer.extend_from_slice(&chunk[..consumed]);

            if consumed < read {
                rewind(self, read - consumed)?;
            }
            if ending_at.is_some() {
                buffer.truncate(buffer.len() - ending.len());
                return Ok(Some(buffer.len()));
            }
        }
        Ok(Some(buffer.len()))
    }
}

fn prefix_table(ending: &[u8]) -> io::Result<Vec<usize>> {
    let mut prefix = Vec::new();
    prefix.try_reserve_exact(ending.len()).map_err(|_| {
        io::Error::new(
            io::ErrorKind::OutOfMemory,
            "stream ending allocation failed",
        )
    })?;
    prefix.resize(ending.len(), 0);

    let mut matched = 0;
    for index in 1..ending.len() {
        while matched > 0 && ending[index] != ending[matched] {
            matched = prefix[matched - 1];
        }
        if ending[index] == ending[matched] {
            matched += 1;
        }
        prefix[index] = matched;
    }
    Ok(prefix)
}

fn find_ending(
    bytes: &[u8],
    ending: &[u8],
    prefix: &[usize],
    matched: &mut usize,
) -> Option<usize> {
    for (index, byte) in bytes.iter().enumerate() {
        while *matched > 0 && *byte != ending[*matched] {
            *matched = prefix[*matched - 1];
        }
        if *byte == ending[*matched] {
            *matched += 1;
        }
        if *matched == ending.len() {
            return Some(index + 1);
        }
    }
    None
}

fn rewind(stream: &mut PhpStream, bytes: usize) -> io::Result<()> {
    let bytes = i64::try_from(bytes).expect("stream line chunk fits in i64");
    stream.seek_backend(SeekFrom::Current(-bytes)).map(|_| ())
}
