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
