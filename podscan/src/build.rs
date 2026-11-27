//! The whole scan: media library → playlists (`.m3u8` and `.m3u`) and `catalog.json`.
//! Used by the `podscan` CLI and by `sitegen --scan` (one command for a container).

use crate::catalog::{Catalog, FORMAT_VERSION};
use crate::podcasts::{self, PodcastRules};
use crate::{m3u, scan};
use anyhow::{Context, Result, bail};
use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

/// What to scan and where to write.
#[derive(Debug, Clone)]
pub struct Options {
    /// Media root (one folder per top-level directory).
    pub media: PathBuf,
    /// Build directory: `catalog.json` and `playlists/` go there.
    pub out: PathBuf,
    /// Podcast rules (`podcasts.toml`); a missing file is not an error.
    pub rules: PathBuf,
    /// Site base URL for the playlists, without a trailing `/`.
    pub base_url: String,
}

/// Checks a base URL: http(s), no spaces or control characters; trailing `/` dropped.
pub fn base_url(url: &str) -> Result<String> {
    let url = url.trim_end_matches('/').to_owned();
    if !(url.starts_with("https://") || url.starts_with("http://"))
        || url.chars().any(|c| c.is_whitespace() || c.is_control())
    {
        bail!("некорректный базовый URL: {url:?}");
    }
    Ok(url)
}

/// Scans the library, writes the playlists and `catalog.json`, returns the catalog.
pub fn run(o: &Options) -> Result<Catalog> {
    let rules = PodcastRules::load(&o.rules)?;
    let mut folders = scan::scan(&o.media, &o.base_url, &rules)?;
    let podcast_list = podcasts::group(&mut folders);
    let catalog = Catalog {
        format_version: FORMAT_VERSION,
        generated_at: SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0),
        base_url: o.base_url.clone(),
        all_playlist: "playlists/all.m3u8".to_owned(),
        podcasts: podcast_list,
        folders,
    };

    let all_episodes = || catalog.folders.iter().flat_map(|p| &p.episodes);
    let by_path: HashMap<&str, _> = all_episodes().map(|e| (e.path.as_str(), e)).collect();

    let mut playlists: Vec<(String, String)> = Vec::new();
    playlists.push((
        catalog.all_playlist.clone(),
        m3u::render("Все эпизоды", all_episodes()),
    ));
    for p in &catalog.folders {
        playlists.push((p.playlist.clone(), m3u::render(&p.title, &p.episodes)));
    }
    for a in &catalog.podcasts {
        let title = a.name.as_deref().unwrap_or("Без подкаста");
        // Paths come from this same catalog, so all of them are found.
        let eps = a
            .episodes
            .iter()
            .filter_map(|path| by_path.get(path.as_str()).copied());
        playlists.push((a.playlist.clone(), m3u::render(title, eps)));
    }

    // Write playlists and remove stale ones in each directory.
    let mut keep: HashMap<PathBuf, HashSet<String>> = HashMap::new();
    for sub in ["playlists", "playlists/dir", "playlists/podcast"] {
        let dir = o.out.join(sub);
        fs::create_dir_all(&dir)
            .with_context(|| format!("не удалось создать {}", dir.display()))?;
        keep.insert(dir, HashSet::new());
    }
    // Every playlist twice: `.m3u8` (the web) and `.m3u` (Gopher), the same text.
    for (rel, text) in &playlists {
        for rel in [rel.clone(), m3u::m3u_path(rel)] {
            let path = o.out.join(&rel);
            write_atomic(&path, text.as_bytes())?;
            if let (Some(dir), Some(name)) =
                (path.parent(), path.file_name().and_then(|n| n.to_str()))
            {
                keep.entry(dir.to_path_buf())
                    .or_default()
                    .insert(name.to_owned());
            }
        }
    }
    for (dir, names) in &keep {
        remove_stale(dir, names)?;
    }

    let json = serde_json::to_vec_pretty(&catalog)?;
    write_atomic(&o.out.join("catalog.json"), &json)?;

    Ok(catalog)
}

/// One line about the catalog for the log.
pub fn summary(catalog: &Catalog) -> String {
    let episodes = || catalog.folders.iter().flat_map(|p| &p.episodes);
    let no_podcast = catalog
        .podcasts
        .iter()
        .find(|a| a.name.is_none())
        .map_or(0, |a| a.episodes.len());
    format!(
        "папок {}, подкастов {} (без подкаста: {no_podcast}), эпизодов {} (без тега названия: {}, без длительности: {})",
        catalog.folders.len(),
        catalog.podcasts.iter().filter(|a| a.name.is_some()).count(),
        episodes().count(),
        episodes().filter(|e| !e.tagged).count(),
        episodes().filter(|e| e.duration_secs.is_none()).count(),
    )
}

/// Writes to a temporary file next to `path` and renames it, so readers (servers)
/// never see a half-written file.
fn write_atomic(path: &Path, data: &[u8]) -> Result<()> {
    let mut tmp = path.as_os_str().to_owned();
    tmp.push(".tmp");
    let tmp = PathBuf::from(tmp);
    fs::write(&tmp, data).with_context(|| format!("не удалось записать {}", tmp.display()))?;
    fs::rename(&tmp, path).with_context(|| format!("не удалось переименовать в {}", path.display()))
}

/// Removes playlists of folders that are no longer in the catalog.
fn remove_stale(dir: &Path, keep: &HashSet<String>) -> Result<()> {
    for entry in fs::read_dir(dir)? {
        let entry = entry?;
        let name = entry.file_name();
        let Some(name) = name.to_str() else { continue };
        if (name.ends_with(".m3u8") || name.ends_with(".m3u")) && !keep.contains(name) {
            fs::remove_file(entry.path())
                .with_context(|| format!("не удалось удалить {}", entry.path().display()))?;
            eprintln!("удалён устаревший плейлист: {name}");
        }
    }
    Ok(())
}
