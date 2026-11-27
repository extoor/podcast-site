//! Determining an episode's podcast (show): album tag + rules from `podcasts.toml`.
//!
//! Order:
//! 1. file name prefix from the rules (the longest wins): manual configuration
//!    beats tags;
//! 2. album tag (TALB) matching a rule's `name` or `tags`;
//! 3. album tag as is;
//! 4. otherwise there is no podcast (the episode goes to the «Без подкаста» playlist).
//!
//! Names are compared in normalized form: letters and digits only, lowercase,
//! so `Russia Goes Clubbing` and `russia goes clubbing` are the same podcast.

use crate::catalog::{Folder, Podcast};
use crate::text::{slugify, unique_slug};
use anyhow::{Context, Result, bail};
use serde::Deserialize;
use std::collections::{HashMap, HashSet};
use std::path::Path;

/// Playlist slug for episodes without a podcast.
pub const NO_PODCAST_SLUG: &str = "bez-podkasta";

#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
struct RulesFile {
    #[serde(default)]
    podcast: Vec<Rule>,
    #[serde(default)]
    artist: Vec<ArtistRule>,
}

/// Other spellings of an artist name (typos in tags and file names).
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct ArtistRule {
    /// Canonical artist name.
    name: String,
    /// Spellings to replace; compared ignoring case, spaces and punctuation.
    tags: Vec<String>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct Rule {
    /// Canonical podcast name.
    name: String,
    /// Spellings found in the album tag.
    #[serde(default)]
    tags: Vec<String>,
    /// File name prefixes (case-insensitive).
    #[serde(default)]
    files: Vec<String>,
}

/// Rules ready to apply.
#[derive(Debug, Default)]
pub struct PodcastRules {
    /// Normalized tag → canonical name.
    by_tag: HashMap<String, String>,
    /// (lowercase prefix, name), sorted by prefix length, longest first.
    by_file: Vec<(String, String)>,
    /// Normalized artist spelling → canonical artist name.
    artists: HashMap<String, String>,
}

impl PodcastRules {
    /// Loads the rules. A missing file is not an error: only tags are used then.
    pub fn load(path: &Path) -> Result<Self> {
        let text = match std::fs::read_to_string(path) {
            Ok(t) => t,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                eprintln!(
                    "предупреждение: {} не найден, подкасты берутся только из тегов",
                    path.display()
                );
                return Ok(Self::default());
            }
            Err(e) => {
                return Err(e).with_context(|| format!("не удалось прочитать {}", path.display()));
            }
        };
        Self::parse(&text).with_context(|| format!("ошибка в {}", path.display()))
    }

    pub fn parse(text: &str) -> Result<Self> {
        let file: RulesFile = toml::from_str(text)?;
        let mut by_tag = HashMap::new();
        let mut by_file: Vec<(String, String)> = Vec::new();
        for rule in file.podcast {
            let Some(name) = crate::text::clean(&rule.name) else {
                bail!("пустое название подкаста");
            };
            for tag in std::iter::once(&rule.name).chain(&rule.tags) {
                let key = normalize(tag);
                if key.is_empty() {
                    continue;
                }
                if let Some(prev) = by_tag.insert(key, name.clone())
                    && prev != name
                {
                    bail!("тег {tag:?} указан и у {prev:?}, и у {name:?}");
                }
            }
            for prefix in rule.files {
                let prefix = prefix.to_lowercase();
                if prefix.is_empty() {
                    bail!("{name}: пустой префикс файла");
                }
                if let Some((_, prev)) = by_file.iter().find(|(p, _)| *p == prefix) {
                    bail!("префикс {prefix:?} указан и у {prev:?}, и у {name:?}");
                }
                by_file.push((prefix, name.clone()));
            }
        }
        by_file.sort_by(|a, b| b.0.len().cmp(&a.0.len()).then_with(|| a.0.cmp(&b.0)));
        let mut artists = HashMap::new();
        for rule in file.artist {
            let Some(name) = crate::text::clean(&rule.name) else {
                bail!("пустое имя исполнителя");
            };
            for tag in rule.tags {
                let key = normalize(&tag);
                if key.is_empty() {
                    continue;
                }
                if let Some(prev) = artists.insert(key, name.clone())
                    && prev != name
                {
                    bail!("написание исполнителя {tag:?} указано и у {prev:?}, и у {name:?}");
                }
            }
        }
        Ok(Self {
            by_tag,
            by_file,
            artists,
        })
    }

    /// Determines the podcast from the file name and the (cleaned) album tag.
    pub fn resolve(&self, file_name: &str, album_tag: Option<&str>) -> Option<String> {
        let lower = file_name.to_lowercase();
        if let Some((_, name)) = self
            .by_file
            .iter()
            .find(|(p, _)| lower.starts_with(p.as_str()))
        {
            return Some(name.clone());
        }
        let tag = album_tag?;
        Some(
            self.by_tag
                .get(&normalize(tag))
                .cloned()
                .unwrap_or_else(|| tag.to_owned()),
        )
    }
}

impl PodcastRules {
    /// The canonical name of an artist spelled `a`, if a rule renames it.
    pub fn artist(&self, a: &str) -> Option<&str> {
        self.artists.get(&normalize(a)).map(String::as_str)
    }

    /// Whether the normalized string `norm` matches `podcast`:
    /// equals its name or one of the variants from `tags`.
    pub fn matches(&self, podcast: &str, norm: &str) -> bool {
        !norm.is_empty()
            && (normalize(podcast) == norm || self.by_tag.get(norm).is_some_and(|p| p == podcast))
    }
}

/// Groups the episodes of all folders by podcast.
///
/// Names differing only in case and punctuation are merged into one
/// podcast spelled as in the first episode seen; `Episode::podcast`
/// is rewritten to that spelling. Podcasts are sorted by name ignoring
/// case; episodes without a podcast form a separate group at the end. Within a group
/// episodes keep their catalog order.
pub fn group(folders: &mut [Folder]) -> Vec<Podcast> {
    // (name, episode paths, total duration)
    let mut groups: Vec<(Option<String>, Vec<String>, u64)> = Vec::new();
    let mut index: HashMap<Option<String>, usize> = HashMap::new();
    for ep in folders.iter_mut().flat_map(|p| &mut p.episodes) {
        let key = ep.podcast.as_deref().map(normalize);
        let i = *index.entry(key).or_insert_with(|| {
            groups.push((ep.podcast.clone(), Vec::new(), 0));
            groups.len() - 1
        });
        ep.podcast.clone_from(&groups[i].0);
        groups[i].1.push(ep.path.clone());
        groups[i].2 += ep.duration_secs.unwrap_or(0);
    }
    groups.sort_by_cached_key(|(name, _, _)| {
        (name.is_none(), name.as_deref().map(str::to_lowercase))
    });

    let mut used = HashSet::from([NO_PODCAST_SLUG.to_owned()]);
    groups
        .into_iter()
        .map(|(name, episodes, total)| {
            let slug = match &name {
                Some(n) => unique_slug(&slugify(n), &mut used),
                None => NO_PODCAST_SLUG.to_owned(),
            };
            Podcast {
                playlist: format!("playlists/podcast/{slug}.m3u8"),
                slug,
                name,
                total_duration_secs: total,
                episodes,
            }
        })
        .collect()
}

/// Key for comparing names: letters and digits only, lowercase.
pub fn normalize(s: &str) -> String {
    s.chars()
        .filter(|c| c.is_alphanumeric())
        .flat_map(char::to_lowercase)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::catalog::Episode;

    const RULES: &str = r#"
        [[podcast]]
        name = "Preview"
        files = ["lutique-"]

        [[podcast]]
        name = "Preview Radioshow"
        tags = ["Preview Radio Show"]
        files = ["lutique-preview-radioshow"]
    "#;

    #[test]
    fn resolve_order() {
        let p = PodcastRules::parse(RULES).unwrap();
        // The longest prefix wins.
        assert_eq!(
            p.resolve("Lutique-Preview-Radioshow-1.mp3", None)
                .as_deref(),
            Some("Preview Radioshow")
        );
        assert_eq!(p.resolve("lutique-x.mp3", None).as_deref(), Some("Preview"));
        // A file prefix wins over the tag.
        assert_eq!(
            p.resolve("lutique-x.mp3", Some("Other")).as_deref(),
            Some("Preview")
        );
        // Tags are matched ignoring case and punctuation.
        assert_eq!(
            p.resolve("x.mp3", Some("preview radio-show")).as_deref(),
            Some("Preview Radioshow")
        );
        // An unknown tag is used as is.
        assert_eq!(p.resolve("x.mp3", Some("Zebra")).as_deref(), Some("Zebra"));
        assert_eq!(p.resolve("x.mp3", None), None);
    }

    #[test]
    fn conflicts_rejected() {
        let dup = r#"
            [[podcast]]
            name = "A"
            files = ["x"]
            [[podcast]]
            name = "B"
            files = ["X"]
        "#;
        assert!(PodcastRules::parse(dup).is_err());
        assert!(PodcastRules::parse("[[podcast]]\nname = \"A\"\nfoo = 1").is_err());
    }

    fn ep(path: &str, podcast: Option<&str>) -> Episode {
        Episode {
            title: path.into(),
            artist: None,
            album: None,
            podcast: podcast.map(Into::into),
            duration_secs: None,
            audio: None,
            date: None,
            size: 0,
            path: path.into(),
            url: String::new(),
            tagged: false,
        }
    }

    #[test]
    fn group_merges_spellings() {
        let mut folders = vec![Folder {
            slug: "p".into(),
            title: "P".into(),
            dir: "P".into(),
            playlist: String::new(),
            total_duration_secs: 0,
            episodes: vec![
                ep("a", Some("Russia Goes Clubbing")),
                ep("b", None),
                ep("c", Some("russia goes clubbing")),
                ep("d", Some("Zebra")),
            ],
        }];
        let g = group(&mut folders);
        let names: Vec<_> = g.iter().map(|p| p.name.as_deref()).collect();
        assert_eq!(names, [Some("Russia Goes Clubbing"), Some("Zebra"), None]);
        assert_eq!(g[0].episodes, ["a", "c"]);
        assert_eq!(g[2].slug, NO_PODCAST_SLUG);
        assert_eq!(
            folders[0].episodes[2].podcast.as_deref(),
            Some("Russia Goes Clubbing")
        );
    }
}
