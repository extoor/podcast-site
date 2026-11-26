//! Walking the media library and reading ID3 tags.
//!
//! Files are not read in full: `id3` reads only the tag header at the start
//! of the file (over rclone that is a few kilobytes, not the whole file).

use crate::catalog::{Episode, Podcast};
use crate::filename;
use crate::programs::{Programs, normalize};
use crate::text::{clean, encode_path, slugify, unique_slug};
use anyhow::{Context, Result};
use id3::TagLike;
use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::Path;

/// Supported audio file extensions (lowercase).
const AUDIO_EXT: &[&str] = &["mp3"];

/// Scans `root`: every top-level directory is a podcast.
/// Hidden entries and symlinks are skipped. Podcasts without audio are left out.
pub fn scan(root: &Path, base_url: &str, programs: &Programs) -> Result<Vec<Podcast>> {
    let mut dirs = Vec::new();
    for entry in
        fs::read_dir(root).with_context(|| format!("не удалось прочитать {}", root.display()))?
    {
        let entry = entry?;
        // file_type() does not follow symlinks.
        if !entry.file_type()?.is_dir() {
            continue;
        }
        match entry.file_name().into_string() {
            Ok(name) if !name.starts_with('.') => dirs.push(name),
            Ok(_) => {}
            Err(name) => warn(&format!("пропущена папка с не-UTF-8 именем: {name:?}")),
        }
    }
    dirs.sort();

    let mut used_slugs = HashSet::new();
    let mut podcasts = Vec::new();
    for dir in dirs {
        let episodes = scan_podcast(&root.join(&dir), &dir, base_url, programs)?;
        if episodes.is_empty() {
            warn(&format!("{dir}: нет аудиофайлов, пропущено"));
            continue;
        }
        let slug = unique_slug(&slugify(&dir), &mut used_slugs);
        let total_duration_secs = episodes.iter().filter_map(|e| e.duration_secs).sum();
        podcasts.push(Podcast {
            title: clean(&dir).unwrap_or_else(|| slug.clone()),
            playlist: format!("playlists/dir/{slug}.m3u8"),
            slug,
            dir,
            total_duration_secs,
            episodes,
        });
    }
    unify_artist_spelling(&mut podcasts);
    Ok(podcasts)
}

/// An artist parsed from a file name is lowercase and has no
/// punctuation (`mrsunny`, `mordax bastards`). If the same artist (ignoring
/// case and punctuation) appears in the tags of other files, use the tag spelling
/// (`Mr.Sunny`, `MORDAX Bastards`), the first one seen.
fn unify_artist_spelling(podcasts: &mut [Podcast]) {
    let mut spelling: HashMap<String, String> = HashMap::new();
    for ep in podcasts
        .iter()
        .flat_map(|p| &p.episodes)
        .filter(|e| e.tagged)
    {
        if let Some(a) = &ep.artist {
            spelling.entry(normalize(a)).or_insert_with(|| a.clone());
        }
    }
    for ep in podcasts
        .iter_mut()
        .flat_map(|p| &mut p.episodes)
        .filter(|e| !e.tagged)
    {
        if let Some(a) = &mut ep.artist
            && let Some(s) = spelling.get(&normalize(a))
        {
            a.clone_from(s);
        }
    }
}

fn scan_podcast(
    path: &Path,
    dir: &str,
    base_url: &str,
    programs: &Programs,
) -> Result<Vec<Episode>> {
    let mut files = Vec::new();
    let mut skipped = 0usize;
    for entry in
        fs::read_dir(path).with_context(|| format!("не удалось прочитать {}", path.display()))?
    {
        let entry = entry?;
        let ft = entry.file_type()?;
        let name = match entry.file_name().into_string() {
            Ok(n) => n,
            Err(n) => {
                warn(&format!("{dir}: пропущен файл с не-UTF-8 именем: {n:?}"));
                continue;
            }
        };
        if name.starts_with('.') {
            continue;
        }
        if ft.is_dir() {
            warn(&format!(
                "{dir}/{name}: вложенные папки не поддерживаются, пропущено"
            ));
            continue;
        }
        if !ft.is_file() || !is_audio(&name) {
            skipped += 1;
            continue;
        }
        let size = entry.metadata()?.len();
        files.push((name, size));
    }
    if skipped > 0 {
        warn(&format!("{dir}: пропущено не-аудио: {skipped}"));
    }
    files.sort();

    let mut episodes = Vec::with_capacity(files.len());
    for (name, size) in files {
        let rel = format!("{dir}/{name}");
        let tag = read_tag(&path.join(&name), &rel);
        let tag_title = tag.as_ref().and_then(|t| t.title()).and_then(clean);
        let tag_artist = tag.as_ref().and_then(|t| t.artist()).and_then(clean);
        let album = tag.as_ref().and_then(|t| t.album()).and_then(clean);
        let program = programs.resolve(&name, album.as_deref());
        let tagged = tag_title.is_some();
        // No title tag: artist and title come from the file name.
        // An artist from the tag, if present, wins over the parsed one.
        let (title, artist) = match tag_title {
            Some(t) => (t, tag_artist),
            None => {
                let parsed = filename::parse(stem(&name), program.as_deref(), programs);
                let title = clean(&parsed.title).unwrap_or_else(|| name.clone());
                (
                    title,
                    tag_artist.or(parsed.artist.as_deref().and_then(clean)),
                )
            }
        };
        episodes.push(Episode {
            title,
            artist,
            program,
            album,
            duration_secs: tag
                .as_ref()
                .and_then(|t| t.duration())
                .and_then(|ms| plausible_duration(ms, size)),
            size,
            url: format!("{base_url}/media/{}", encode_path(&rel)),
            path: rel,
            tagged,
        });
    }
    Ok(episodes)
}

/// Reads the ID3v2 tag. For a partially corrupted tag, takes whatever could be read.
fn read_tag(path: &Path, rel: &str) -> Option<id3::Tag> {
    match id3::Tag::read_from_path(path) {
        Ok(tag) => Some(tag),
        Err(e) if matches!(e.kind, id3::ErrorKind::NoTag) => None,
        Err(e) => {
            warn(&format!("{rel}: ошибка чтения тега: {e}"));
            e.partial_tag
        }
    }
}

/// Plausible range of the average bitrate, kbit/s, used to check TLEN.
const BITRATE_KBPS: std::ops::RangeInclusive<u64> = 24..=512;

/// Converts TLEN (milliseconds) to seconds, dropping obviously wrong
/// values: some encoders write a garbage constant into TLEN
/// (e.g. 97392000 ms ≈ 27 h). We check that the average bitrate
/// computed from the file size is plausible.
fn plausible_duration(ms: u32, size: u64) -> Option<u64> {
    let secs = (u64::from(ms) + 500) / 1000;
    if secs == 0 {
        return None;
    }
    let kbps = size * 8 / secs / 1000;
    BITRATE_KBPS.contains(&kbps).then_some(secs)
}

fn is_audio(name: &str) -> bool {
    Path::new(name)
        .extension()
        .and_then(|e| e.to_str())
        .is_some_and(|e| AUDIO_EXT.iter().any(|a| e.eq_ignore_ascii_case(a)))
}

fn stem(name: &str) -> &str {
    Path::new(name)
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or(name)
}

fn warn(msg: &str) {
    eprintln!("предупреждение: {msg}");
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn duration_sanity() {
        // 2 hours at 128 kbit/s ≈ 115 MB is plausible.
        assert_eq!(plausible_duration(7_200_000, 115_200_000), Some(7200));
        // Garbage TLEN from real files: 27 h for 82 MB (~7 kbit/s).
        assert_eq!(plausible_duration(97_392_000, 82_000_000), None);
        assert_eq!(plausible_duration(0, 1000), None);
    }

    #[test]
    fn audio_ext() {
        assert!(is_audio("x.MP3"));
        assert!(!is_audio("index.html"));
        assert!(!is_audio("mp3"));
    }
}
