use std::io::{self, SeekFrom};

use super::PhpStream;

impl PhpStream {
    /// Read at most `maximum` bytes from the current or requested absolute
    /// position. A fixed stack chunk bounds temporary memory while fallible
    /// incremental reservations avoid preallocating an attacker-sized length.
    #[cold]
    pub fn read_contents(
        &mut self,
        buffer: &mut Vec<u8>,
        maximum: Option<usize>,
        offset: Option<u64>,
    ) -> io::Result<usize> {
        if !self.is_readable() {
            return Err(io::Error::new(
                io::ErrorKind::PermissionDenied,
                "stream is not readable",
            ));
        }
        if let Some(offset) = offset {
            self.seek(SeekFrom::Start(offset))?;
        }

        buffer.clear();
        let maximum = maximum.unwrap_or(usize::MAX);
        let mut chunk = [0u8; 8 * 1024];
        while buffer.len() < maximum {
            let requested = chunk.len().min(maximum - buffer.len());
            buffer
                .try_reserve(requested)
                .map_err(|_| allocation_error())?;
            let read = self.read(&mut chunk[..requested])?;
            if read == 0 {
                break;
            }
            buffer.extend_from_slice(&chunk[..read]);
        }
        Ok(buffer.len())
    }
}

fn allocation_error() -> io::Error {
    io::Error::new(
        io::ErrorKind::OutOfMemory,
        "stream contents allocation failed",
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bounded_and_unbounded_reads_preserve_cursor_and_eof() {
        let mut stream = PhpStream::open("php://memory", "w+").unwrap();
        assert_eq!(stream.write(b"abcdef").unwrap(), 6);

        let mut contents = Vec::new();
        assert_eq!(
            stream
                .read_contents(&mut contents, Some(2), Some(2))
                .unwrap(),
            2
        );
        assert_eq!(contents, b"cd");
        assert_eq!(stream.position().unwrap(), 4);
        assert!(!stream.is_eof());

        assert_eq!(stream.read_contents(&mut contents, None, None).unwrap(), 2);
        assert_eq!(contents, b"ef");
        assert_eq!(stream.position().unwrap(), 6);
        assert!(stream.is_eof());

        assert_eq!(
            stream
                .read_contents(&mut contents, Some(0), Some(3))
                .unwrap(),
            0
        );
        assert!(contents.is_empty());
        assert_eq!(stream.position().unwrap(), 3);
        assert!(!stream.is_eof());
    }

    #[test]
    fn temporary_stream_reads_across_the_spill_boundary() {
        let mut stream = PhpStream::open("php://temp/maxmemory:4", "w+").unwrap();
        assert_eq!(stream.write(b"abcdefghij").unwrap(), 10);

        let mut contents = Vec::new();
        assert_eq!(
            stream
                .read_contents(&mut contents, Some(5), Some(3))
                .unwrap(),
            5
        );
        assert_eq!(contents, b"defgh");
        assert_eq!(stream.position().unwrap(), 8);
    }

    #[test]
    fn unbounded_read_crosses_multiple_fixed_chunks_without_truncation() {
        let payload: Vec<u8> = (0..20_000).map(|index| (index % 251) as u8).collect();
        let mut stream = PhpStream::open("php://memory", "w+").unwrap();
        assert_eq!(stream.write(&payload).unwrap(), payload.len());

        let mut contents = Vec::new();
        assert_eq!(
            stream.read_contents(&mut contents, None, Some(0)).unwrap(),
            payload.len()
        );
        assert_eq!(contents, payload);
        assert!(stream.is_eof());
    }
}
