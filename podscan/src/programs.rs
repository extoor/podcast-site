//! Determining an episode's program (show): album tag + rules from `programs.toml`.
//!
//! Order:
//! 1. file name prefix from the rules (the longest wins): manual configuration
//!    beats tags;
//! 2. album tag (TALB) matching a rule's `name` or `tags`;
//! 3. album tag as is;
//! 4. otherwise there is no program (the episode goes to the «Без программы» playlist).
//!
//! Names are compared in normalized form: letters and digits only, lowercase,
//! so `Russia Goes Clubbing` and `russia goes clubbing` are the same program.

use crate::catalog::{Podcast, Program};
use crate::text::{slugify, unique_slug};
use anyhow::{Context, Result, bail};
use serde::Deserialize;
use std::collections::{HashMap, HashSet};
use std::path::Path;

/// Playlist slug for episodes without a program.
pub const NO_PROGRAM_SLUG: &str = "bez-programmy";

#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
struct ProgramsFile {
    #[serde(default)]
    program: Vec<Rule>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct Rule {
    /// Canonical program name.
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
pub struct Programs {
    /// Normalized tag → canonical name.
    by_tag: HashMap<String, String>,
    /// (lowercase prefix, name), sorted by prefix length, longest first.
    by_file: Vec<(String, String)>,
}

impl Programs {
    /// Loads the rules. A missing file is not an error: only tags are used then.
    pub fn load(path: &Path) -> Result<Self> {
        let text = match std::fs::read_to_string(path) {
            Ok(t) => t,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                eprintln!(
                    "предупреждение: {} не найден, программы берутся только из тегов",
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
        let file: ProgramsFile = toml::from_str(text)?;
        let mut by_tag = HashMap::new();
        let mut by_file: Vec<(String, String)> = Vec::new();
        for rule in file.program {
            let Some(name) = crate::text::clean(&rule.name) else {
                bail!("пустое название программы");
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
        Ok(Self { by_tag, by_file })
    }

    /// Determines the program from the file name and the (cleaned) album tag.
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

impl Programs {
    /// Whether the normalized string `norm` matches `program`:
    /// equals its name or one of the variants from `tags`.
    pub fn matches(&self, program: &str, norm: &str) -> bool {
        !norm.is_empty()
            && (normalize(program) == norm || self.by_tag.get(norm).is_some_and(|p| p == program))
    }
}

/// Groups the episodes of all podcasts by program.
///
/// Names differing only in case and punctuation are merged into one
/// program spelled as in the first episode seen; `Episode::program`
/// is rewritten to that spelling. Programs are sorted by name ignoring
/// case; episodes without a program form a separate group at the end. Within a group
/// episodes keep their catalog order.
pub fn group(podcasts: &mut [Podcast]) -> Vec<Program> {
    // (name, episode paths, total duration)
    let mut groups: Vec<(Option<String>, Vec<String>, u64)> = Vec::new();
    let mut index: HashMap<Option<String>, usize> = HashMap::new();
    for ep in podcasts.iter_mut().flat_map(|p| &mut p.episodes) {
        let key = ep.program.as_deref().map(normalize);
        let i = *index.entry(key).or_insert_with(|| {
            groups.push((ep.program.clone(), Vec::new(), 0));
            groups.len() - 1
        });
        ep.program.clone_from(&groups[i].0);
        groups[i].1.push(ep.path.clone());
        groups[i].2 += ep.duration_secs.unwrap_or(0);
    }
    groups.sort_by_cached_key(|(name, _, _)| {
        (name.is_none(), name.as_deref().map(str::to_lowercase))
    });

    let mut used = HashSet::from([NO_PROGRAM_SLUG.to_owned()]);
    groups
        .into_iter()
        .map(|(name, episodes, total)| {
            let slug = match &name {
                Some(n) => unique_slug(&slugify(n), &mut used),
                None => NO_PROGRAM_SLUG.to_owned(),
            };
            Program {
                playlist: format!("playlists/program/{slug}.m3u8"),
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
        [[program]]
        name = "Preview"
        files = ["lutique-"]

        [[program]]
        name = "Preview Radioshow"
        tags = ["Preview Radio Show"]
        files = ["lutique-preview-radioshow"]
    "#;

    #[test]
    fn resolve_order() {
        let p = Programs::parse(RULES).unwrap();
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
            [[program]]
            name = "A"
            files = ["x"]
            [[program]]
            name = "B"
            files = ["X"]
        "#;
        assert!(Programs::parse(dup).is_err());
        assert!(Programs::parse("[[program]]\nname = \"A\"\nfoo = 1").is_err());
    }

    fn ep(path: &str, program: Option<&str>) -> Episode {
        Episode {
            title: path.into(),
            artist: None,
            album: None,
            program: program.map(Into::into),
            duration_secs: None,
            audio: None,
            size: 0,
            path: path.into(),
            url: String::new(),
            tagged: false,
        }
    }

    #[test]
    fn group_merges_spellings() {
        let mut podcasts = vec![Podcast {
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
        let g = group(&mut podcasts);
        let names: Vec<_> = g.iter().map(|p| p.name.as_deref()).collect();
        assert_eq!(names, [Some("Russia Goes Clubbing"), Some("Zebra"), None]);
        assert_eq!(g[0].episodes, ["a", "c"]);
        assert_eq!(g[2].slug, NO_PROGRAM_SLUG);
        assert_eq!(
            podcasts[0].episodes[2].program.as_deref(),
            Some("Russia Goes Clubbing")
        );
    }
}
