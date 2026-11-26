//! Walking the media library and reading ID3 tags.
//!
//! Files are not read in full: `id3` reads only the tag header at the start
//! of the file (over rclone that is a few kilobytes, not the whole file).

use crate::catalog::{Episode, Podcast};
use crate::filename;
use crate::mpeg;
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
    let mut tails = HashMap::new();
    for dir in dirs {
        let episodes = scan_podcast(&root.join(&dir), &dir, base_url, programs, &mut tails)?;
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
    match_tagged_siblings(&mut podcasts, &tails);
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

/// Makes untagged episodes look like tagged episodes of the same program.
///
/// A title parsed from a file name is `<program> <tail>` (`tails` maps an episode
/// path to its tail). If tagged episodes of the program start their titles with only
/// the last words of the program name (or none of it), the parsed title is cut the
/// same way: with tags like `Top 2014 Mega Podcast #1` for program `Kiss FM Top`,
/// `Kiss FM Top 2016 Mega Podcast 100` becomes `Top 2016 Mega Podcast 100`.
/// A parsed title that is just the program name (no tail) gets the tagged title
/// shared by several episodes, if any (`BOGOF` for `Buy One Get One Free`).
/// An untagged episode without an artist gets the most common tagged artist.
/// Programs without tagged episodes are left as is.
fn match_tagged_siblings(podcasts: &mut [Podcast], tails: &HashMap<String, String>) {
    struct Style {
        /// Program words that start a title.
        kept: Vec<String>,
        /// A title shared by several tagged episodes.
        common_title: Option<String>,
        artist: Option<String>,
    }
    // normalized program → style of its tagged episodes
    let mut styles: HashMap<String, Style> = HashMap::new();
    let mut tagged: HashMap<String, (String, Vec<&Episode>)> = HashMap::new();
    for ep in podcasts
        .iter()
        .flat_map(|p| &p.episodes)
        .filter(|e| e.tagged)
    {
        if let Some(program) = &ep.program {
            tagged
                .entry(normalize(program))
                .or_insert_with(|| (program.clone(), Vec::new()))
                .1
                .push(ep);
        }
    }
    for (key, (program, eps)) in tagged {
        let words: Vec<&str> = program.split_whitespace().collect();
        let skip = most_common(eps.iter().map(|e| {
            let title = normalize(&e.title);
            (0..words.len())
                .find(|&i| title.starts_with(&normalize(&words[i..].concat())))
                .unwrap_or(words.len())
        }))
        .map_or(0, |(skip, _)| skip);
        let style = Style {
            kept: words[skip..].iter().map(|w| (*w).to_owned()).collect(),
            common_title: most_common(eps.iter().map(|e| e.title.clone()))
                .filter(|(_, n)| *n >= 2)
                .map(|(t, _)| t),
            artist: most_common(eps.iter().filter_map(|e| e.artist.clone())).map(|(a, _)| a),
        };
        styles.insert(key, style);
    }

    for ep in podcasts
        .iter_mut()
        .flat_map(|p| &mut p.episodes)
        .filter(|e| !e.tagged)
    {
        let Some(program) = ep.program.as_deref() else {
            continue;
        };
        let Some(style) = styles.get(&normalize(program)) else {
            continue;
        };
        if let Some(tail) = tails.get(&ep.path) {
            ep.title = style
                .kept
                .iter()
                .map(String::as_str)
                .chain([tail.as_str()])
                .collect::<Vec<_>>()
                .join(" ");
        } else if normalize(&ep.title) == normalize(program)
            && let Some(t) = &style.common_title
        {
            ep.title.clone_from(t);
        }
        if ep.artist.is_none() {
            ep.artist.clone_from(&style.artist);
        }
    }
}

/// The most frequent item and its count; on a tie, the one seen first.
fn most_common<T: PartialEq>(items: impl Iterator<Item = T>) -> Option<(T, usize)> {
    let mut counts: Vec<(T, usize)> = Vec::new();
    for item in items {
        match counts.iter_mut().find(|(x, _)| *x == item) {
            Some((_, n)) => *n += 1,
            None => counts.push((item, 1)),
        }
    }
    let max = counts.iter().map(|(_, n)| *n).max()?;
    counts.into_iter().find(|(_, n)| *n == max)
}

fn scan_podcast(
    path: &Path,
    dir: &str,
    base_url: &str,
    programs: &Programs,
    tails: &mut HashMap<String, String>,
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
        let head = match mpeg::Head::read(&path.join(&name)) {
            Ok(h) => h,
            Err(e) => {
                warn(&format!("{rel}: не удалось прочитать, пропущено: {e}"));
                continue;
            }
        };
        let tag = if head.tag_too_big {
            warn(&format!(
                "{rel}: ID3-тег больше {} МБ, теги не читаются",
                mpeg::MAX_TAG >> 20
            ));
            None
        } else {
            read_tag(&head.bytes, &rel)
        };
        let probe = head.probe(size);
        if probe.is_none() {
            warn(&format!("{rel}: не найден MPEG-заголовок"));
        }
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
                if let Some(tail) = parsed.tail.as_deref().and_then(clean) {
                    tails.insert(rel.clone(), tail);
                }
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
            duration_secs: probe.as_ref().and_then(|p| p.duration_secs).or_else(|| {
                tag.as_ref()
                    .and_then(|t| t.duration())
                    .and_then(|ms| plausible_duration(ms, size))
            }),
            audio: probe.map(|p| p.info),
            size,
            url: format!("{base_url}/media/{}", encode_path(&rel)),
            path: rel,
            tagged,
        });
    }
    Ok(episodes)
}

/// Reads the ID3v2 tag. For a partially corrupted tag, takes whatever could be read.
fn read_tag(head: &[u8], rel: &str) -> Option<id3::Tag> {
    match id3::Tag::read_from2(std::io::Cursor::new(head)) {
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

    fn ep(path: &str, program: &str, title: &str, artist: Option<&str>, tagged: bool) -> Episode {
        Episode {
            title: title.to_owned(),
            artist: artist.map(str::to_owned),
            album: None,
            program: Some(program.to_owned()),
            duration_secs: None,
            audio: None,
            size: 0,
            path: path.to_owned(),
            url: String::new(),
            tagged,
        }
    }

    fn podcast(episodes: Vec<Episode>) -> Vec<Podcast> {
        vec![Podcast {
            slug: "p".into(),
            title: "P".into(),
            dir: "P".into(),
            playlist: String::new(),
            total_duration_secs: 0,
            episodes,
        }]
    }

    #[test]
    fn untagged_titles_follow_tagged_siblings() {
        let mut p = podcast(vec![
            ep(
                "a",
                "Kiss FM Top",
                "Top 2014 Mega Podcast #1",
                Some("KISSFM"),
                true,
            ),
            ep(
                "b",
                "Kiss FM Top",
                "Top 2014 Mega Podcast #2",
                Some("KISSFM"),
                true,
            ),
            ep(
                "c",
                "Kiss FM Top",
                "Kiss FM Top 2016 Mega Podcast 100",
                None,
                false,
            ),
            // No tagged siblings: left as is.
            ep(
                "d",
                "Kiss FM Dance Marafon",
                "Kiss FM Dance Marafon 2003",
                None,
                false,
            ),
            // An artist parsed from the file name is kept.
            ep(
                "e",
                "Kiss FM Top",
                "Kiss FM Top 2017",
                Some("Someone"),
                false,
            ),
        ]);
        let tails = [("c", "2016 Mega Podcast 100"), ("d", "2003"), ("e", "2017")]
            .map(|(k, v)| (k.to_owned(), v.to_owned()))
            .into();
        match_tagged_siblings(&mut p, &tails);
        let got: Vec<_> = p[0].episodes[2..]
            .iter()
            .map(|e| (e.artist.as_deref(), e.title.as_str()))
            .collect();
        assert_eq!(
            got,
            [
                (Some("KISSFM"), "Top 2016 Mega Podcast 100"),
                (None, "Kiss FM Dance Marafon 2003"),
                (Some("Someone"), "Top 2017"),
            ]
        );
    }

    #[test]
    fn bare_program_title_takes_common_tagged_title() {
        let bogof = "Buy One Get One Free";
        let mut p = podcast(vec![
            ep("a", bogof, "BOGOF", Some(bogof), true),
            ep("b", bogof, "BOGOF", Some(bogof), true),
            ep("c", bogof, bogof, None, false),
            ep("d", bogof, bogof, Some("Jalsomino Ml Key"), false),
        ]);
        match_tagged_siblings(&mut p, &HashMap::new());
        let got: Vec<_> = p[0].episodes[2..]
            .iter()
            .map(|e| (e.artist.as_deref(), e.title.as_str()))
            .collect();
        assert_eq!(
            got,
            [(Some(bogof), "BOGOF"), (Some("Jalsomino Ml Key"), "BOGOF")]
        );
    }

    #[test]
    fn full_program_name_in_tags_is_kept() {
        let mut p = podcast(vec![
            ep(
                "a",
                "Preview",
                "Preview Radioshow 19",
                Some("Dj Lutique"),
                true,
            ),
            ep(
                "b",
                "Preview",
                "Preview Radioshow 20",
                Some("Dj Lutique"),
                false,
            ),
        ]);
        let tails = [("b".to_owned(), "Radioshow 20".to_owned())].into();
        match_tagged_siblings(&mut p, &tails);
        assert_eq!(p[0].episodes[1].title, "Preview Radioshow 20");
    }

    #[test]
    fn audio_ext() {
        assert!(is_audio("x.MP3"));
        assert!(!is_audio("index.html"));
        assert!(!is_audio("mp3"));
    }
}
