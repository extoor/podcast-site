//! Catalog model: the contents of `build/catalog.json`.

use serde::{Deserialize, Serialize};

/// Current `catalog.json` format version. Bump on incompatible changes.
pub const FORMAT_VERSION: u32 = 1;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Catalog {
    pub format_version: u32,
    /// Generation time, Unix seconds.
    pub generated_at: u64,
    /// Site base URL without a trailing `/`, e.g. `https://example.org`.
    pub base_url: String,
    /// Playlist with all episodes: `playlists/all.m3u8`.
    pub all_playlist: String,
    /// Podcasts (directories).
    pub podcasts: Vec<Podcast>,
    /// Programs sorted by name; the "no program" group comes last.
    pub programs: Vec<Program>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Program {
    /// ASCII identifier: `[a-z0-9-]+`, unique among programs.
    pub slug: String,
    /// Program name; `None` for episodes without a program.
    pub name: Option<String>,
    /// `playlists/program/<slug>.m3u8`.
    pub playlist: String,
    pub total_duration_secs: u64,
    /// Episode paths (`Episode::path`) in playlist order.
    pub episodes: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Podcast {
    /// ASCII identifier: `[a-z0-9-]+`, unique within the catalog.
    pub slug: String,
    /// Display name (directory name).
    pub title: String,
    /// Directory name relative to the media root.
    pub dir: String,
    /// Playlist path relative to the build directory: `playlists/dir/<slug>.m3u8`.
    pub playlist: String,
    /// Total duration of episodes with a known duration, seconds.
    pub total_duration_secs: u64,
    pub episodes: Vec<Episode>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Episode {
    /// Title: from the TIT2 tag, otherwise from the file name (see `podscan::filename`).
    pub title: String,
    /// Artist: from the TPE1 tag, otherwise (if there is no title tag) from the file name.
    pub artist: Option<String>,
    pub album: Option<String>,
    /// Program after applying the `programs.toml` rules (see `podscan::programs`).
    pub program: Option<String>,
    /// Duration from the TLEN tag; `None` if there is no tag.
    pub duration_secs: Option<u64>,
    /// File size in bytes.
    pub size: u64,
    /// Path relative to the media root (`/` as separator).
    pub path: String,
    /// Full URL for download/streaming.
    pub url: String,
    /// `true` if the title comes from a tag; `false` if title and artist
    /// were parsed from the file name.
    pub tagged: bool,
}
