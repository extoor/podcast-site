//! Building extended M3U8 (UTF-8).

use crate::catalog::Episode;
use std::fmt::Write;

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
