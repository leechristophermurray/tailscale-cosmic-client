//! Parsing `text/uri-list`, the payload a Wayland drag-and-drop delivers.
//!
//! Wayland offers dropped files as a MIME payload rather than as paths, so the
//! bytes have to be decoded before anything can be sent. The format is RFC 2483:
//! CRLF-separated URIs, with `#` comment lines.

use std::path::PathBuf;

/// Extract local file paths from a `text/uri-list` payload.
///
/// Non-`file://` URIs are skipped: a URL dragged from a browser is a perfectly
/// normal thing to drop, and there is nothing to send for it.
#[must_use]
pub fn parse(bytes: &[u8]) -> Vec<PathBuf> {
    let text = String::from_utf8_lossy(bytes);

    text.lines()
        .map(str::trim)
        .filter(|line| !line.is_empty() && !line.starts_with('#'))
        .filter_map(path_from_uri)
        .collect()
}

/// Turn one `file://` URI into a path, undoing percent-encoding.
fn path_from_uri(uri: &str) -> Option<PathBuf> {
    // Some senders drop a bare path rather than a URI. Accept it.
    let encoded = if let Some(rest) = uri.strip_prefix("file://") {
        // Strip the authority, which is empty for local files: `file:///tmp/x`.
        // A non-empty one means a remote host, which is not ours to read.
        match rest.find('/') {
            Some(0) => rest,
            _ => return None,
        }
    } else if uri.starts_with('/') {
        uri
    } else {
        return None;
    };

    Some(PathBuf::from(percent_decode(encoded)))
}

/// Decode `%XX` escapes. A filename with a space arrives as `%20`, and opening
/// a literal `%20` file fails in a way that is hard to diagnose.
fn percent_decode(value: &str) -> String {
    let bytes = value.as_bytes();
    let mut out: Vec<u8> = Vec::with_capacity(bytes.len());
    let mut index = 0;

    while index < bytes.len() {
        if bytes[index] == b'%' && index + 2 < bytes.len() {
            let hex = std::str::from_utf8(&bytes[index + 1..index + 3]).ok();
            if let Some(byte) = hex.and_then(|hex| u8::from_str_radix(hex, 16).ok()) {
                out.push(byte);
                index += 3;
                continue;
            }
        }
        out.push(bytes[index]);
        index += 1;
    }

    String::from_utf8_lossy(&out).into_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_a_typical_drop() {
        let payload = "file:///home/alex/notes.md\r\nfile:///home/alex/photo.png\r\n";
        assert_eq!(
            parse(payload.as_bytes()),
            vec![
                PathBuf::from("/home/alex/notes.md"),
                PathBuf::from("/home/alex/photo.png"),
            ]
        );
    }

    /// Spaces and other characters arrive percent-encoded; opening the literal
    /// `%20` form fails confusingly.
    #[test]
    fn decodes_percent_escapes() {
        let payload = "file:///home/alex/Q3%20report%20%282024%29.pdf\r\n";
        assert_eq!(
            parse(payload.as_bytes()),
            vec![PathBuf::from("/home/alex/Q3 report (2024).pdf")]
        );
    }

    /// Non-ASCII filenames are percent-encoded UTF-8 byte by byte.
    #[test]
    fn decodes_multibyte_filenames() {
        let payload = "file:///home/alex/r%C3%A9sum%C3%A9.pdf\r\n";
        assert_eq!(
            parse(payload.as_bytes()),
            vec![PathBuf::from("/home/alex/résumé.pdf")]
        );
    }

    #[test]
    fn skips_comments_and_blank_lines() {
        let payload = "# a comment\r\n\r\nfile:///tmp/a.txt\r\n";
        assert_eq!(parse(payload.as_bytes()), vec![PathBuf::from("/tmp/a.txt")]);
    }

    /// Dragging a link from a browser is normal; there is nothing to send.
    #[test]
    fn ignores_non_file_uris() {
        let payload = "https://example.com/page\r\nfile:///tmp/real.txt\r\n";
        assert_eq!(parse(payload.as_bytes()), vec![PathBuf::from("/tmp/real.txt")]);
    }

    /// A `file://host/path` URI points at another machine's filesystem.
    #[test]
    fn ignores_remote_file_uris() {
        assert!(parse(b"file://otherhost/srv/data.bin\r\n").is_empty());
    }

    /// Some senders drop a bare path instead of a URI.
    #[test]
    fn accepts_a_bare_path() {
        assert_eq!(parse(b"/tmp/plain.txt\n"), vec![PathBuf::from("/tmp/plain.txt")]);
    }

    #[test]
    fn an_empty_payload_yields_nothing() {
        assert!(parse(b"").is_empty());
        assert!(parse(b"\r\n\r\n").is_empty());
    }
}
