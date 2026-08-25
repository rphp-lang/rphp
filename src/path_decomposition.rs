pub const PATHINFO_DIRNAME: i64 = 1;
pub const PATHINFO_BASENAME: i64 = 2;
pub const PATHINFO_EXTENSION: i64 = 4;
pub const PATHINFO_FILENAME: i64 = 8;
pub const PATHINFO_ALL: i64 = 15;

pub struct PathInfo {
    pub dirname: Vec<u8>,
    pub basename: Vec<u8>,
    pub extension: Option<Vec<u8>>,
    pub filename: Vec<u8>,
}

pub fn basename(path: &[u8], suffix: &[u8]) -> Vec<u8> {
    let basename = basename_without_suffix(path);
    if !suffix.is_empty() && suffix.len() < basename.len() && basename.ends_with(suffix) {
        basename[..basename.len() - suffix.len()].to_vec()
    } else {
        basename.to_vec()
    }
}

fn basename_without_suffix(path: &[u8]) -> &[u8] {
    let mut end = path.len();
    while end > 0 && path[end - 1] == b'/' {
        end -= 1;
    }
    let start = path[..end]
        .iter()
        .rposition(|byte| *byte == b'/')
        .map_or(0, |separator| separator + 1);
    &path[start..end]
}

pub fn dirname(path: &[u8], mut levels: u64) -> Vec<u8> {
    if path.is_empty() {
        return Vec::new();
    }

    let absolute = path[0] == b'/';
    let mut end = path.len();
    while levels > 0 {
        while end > 0 && path[end - 1] == b'/' {
            end -= 1;
        }
        if end == 0 {
            return if absolute { vec![b'/'] } else { Vec::new() };
        }

        while end > 0 && path[end - 1] != b'/' {
            end -= 1;
        }
        while end > 0 && path[end - 1] == b'/' {
            end -= 1;
        }
        if end == 0 {
            return if absolute { vec![b'/'] } else { vec![b'.'] };
        }
        levels -= 1;
    }
    path[..end].to_vec()
}

pub fn pathinfo(path: &[u8]) -> PathInfo {
    let mut dirname = dirname(path, 1);
    if let Some(nul) = dirname.iter().position(|byte| *byte == 0) {
        dirname.truncate(nul);
    }
    let basename = basename_without_suffix(path).to_vec();
    let (extension, filename) = match basename.iter().rposition(|byte| *byte == b'.') {
        Some(dot) => (Some(basename[dot + 1..].to_vec()), basename[..dot].to_vec()),
        None => (None, basename.clone()),
    };
    PathInfo {
        dirname,
        basename,
        extension,
        filename,
    }
}

#[cfg(test)]
mod tests {
    use super::{basename, dirname, pathinfo};

    #[test]
    fn basename_ignores_trailing_separators_and_removes_only_shorter_suffixes() {
        assert_eq!(basename(b"///top//leaf.ext///", b".ext"), b"leaf");
        assert_eq!(basename(b"leaf.ext", b"leaf.ext"), b"leaf.ext");
        assert_eq!(basename(b"/", b""), b"");
        assert_eq!(basename(b"top/na\0me.\xff", b".\xff"), b"na\0me");
    }

    #[test]
    fn dirname_preserves_retained_separators_and_saturates_at_root_or_dot() {
        assert_eq!(dirname(b"a//b///c/", 1), b"a//b");
        assert_eq!(dirname(b"///a//b///c/", 2), b"///a");
        assert_eq!(dirname(b"///a//b///c/", u64::MAX), b"/");
        assert_eq!(dirname(b"a//b///c/", u64::MAX), b".");
        assert_eq!(dirname(b"", u64::MAX), b"");
    }

    #[test]
    fn pathinfo_tracks_extension_presence_separately_from_empty_extension() {
        let plain = pathinfo(b"leaf");
        assert_eq!(plain.dirname, b".");
        assert_eq!(plain.basename, b"leaf");
        assert_eq!(plain.extension, None);
        assert_eq!(plain.filename, b"leaf");

        let dotted = pathinfo(b"/top/.hidden.");
        assert_eq!(dotted.dirname, b"/top");
        assert_eq!(dotted.basename, b".hidden.");
        assert_eq!(dotted.extension, Some(Vec::new()));
        assert_eq!(dotted.filename, b".hidden");

        let nul_dirname = pathinfo(b"A/\0/B.ext");
        assert_eq!(nul_dirname.dirname, b"A/");
        assert_eq!(dirname(b"A/\0/B.ext", 1), b"A/\0");
    }
}
