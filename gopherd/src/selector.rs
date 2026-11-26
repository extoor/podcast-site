//! Request line parsing and selector validation. Pure functions, no I/O.
//!
//! A selector is accepted only if it is a clean relative path under one of the
//! roots; everything else is rejected before the file system is touched:
//! NUL and other control bytes, invalid UTF-8, backslashes, empty, `.` and `..`
//! segments, and segments starting with a dot (hidden files).

use std::path::PathBuf;

/// Longest selector, bytes (RFC 1436).
pub const MAX_SELECTOR: usize = 255;

/// Which directory a selector points into.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Root {
    /// Generated menus: `build/gopher` (a directory means its `gophermap`).
    Menus,
    /// `/playlists/…` → `build/playlists`.
    Playlists,
    /// `/media/…` → the audio library.
    Media,
}

/// A validated request: a root and a relative path made of plain segments.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Route {
    pub root: Root,
    pub rel: PathBuf,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Reject {
    /// No line end within the limit.
    TooLong,
    /// Bytes or path segments that are never valid.
    Invalid,
}

/// Extracts the selector from a request line (without its line end). Anything after
/// a tab (Gopher+ markers, search queries) is ignored.
pub fn selector(line: &[u8]) -> Result<&[u8], Reject> {
    let sel = line.split(|&b| b == b'\t').next().unwrap_or_default();
    if sel.len() > MAX_SELECTOR {
        return Err(Reject::TooLong);
    }
    Ok(sel)
}

/// Validates a selector and maps it to a route.
pub fn route(sel: &[u8]) -> Result<Route, Reject> {
    if sel.iter().any(|&b| b < 0x20 || b == 0x7F || b == b'\\') {
        return Err(Reject::Invalid);
    }
    let sel = std::str::from_utf8(sel).map_err(|_| Reject::Invalid)?;
    // Unicode control characters too (C1 range etc.).
    if sel.chars().any(char::is_control) {
        return Err(Reject::Invalid);
    }
    // "" and "/" are the root menu; otherwise the selector must be absolute.
    let path = match sel {
        "" | "/" => "",
        s => s.strip_prefix('/').ok_or(Reject::Invalid)?,
    };
    // A trailing slash marks a directory (menu); it adds no segment.
    let path = path.strip_suffix('/').unwrap_or(path);
    let mut segments: Vec<&str> = if path.is_empty() {
        Vec::new()
    } else {
        path.split('/').collect()
    };
    if segments.iter().any(|s| s.is_empty() || s.starts_with('.')) {
        return Err(Reject::Invalid);
    }
    let root = match segments.first().copied() {
        Some("media") => Root::Media,
        Some("playlists") => Root::Playlists,
        _ => Root::Menus,
    };
    if root != Root::Menus {
        segments.remove(0);
        // `/media/` alone would be the library root: not something to serve.
        if segments.is_empty() {
            return Err(Reject::Invalid);
        }
    }
    Ok(Route {
        root,
        rel: segments.iter().collect(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ok(sel: &str) -> (Root, String) {
        let r = route(sel.as_bytes()).unwrap();
        (r.root, r.rel.to_string_lossy().into_owned())
    }

    #[test]
    fn valid() {
        assert_eq!(ok(""), (Root::Menus, String::new()));
        assert_eq!(ok("/"), (Root::Menus, String::new()));
        assert_eq!(ok("/podcast/zebra/"), (Root::Menus, "podcast/zebra".into()));
        assert_eq!(ok("/podcast/zebra"), (Root::Menus, "podcast/zebra".into()));
        assert_eq!(
            ok("/playlists/all.m3u8"),
            (Root::Playlists, "all.m3u8".into())
        );
        assert_eq!(
            ok("/media/KissFM/a b-ø.mp3"),
            (Root::Media, "KissFM/a b-ø.mp3".into())
        );
    }

    #[test]
    fn traversal_and_junk() {
        for bad in [
            "..",
            "/..",
            "/../etc/passwd",
            "/media/../../etc/passwd",
            "/media/KissFM/../x",
            "/./x",
            "/media/.hidden",
            "//etc/passwd",
            "/media//x",
            "etc/passwd",
            "/media",
            "/media/",
            "/playlists/",
            "/a\\..\\b",
            "/a\0b",
            "/a\x1bb",
            "/a\x7fb",
            "/a\u{85}b",
        ] {
            assert_eq!(route(bad.as_bytes()), Err(Reject::Invalid), "{bad:?}");
        }
        assert_eq!(route(b"/\xff\xfe"), Err(Reject::Invalid));
    }

    #[test]
    fn request_line() {
        assert_eq!(selector(b"/x\t+"), Ok(&b"/x"[..]));
        assert_eq!(selector(b"/x\tquery\t$"), Ok(&b"/x"[..]));
        assert_eq!(selector(&[b'a'; 255]), Ok(&[b'a'; 255][..]));
        assert_eq!(selector(&[b'a'; 256]), Err(Reject::TooLong));
    }
}
