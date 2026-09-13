use super::{PhpStream, StreamMode, php_memory_stream_mode};
use std::io::SeekFrom;

#[test]
fn retained_readahead_never_exposes_stale_suffixes_after_seek_short_read_or_error() {
    let path = std::env::temp_dir().join(format!("rphp-retained-readahead-{}", std::process::id()));
    std::fs::write(&path, []).unwrap();
    let mut stream = PhpStream::open(path.to_str().unwrap(), "r+").unwrap();
    let mut allocation = None;
    for length in [10000, 65, 1, 0, 257, 2] {
        let payload: Vec<u8> = (0..length).map(|i| (i + 128) as u8).collect();
        std::fs::write(&path, &payload).unwrap();
        stream.seek(SeekFrom::Start(0)).unwrap();
        let mut first = [0xaa];
        let count = stream.read(&mut first).unwrap();
        assert_eq!(&first[..count], &payload[..count]);
        let mut tail = vec![0xaa; payload.len() + 1];
        let read = stream.read(&mut tail).unwrap();
        assert_eq!(&tail[..read], &payload[count..]);
        assert!(tail[read..].iter().all(|&byte| byte == 0xaa));
        assert!(stream.is_eof());
        assert_eq!(stream.unread_len(), 0);
        let buffer = stream.read_buffer.as_ref().unwrap();
        assert_eq!(buffer.bytes.len(), 8192);
        let pointer = buffer.bytes.as_ptr();
        if let Some(previous) = allocation {
            assert_eq!(pointer, previous);
        }
        allocation = Some(pointer);
    }
    // Both put-back shapes must publish only their explicit valid range.
    stream.put_back(b"abcd");
    let mut first = [0; 3];
    assert_eq!(stream.read_prefetched(&mut first).unwrap(), 3);
    assert_eq!(&first, b"abc");
    stream.put_back(b"XY");
    stream.put_back(b"12345");
    let mut restored = [0; 8];
    assert_eq!(stream.read_prefetched(&mut restored).unwrap(), 8);
    assert_eq!(&restored, b"12345XYd");
    let mut write_only = PhpStream::open(path.to_str().unwrap(), "w").unwrap();
    write_only.read_buffer = stream.read_buffer.take();
    write_only.discard_prefetched();
    assert!(write_only.read(&mut [0; 3]).is_err());
    assert_eq!(write_only.unread_len(), 0);
    assert_eq!(write_only.read_buffer.as_ref().unwrap().end, 0);
    drop(write_only);
    drop(stream);
    std::fs::remove_file(path).unwrap();
}

#[test]
fn memory_overwrite_matches_cursor_for_empty_bounds_growth_and_gaps() {
    use std::io::{Cursor, Write};
    for size in [0, 1, 63, 64, 65, 129] {
        let initial: Vec<u8> = (0..size).map(|i| i as u8).collect();
        for position in [0, 1, 63, 64, 65, 128, 129, 9999] {
            for count in [0, 1, 2, 63, 64, 65, 130] {
                let bytes: Vec<u8> = (0..count).map(|i| (i + 128) as u8).collect();
                let mut actual = Cursor::new(initial.clone());
                let mut expected = actual.clone();
                actual.set_position(position);
                expected.set_position(position);
                for _ in 0..2 {
                    assert_eq!(
                        PhpStream::write_memory(&mut actual, &bytes).unwrap(),
                        expected.write(&bytes).unwrap()
                    );
                    assert_eq!(actual.get_ref(), expected.get_ref());
                    assert_eq!(actual.position(), expected.position());
                }
            }
        }
    }
}

#[test]
fn owned_read_matches_slice_read_with_prefetch_empty_eof_and_far_cursors() {
    let payload: Vec<u8> = (0..9000).map(|index| index as u8).collect();
    for path in [
        "php://memory",
        "php://temp/maxmemory:99999",
        "php://temp/maxmemory:3",
    ] {
        for position in [0, 1, 63, 64, 65, 8191, 8999, 9000, 9999] {
            for length in [0, 1, 2, 63, 64, 65, 8191, 8192, 8193] {
                for prefetch in [false, true] {
                    let mut owned = PhpStream::open(path, "w+").unwrap();
                    let mut slice = PhpStream::open(path, "w+").unwrap();
                    for stream in [&mut owned, &mut slice] {
                        stream.write(&payload).unwrap();
                        stream.seek(SeekFrom::Start(position)).unwrap();
                        if prefetch {
                            stream.read_line(&mut Vec::new(), Some(2)).unwrap();
                        }
                    }
                    let mut actual = vec![0xaa; 5];
                    actual.reserve(length);
                    let mut expected = vec![0; length];
                    for _ in 0..2 {
                        let count = slice.read(&mut expected).unwrap();
                        assert_eq!(owned.read_into_vec(&mut actual, length).unwrap(), count);
                        assert_eq!(actual, expected[..count]);
                        assert_eq!(owned.position().unwrap(), slice.position().unwrap());
                        assert_eq!(owned.is_eof(), slice.is_eof());
                        assert_eq!(owned.metadata().unread_bytes, slice.metadata().unread_bytes);
                    }
                }
            }
        }
    }
}

#[test]
fn direct_line_projection_matches_scratch_cursor_and_retained_bytes() {
    let path = std::env::temp_dir().join(format!("rphp-line-projection-{}", std::process::id()));
    let payload: Vec<u8> = (0..8300)
        .map(|index| {
            if index % 71 == 0 {
                b'\n'
            } else {
                (index % 256) as u8
            }
        })
        .collect();
    for uri in [
        "php://memory",
        "php://temp/maxmemory:99999",
        "php://temp/maxmemory:3",
        path.to_str().unwrap(),
    ] {
        for position in [0, 1, 70, 8190, 8191, 8192, 8299, 8300, 9999] {
            for maximum in [1, 2, 3, 31, 63, 64] {
                let mut direct = PhpStream::open(uri, "w+").unwrap();
                let mut scratch = PhpStream::open(uri, "w+").unwrap();
                for stream in [&mut direct, &mut scratch] {
                    stream.write(&payload).unwrap();
                    stream.seek(SeekFrom::Start(position)).unwrap();
                    stream.take_plain_file_io();
                }
                for _ in 0..4 {
                    let mut actual = Vec::new();
                    let mut expected = Vec::new();
                    assert_eq!(
                        direct.read_line(&mut actual, Some(maximum + 1)).unwrap(),
                        scratch
                            .read_line_chunks(&mut expected, maximum, &mut [0; 64])
                            .unwrap()
                    );
                    assert_eq!(actual, expected);
                    assert_eq!(direct.position().unwrap(), scratch.position().unwrap());
                    assert_eq!(direct.is_eof(), scratch.is_eof());
                    assert_eq!(direct.unread_len(), scratch.unread_len());
                    assert_eq!(direct.take_plain_file_io(), scratch.take_plain_file_io());
                    let left = direct.read_buffer.as_ref().unwrap();
                    let right = scratch.read_buffer.as_ref().unwrap();
                    assert_eq!(
                        &left.bytes[left.start..left.end],
                        &right.bytes[right.start..right.end]
                    );
                }
            }
        }
    }
    std::fs::remove_file(path).unwrap();
}

#[test]
fn bounded_line_scratch_preserves_bytes_prefetch_and_eof_across_boundaries() {
    for path in [
        "php://memory",
        "php://temp/maxmemory:99999",
        "php://temp/maxmemory:3",
    ] {
        for newline in [0, 1, 62, 63, 64, 65, 8190, 8191, 8192, 8193] {
            let mut payload = vec![0x80; newline];
            payload.extend_from_slice(b"\n\0\xfftail");
            for maximum in [1, 2, 63, 64, 65, 8191, 8192, 8193] {
                let mut bounded = PhpStream::open(path, "w+").unwrap();
                let mut large = PhpStream::open(path, "w+").unwrap();
                for stream in [&mut bounded, &mut large] {
                    stream.write(&payload).unwrap();
                    stream.seek(SeekFrom::Start(0)).unwrap();
                }
                let mut actual = vec![0xaa; 5];
                let mut expected = Vec::new();
                let count = (newline + 1).min(maximum);
                assert_eq!(
                    bounded.read_line(&mut actual, Some(maximum + 1)).unwrap(),
                    Some(count)
                );
                assert_eq!(
                    large.read_line_large(&mut expected, maximum).unwrap(),
                    Some(count)
                );
                assert_eq!(actual, payload[..count]);
                assert_eq!(actual, expected);
                assert_eq!(bounded.position().unwrap(), count as u64);
                assert_eq!(bounded.is_eof(), large.is_eof());
                assert!(!bounded.is_eof());
                assert_eq!(
                    bounded.metadata().unread_bytes,
                    large.metadata().unread_bytes
                );
                let mut rest = vec![0; payload.len() + 1];
                assert_eq!(bounded.read(&mut rest).unwrap(), payload.len() - count);
                assert_eq!(&rest[..payload.len() - count], &payload[count..]);
                assert!(bounded.is_eof());
                assert_eq!(bounded.position().unwrap(), payload.len() as u64);
                assert!(bounded.rewind());
                assert_eq!(bounded.read_line(&mut actual, Some(1)).unwrap(), None);
                assert_eq!(bounded.position().unwrap(), 0);
            }
        }
    }
}

#[test]
fn boolean_rewind_matches_absolute_seek_for_prefetch_eof_and_failures() {
    for path in [
        "php://memory",
        "php://temp/maxmemory:99",
        "php://temp/maxmemory:3",
    ] {
        for stage in 0..3 {
            let mut direct = PhpStream::open(path, "w+").unwrap();
            let mut general = PhpStream::open(path, "w+").unwrap();
            for stream in [&mut direct, &mut general] {
                stream.write(b"a\nbc").unwrap();
                stream.seek(SeekFrom::Start(0)).unwrap();
                if stage > 0 {
                    stream.read_line(&mut Vec::new(), None).unwrap();
                }
                if stage > 1 {
                    stream.read(&mut [0; 32]).unwrap();
                    assert!(stream.is_eof());
                }
            }
            assert_eq!(direct.rewind(), general.seek(SeekFrom::Start(0)).is_ok());
            assert_eq!(direct.position().unwrap(), 0);
            assert_eq!(direct.is_eof(), general.is_eof());
            assert_eq!(direct.metadata().unread_bytes, 0);
            let mut actual = [0; 8];
            let mut expected = [0; 8];
            assert_eq!(
                direct.read(&mut actual).unwrap(),
                general.read(&mut expected).unwrap()
            );
            assert_eq!(actual, expected);
        }
    }
    for kind in [
        super::StandardStream::Input,
        super::StandardStream::Output,
        super::StandardStream::Error,
    ] {
        let mut stream = PhpStream::standard(kind);
        stream.eof = true;
        assert!(!stream.rewind());
        assert!(stream.is_eof());
        assert_eq!(stream.metadata().unread_bytes, 0);
    }
}

#[test]
#[cfg(feature = "stream-truncate")]
fn boolean_rewind_retires_memory_truncate_append_state() {
    let mut stream = PhpStream::open("php://memory", "w+").unwrap();
    stream.write(b"abcdef").unwrap();
    stream.truncate(2).unwrap();
    assert!(stream.rewind());
    stream.write(b"Z").unwrap();
    assert!(stream.rewind());
    let mut bytes = [0; 4];
    assert_eq!(stream.read(&mut bytes).unwrap(), 2);
    assert_eq!(&bytes[..2], b"Zb");
}

#[test]
fn finite_memory_read_preserves_eof_empty_and_prefetched_boundaries() {
    for length in [0, 1, 7, 31, 64, 255, 8193] {
        for extra in [0, 1, 9] {
            let mut stream = PhpStream::open("php://memory", "w+").unwrap();
            let payload: Vec<u8> = (0..length).map(|index| index as u8).collect();
            stream.write(&payload).unwrap();
            stream.seek(SeekFrom::Start(0)).unwrap();
            let mut output = vec![0xaa; length + extra];
            assert_eq!(stream.read(&mut output).unwrap(), length);
            assert_eq!(&output[..length], &payload);
            assert!(output[length..].iter().all(|byte| *byte == 0xaa));
            assert_eq!(stream.is_eof(), extra != 0);
            assert_eq!(stream.position().unwrap(), length as u64);
            assert_eq!(stream.read(&mut []).unwrap(), 0);
            assert_eq!(stream.is_eof(), extra != 0);
            assert_eq!(stream.read(&mut [0]).unwrap(), 0);
            assert!(stream.is_eof());
            assert_eq!(stream.read(&mut []).unwrap(), 0);
            assert!(stream.is_eof());
            stream.seek(SeekFrom::Start(length as u64 + 3)).unwrap();
            assert_eq!(stream.read(&mut [0]).unwrap(), 0);
            assert_eq!(stream.position().unwrap(), length as u64 + 3);
            assert!(stream.is_eof());
        }
    }
    let mut stream = PhpStream::open("php://memory", "w+").unwrap();
    stream.write(b"a\nbc").unwrap();
    stream.seek(SeekFrom::Start(0)).unwrap();
    let mut line = Vec::new();
    stream.read_line(&mut line, None).unwrap();
    assert_eq!(line, b"a\n");
    assert_eq!(stream.metadata().unread_bytes, 2);
    let mut rest = [0; 2];
    assert_eq!(stream.read(&mut rest).unwrap(), 2);
    assert_eq!(&rest, b"bc");
    assert!(!stream.is_eof());
    assert_eq!(stream.read(&mut []).unwrap(), 0);
    assert!(!stream.is_eof());
    assert_eq!(stream.read(&mut [0]).unwrap(), 0);
    assert!(stream.is_eof());
}

#[test]
fn native_prefetch_keeps_logical_seek_write_and_failed_seek_state() {
    let mut stream = PhpStream::open("php://memory", "w+").unwrap();
    stream.write(b"a\nbcdef").unwrap();
    stream.seek(SeekFrom::Start(0)).unwrap();
    let mut line = Vec::new();
    stream.read_line(&mut line, None).unwrap();
    assert_eq!(line, b"a\n");
    assert_eq!(stream.metadata().unread_bytes, 5);
    assert_eq!(stream.position().unwrap(), 2);
    assert!(stream.seek(SeekFrom::Current(-3)).is_err());
    assert_eq!(stream.metadata().unread_bytes, 5);
    assert_eq!(stream.position().unwrap(), 2);
    stream.seek(SeekFrom::Current(1)).unwrap();
    assert_eq!(stream.position().unwrap(), 3);
    stream.write(b"XY").unwrap();
    let mut bytes = [0; 8];
    assert_eq!(stream.read(&mut bytes).unwrap(), 2);
    assert_eq!(&bytes[..2], b"ef");
    stream.seek(SeekFrom::Start(0)).unwrap();
    assert_eq!(stream.read(&mut bytes).unwrap(), 7);
    assert_eq!(&bytes[..7], b"a\nbXYef");
    assert_eq!(stream.metadata().unread_bytes, 0);
}

#[test]
fn line_prefetch_crosses_chunks_without_reordering_or_rereading() {
    let payload = [vec![b'x'; 8200], b"\ny\nlast".to_vec()].concat();
    let mut stream = PhpStream::open("php://memory", "w+").unwrap();
    stream.write(&payload).unwrap();
    stream.seek(SeekFrom::Start(0)).unwrap();
    let mut line = Vec::new();
    assert_eq!(stream.read_line(&mut line, Some(3)).unwrap(), Some(2));
    assert_eq!(stream.metadata().unread_bytes, 8190);
    assert_eq!(stream.read_line(&mut line, None).unwrap(), Some(8199));
    assert_eq!(&line, &payload[2..8201]);
    assert_eq!(stream.position().unwrap(), 8201);
    assert_eq!(stream.read_line(&mut line, None).unwrap(), Some(2));
    assert_eq!(line, b"y\n");
    assert_eq!(stream.read_line(&mut line, None).unwrap(), Some(4));
    assert_eq!(line, b"last");
}

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
fn memory_wrappers_keep_their_permissive_legacy_mode_grammar() {
    assert_eq!(
        php_memory_stream_mode("+r"),
        StreamMode {
            read: true,
            write: true,
            append: false,
            create: true,
            truncate: true,
            exclusive: false,
        }
    );
    assert!(PhpStream::open("php://memory", "+r").is_ok());
    assert!(PhpStream::open("php://temp", "not-a-file-mode").is_ok());
    assert!(PhpStream::open("/rphp/does-not-exist", "+r").is_err());
    for spelling in ["r+", "w", "rw", "a", "za", "xw"] {
        let mode = php_memory_stream_mode(spelling);
        assert!(mode.read && mode.write, "{spelling}");
        assert_eq!(mode.append, spelling.contains('a'));
    }
    assert!(!php_memory_stream_mode("r\0w").write);
    assert!(php_memory_stream_mode("x").read);
    assert!(!php_memory_stream_mode("x").write);
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
#[cfg(feature = "file-write")]
fn regular_file_lock_precedes_truncation_and_blocks_competing_locks() {
    let path = std::env::temp_dir().join(format!(
        "rphp-stream-lock-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::write(&path, b"before").unwrap();
    let mut stream = PhpStream::open(path.to_str().unwrap(), "c").unwrap();
    stream.lock_exclusive().unwrap();

    let competitor = std::fs::OpenOptions::new().write(true).open(&path).unwrap();
    assert!(matches!(
        competitor.try_lock(),
        Err(std::fs::TryLockError::WouldBlock)
    ));

    stream.truncate_file().unwrap();
    stream.write(b"after").unwrap();
    drop(stream);
    competitor.try_lock().unwrap();
    assert_eq!(std::fs::read(&path).unwrap(), b"after");
    std::fs::remove_file(path).unwrap();
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

#[test]
fn line_reads_preserve_newline_length_position_and_eof() {
    let mut stream = PhpStream::open("php://memory", "w+").unwrap();
    stream.write(b"a\nbc\nlast").unwrap();
    stream.seek(SeekFrom::Start(0)).unwrap();
    let mut line = Vec::new();

    assert_eq!(stream.read_line(&mut line, None).unwrap(), Some(2));
    assert_eq!(line, b"a\n");
    assert_eq!(stream.position().unwrap(), 2);
    assert!(!stream.is_eof());

    assert_eq!(stream.read_line(&mut line, Some(4)).unwrap(), Some(3));
    assert_eq!(line, b"bc\n");
    assert_eq!(stream.read_line(&mut line, Some(3)).unwrap(), Some(2));
    assert_eq!(line, b"la");
    assert!(!stream.is_eof());

    assert_eq!(stream.read_line(&mut line, None).unwrap(), Some(2));
    assert_eq!(line, b"st");
    assert!(stream.is_eof());
    assert_eq!(stream.read_line(&mut line, None).unwrap(), None);
    assert!(stream.is_eof());
}

#[test]
fn metadata_identifies_each_backend_without_platform_helpers() {
    let memory = PhpStream::open("php://memory", "w+").unwrap();
    let metadata = memory.metadata();
    assert_eq!(metadata.wrapper_type, "PHP");
    assert_eq!(metadata.stream_type, "MEMORY");
    assert_eq!(metadata.mode, "w+b");
    assert_eq!(metadata.eof, Some(false));
    assert_eq!(metadata.uri, "php://memory");

    let temporary = PhpStream::open("php://temp/maxmemory:4", "a+").unwrap();
    let metadata = temporary.metadata();
    assert_eq!(metadata.stream_type, "TEMP");
    assert_eq!(metadata.mode, "a+b");
    assert_eq!(metadata.eof, None);
}

#[test]
fn canonical_stream_metadata_borrows_only_static_backend_strings() {
    use super::{Cow, StandardStream};

    assert_eq!(
        std::mem::size_of::<Cow<'static, str>>(),
        std::mem::size_of::<String>()
    );
    for (requested, reported) in [
        ("r", "rb"),
        ("w", "w+b"),
        ("a+", "a+b"),
        ("x", "rb"),
        ("r\0w", "rb"),
    ] {
        let mut path = String::from("php://memory");
        let mut mode = requested.to_string();
        let stream = PhpStream::open(&path, &mode).unwrap();
        path.clear();
        mode.clear();
        assert!(matches!(stream.uri, Cow::Borrowed("php://memory")));
        assert!(matches!(stream.reported_mode, Cow::Borrowed(_)));
        assert_eq!(stream.metadata().uri, "php://memory");
        assert_eq!(stream.metadata().mode, reported);
    }
    for (kind, uri, mode) in [
        (StandardStream::Input, "php://stdin", "rb"),
        (StandardStream::Output, "php://stdout", "wb"),
        (StandardStream::Error, "php://stderr", "wb"),
    ] {
        let stream = PhpStream::standard(kind);
        assert!(matches!(stream.uri, Cow::Borrowed(_)));
        assert!(matches!(stream.reported_mode, Cow::Borrowed(_)));
        assert_eq!((stream.metadata().uri, stream.metadata().mode), (uri, mode));
    }
}

#[test]
fn dynamic_stream_metadata_keeps_its_own_input_snapshot() {
    use super::Cow;

    let mut path = String::from("php://temp/maxmemory:3");
    let mut mode = String::from("a+");
    let mut temporary = PhpStream::open(&path, &mode).unwrap();
    path.clear();
    mode.clear();
    assert!(matches!(temporary.uri, Cow::Owned(_)));
    assert!(matches!(temporary.reported_mode, Cow::Borrowed(_)));
    temporary.write(b"spill and retain metadata").unwrap();
    assert_eq!(temporary.metadata().uri, "php://temp/maxmemory:3");
    assert_eq!(temporary.metadata().mode, "a+b");

    let mut uri = String::from("data:text/plain,example");
    let mut requested_mode = String::from("wb\0ignored");
    let decoded = PhpStream::decoded_input(b"example".to_vec(), &uri, &requested_mode);
    uri.clear();
    requested_mode.clear();
    assert!(matches!(decoded.uri, Cow::Owned(_)));
    assert!(matches!(decoded.reported_mode, Cow::Owned(_)));
    assert_eq!(decoded.metadata().uri, "data:text/plain,example");
    assert_eq!(decoded.metadata().mode, "wb");
    assert!(decoded.is_readable());
    assert!(!decoded.is_writable());
}

#[test]
fn line_reads_cross_stack_chunks_without_hiding_cursor_bytes() {
    let mut contents = vec![b'x'; 9_000];
    contents.push(b'\n');
    contents.extend_from_slice(b"tail");
    let mut stream = PhpStream::open("php://memory", "w+").unwrap();
    stream.write(&contents).unwrap();
    stream.seek(SeekFrom::Start(0)).unwrap();
    let mut line = Vec::new();

    assert_eq!(stream.read_line(&mut line, None).unwrap(), Some(9_001));
    assert_eq!(line.len(), 9_001);
    assert_eq!(line.last(), Some(&b'\n'));
    assert_eq!(stream.position().unwrap(), 9_001);
    assert!(!stream.is_eof());

    assert_eq!(stream.read_line(&mut line, None).unwrap(), Some(4));
    assert_eq!(line, b"tail");
    assert!(stream.is_eof());
}

#[test]
#[cfg(feature = "stream-line")]
fn arbitrary_line_endings_preserve_limits_cursor_and_eof() {
    let mut stream = PhpStream::open("php://memory", "w+").unwrap();
    stream.write(b"ab--cd--ef").unwrap();
    stream.seek(SeekFrom::Start(0)).unwrap();
    let mut line = Vec::new();

    assert_eq!(stream.read_until(&mut line, None, b"--").unwrap(), Some(2));
    assert_eq!(line, b"ab");
    assert_eq!(stream.position().unwrap(), 4);
    assert!(!stream.is_eof());

    assert_eq!(
        stream.read_until(&mut line, Some(4), b"--").unwrap(),
        Some(2)
    );
    assert_eq!(line, b"cd");
    assert_eq!(stream.position().unwrap(), 8);

    assert_eq!(
        stream.read_until(&mut line, Some(99), b"--").unwrap(),
        Some(2)
    );
    assert_eq!(line, b"ef");
    assert!(stream.is_eof());
    assert_eq!(stream.read_until(&mut line, None, b"--").unwrap(), None);
}

#[test]
#[cfg(feature = "stream-line")]
fn arbitrary_line_endings_match_across_chunks_and_overlap() {
    let mut contents = vec![b'x'; 8_191];
    contents.extend_from_slice(b"abababaca-tail");
    let mut stream = PhpStream::open("php://memory", "w+").unwrap();
    stream.write(&contents).unwrap();
    stream.seek(SeekFrom::Start(0)).unwrap();
    let mut line = Vec::new();

    assert_eq!(
        stream.read_until(&mut line, None, b"ababaca").unwrap(),
        Some(8_193)
    );
    let mut expected = vec![b'x'; 8_191];
    expected.extend_from_slice(b"ab");
    assert_eq!(line, expected);
    assert_eq!(stream.position().unwrap(), 8_200);
    assert_eq!(
        stream.read_until(&mut line, None, b"ababaca").unwrap(),
        Some(5)
    );
    assert_eq!(line, b"-tail");
    assert!(stream.is_eof());
}

#[test]
#[cfg(feature = "stream-truncate")]
fn truncation_resizes_memory_and_temp_without_moving_cursor_or_eof() {
    for path in ["php://memory", "php://temp/maxmemory:99"] {
        let mut stream = PhpStream::open(path, "w+").unwrap();
        stream.write(b"abcdef").unwrap();
        stream.seek(SeekFrom::Start(2)).unwrap();
        stream.truncate(4).unwrap();
        assert_eq!(stream.position().unwrap(), 2);
        assert!(!stream.is_eof());

        stream.seek(SeekFrom::Start(0)).unwrap();
        let mut bytes = [0u8; 8];
        assert_eq!(stream.read(&mut bytes).unwrap(), 4);
        assert_eq!(&bytes[..4], b"abcd");
        assert_eq!(stream.read(&mut bytes).unwrap(), 0);
        assert!(stream.is_eof());

        stream.truncate(8).unwrap();
        assert_eq!(stream.position().unwrap(), 4);
        assert!(stream.is_eof());
        stream.seek(SeekFrom::Start(0)).unwrap();
        assert_eq!(stream.read(&mut bytes).unwrap(), 8);
        assert_eq!(&bytes, b"abcd\0\0\0\0");
    }
}

#[test]
#[cfg(feature = "stream-truncate")]
fn truncation_preserves_php_memory_append_and_spilled_file_gap_rules() {
    for path in ["php://memory", "php://temp/maxmemory:99"] {
        let mut stream = PhpStream::open(path, "w+").unwrap();
        stream.write(b"abcdef").unwrap();
        stream.truncate(2).unwrap();
        assert_eq!(stream.position().unwrap(), 6);
        stream.write(b"Z").unwrap();
        assert_eq!(stream.position().unwrap(), 7);
        stream.seek(SeekFrom::Start(0)).unwrap();
        let mut bytes = [0u8; 7];
        assert_eq!(stream.read(&mut bytes).unwrap(), 3);
        assert_eq!(&bytes[..3], b"abZ");
    }

    let mut spilled = PhpStream::open("php://temp/maxmemory:2", "w+").unwrap();
    spilled.write(b"abcdef").unwrap();
    assert!(spilled.temp_spill_path().is_some());
    spilled.truncate(2).unwrap();
    spilled.write(b"Z").unwrap();
    spilled.seek(SeekFrom::Start(0)).unwrap();
    let mut bytes = [0u8; 7];
    assert_eq!(spilled.read(&mut bytes).unwrap(), 7);
    assert_eq!(&bytes, b"ab\0\0\0\0Z");

    let mut grown = PhpStream::open("php://temp/maxmemory:4", "w+").unwrap();
    grown.truncate(8).unwrap();
    assert!(grown.temp_spill_path().is_some());
}

#[test]
fn csv_length_boundary_and_open_enclosure_follow_php_cursor_rules() {
    let mut stream = PhpStream::open("php://memory", "w+").unwrap();
    stream.write(b"\"abcdef\",x\nnext,row\n").unwrap();
    stream.seek(SeekFrom::Start(0)).unwrap();

    let record = stream
        .read_csv_record(Some(8), b',', b'"', Some(b'\\'))
        .unwrap()
        .unwrap();
    assert_eq!(record, vec![Some(b"abcdef".to_vec())]);
    assert_eq!(stream.position().unwrap(), 8);

    let record = stream
        .read_csv_record(None, b',', b'"', Some(b'\\'))
        .unwrap()
        .unwrap();
    assert_eq!(record, vec![Some(Vec::new()), Some(b"x".to_vec())]);
    assert_eq!(stream.position().unwrap(), 11);

    stream.seek(SeekFrom::Start(0)).unwrap();
    let record = stream
        .read_csv_record(Some(4), b',', b'"', Some(b'\\'))
        .unwrap()
        .unwrap();
    assert_eq!(record, vec![Some(b"abcdef".to_vec()), Some(b"x".to_vec())]);
    assert_eq!(stream.position().unwrap(), 11);
}

#[test]
fn csv_records_preserve_quoted_newlines_and_blank_line_identity() {
    let mut stream = PhpStream::open("php://temp/maxmemory:4", "w+").unwrap();
    stream.write(b"a,\"two\nlines\",z\r\n\r\n").unwrap();
    stream.seek(SeekFrom::Start(0)).unwrap();

    let record = stream
        .read_csv_record(None, b',', b'"', None)
        .unwrap()
        .unwrap();
    assert_eq!(
        record,
        vec![
            Some(b"a".to_vec()),
            Some(b"two\nlines".to_vec()),
            Some(b"z".to_vec())
        ]
    );
    assert_eq!(
        stream
            .read_csv_record(None, b',', b'"', None)
            .unwrap()
            .unwrap(),
        vec![None]
    );
    assert!(
        stream
            .read_csv_record(None, b',', b'"', None)
            .unwrap()
            .is_none()
    );
    assert!(stream.is_eof());
}
