//! Building extended M3U8 (UTF-8).

use crate::catalog::Episode;
use std::fmt::Write;

/// The `.m3u` twin of a `.m3u8` playlist path (same text). Gopher links to it:
/// some clients and players on the other end know only the `.m3u` extension.
pub fn m3u_path(m3u8: &str) -> String {
    m3u8.strip_suffix(".m3u8")
        .map_or_else(|| m3u8.to_owned(), |base| format!("{base}.m3u"))
}

/// Returns the playlist text. All strings are already free of control characters
/// (see [`crate::text::clean`]), so they cannot contain line breaks.
pub fn render<'a>(title: &str, episodes: impl IntoIterator<Item = &'a Episode>) -> String {
    let mut out = String::from("#EXTM3U\n");
    // write! to a String cannot fail.
    let _ = writeln!(out, "#PLAYLIST:{title}");
    for ep in episodes {
        let dur = ep
            .duration_secs
            .map_or(-1, |d| i64::try_from(d).unwrap_or(-1));
        let name = match &ep.artist {
            Some(a) => format!("{a} - {}", ep.title),
            None => ep.title.clone(),
        };
        let _ = writeln!(out, "#EXTINF:{dur},{name}");
        let _ = writeln!(out, "{}", ep.url);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn m3u_twin() {
        assert_eq!(
            m3u_path("playlists/podcast/zebra.m3u8"),
            "playlists/podcast/zebra.m3u"
        );
        assert_eq!(m3u_path("playlists/all.m3u8"), "playlists/all.m3u");
        assert_eq!(m3u_path("x.m3u"), "x.m3u");
    }
}
