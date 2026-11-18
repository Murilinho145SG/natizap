//! Files copied in a file manager, as they reach the clipboard on Linux:
//! GPUI reads only the text a file manager offers alongside the files,
//! which is their locations.

use std::path::PathBuf;

/// The locations in `text`, when it is nothing but locations: absolute
/// paths or `file://` URIs, one per line, after GNOME's leading `copy` or
/// `cut` if present. Any other text gives `None`.
pub(crate) fn copied_paths(text: &str) -> Option<Vec<PathBuf>> {
    let mut lines = text
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .peekable();
    if lines
        .peek()
        .is_some_and(|first| matches!(*first, "copy" | "cut"))
    {
        lines.next();
    }
    let paths = lines.map(location).collect::<Option<Vec<_>>>()?;
    (!paths.is_empty()).then_some(paths)
}

fn location(line: &str) -> Option<PathBuf> {
    if let Some(rest) = line.strip_prefix("file://") {
        // `file:///home/…` has an empty host; `file://localhost/home/…`
        // names one.
        let path = &rest[rest.find('/')?..];
        return percent_decoded(path).map(PathBuf::from);
    }
    line.starts_with('/').then(|| PathBuf::from(line))
}

/// `text` with every `%XX` replaced by the byte it stands for.
fn percent_decoded(text: &str) -> Option<String> {
    let bytes = text.as_bytes();
    let mut decoded = Vec::with_capacity(bytes.len());
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] == b'%' {
            let hex = text.get(index + 1..index + 3)?;
            decoded.push(u8::from_str_radix(hex, 16).ok()?);
            index += 3;
        } else {
            decoded.push(bytes[index]);
            index += 1;
        }
    }
    String::from_utf8(decoded).ok()
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::copied_paths;

    #[test]
    fn reads_a_copied_path() {
        assert_eq!(
            copied_paths("/home/ana/Pictures/beach.jpg\n"),
            Some(vec![PathBuf::from("/home/ana/Pictures/beach.jpg")])
        );
    }

    #[test]
    fn reads_several_uris_and_decodes_them() {
        assert_eq!(
            copied_paths(
                "copy\nfile:///home/ana/Notas%20da%20aula.pdf\nfile:///tmp/a%C3%A7a%C3%AD.png"
            ),
            Some(vec![
                PathBuf::from("/home/ana/Notas da aula.pdf"),
                PathBuf::from("/tmp/açaí.png"),
            ])
        );
    }

    #[test]
    fn reads_a_uri_that_names_its_host() {
        assert_eq!(
            copied_paths("file://localhost/tmp/x.txt"),
            Some(vec![PathBuf::from("/tmp/x.txt")])
        );
    }

    #[test]
    fn leaves_ordinary_text_alone() {
        assert_eq!(copied_paths("see /home/ana/notes.txt"), None);
        assert_eq!(copied_paths("/tmp/a.txt\nand some words"), None);
        assert_eq!(copied_paths("   "), None);
        assert_eq!(copied_paths("copy"), None);
    }

    #[test]
    fn leaves_broken_escapes_alone() {
        assert_eq!(copied_paths("file:///tmp/100%"), None);
    }
}
