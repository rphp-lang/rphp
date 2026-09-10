use super::{PhpStream, StreamMode, php_memory_stream_mode};
use std::io::SeekFrom;

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
