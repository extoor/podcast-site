//! Artist and title from the file name, for files without a title tag.
//!
//! KissFM file names look like this:
//! `<artist>-<podcast>-<YYYY-MM-DD>-podkasty-<DD.MM.YY>-kissfm.ua.mp3`,
//! but sometimes artist and podcast are swapped
//! (`best-clubs-show-harkov-misto-…`). So we look for the podcast among the words
//! (already determined via `podcasts.toml`) and treat the rest as the artist.
//! The title becomes the canonical podcast name plus the tail of the name
//! (episode number etc.):
//!
//! - `van-gogh-colours-of-house-…`       → «Van Gogh» — «Colours of House»;
//! - `best-clubs-show-harkov-misto-…`    → «Harkov Misto» — «Best Club Show»;
//! - `dj-lutique-preview-radioshow-20-…` → «Dj Lutique» — «Preview Radioshow 20 …»;
//! - `kissfm-top-2016-mega-podcast-100`  → no artist — «Kiss FM Top 2016 Mega Podcast 100».
//!
//! If no podcast is found in the name, the whole name is the artist and the
//! title becomes the podcast name. The artist's case is later refined from the tags
//! of other files, and the title and artist are adjusted to match tagged episodes
//! of the same podcast (see `scan`).

use crate::podcasts::{PodcastRules, normalize};

/// Result of parsing a file name.
#[derive(Debug, PartialEq, Eq)]
pub struct Parsed {
    pub artist: Option<String>,
    pub title: String,
    /// The part of the title after the podcast name (episode number etc.),
    /// when the title is `<podcast> <tail>`.
    pub tail: Option<String>,
}

/// Parses a file name without extension. `podcast` is the already determined podcast.
pub fn parse(stem: &str, podcast: Option<&str>, rules: &PodcastRules) -> Parsed {
    let words = words(stem);
    let Some(podcast) = podcast else {
        return Parsed {
            artist: None,
            title: join(&words).unwrap_or_else(|| stem.to_owned()),
            tail: None,
        };
    };

    // First look for the exact podcast name, then for spellings from `tags`:
    // in `mordax-bastards-zebra` the right match is «zebra», not the variant «bastards zebra».
    let canonical = normalize(podcast);
    let Some((i, j)) = find_span(&words, |norm| norm == canonical)
        .or_else(|| find_span(&words, |norm| rules.matches(podcast, norm)))
    else {
        return Parsed {
            artist: join(&words),
            title: podcast.to_owned(),
            tail: None,
        };
    };
    let (before, after) = (&words[..i], &words[j..]);
    let after_is_podcast = rules.matches(podcast, &normalize(&after.concat()));
    let after_starts_with_word = after
        .first()
        .and_then(|w| w.chars().next())
        .is_some_and(char::is_alphabetic);

    let (artist, tail): (&[&str], &[&str]) = if !before.is_empty() {
        (before, after)
    } else if after_is_podcast {
        // `buy-one-get-one-free-bogof`, `kissfm-club-kissfm-club`: the name is repeated.
        (&[], &[])
    } else if after_starts_with_word {
        // Podcast at the start of the name: the artist follows it.
        (after, &[])
    } else {
        // `kiss-fm-dance-marafon-2003`, `kissfm-top-2016-…`: the tail is the episode number.
        (&[], after)
    };
    let tail = join(tail);
    let title = match &tail {
        Some(t) => format!("{podcast} {t}"),
        None => podcast.to_owned(),
    };
    Parsed {
        artist: join(artist),
        title,
        tail,
    }
}

/// Release date `YYYY-MM-DD` from a file name without extension: the publication
/// date `podkasty-DD.MM.YY` of KissFM names, otherwise a `YYYY-MM-DD` run of words.
pub fn date(stem: &str) -> Option<String> {
    let lower = stem.to_ascii_lowercase();
    if let Some(i) = lower.find("podkasty-")
        && let Some(d) = lower.get(i + 9..i + 17)
        && let [dd, mm, yy] = d.split('.').collect::<Vec<_>>()[..]
        && is_digits(dd, 2)
        && is_digits(mm, 2)
        && is_digits(yy, 2)
    {
        return valid_date(&format!("20{yy}"), mm, dd);
    }
    let words: Vec<&str> = stem.split(['-', '_', ' ']).collect();
    words.windows(3).find_map(|w| {
        (is_digits(w[0], 4) && is_digits(w[1], 2) && is_digits(w[2], 2))
            .then(|| valid_date(w[0], w[1], w[2]))
            .flatten()
    })
}

fn valid_date(y: &str, m: &str, d: &str) -> Option<String> {
    let (mn, dn): (u32, u32) = (m.parse().ok()?, d.parse().ok()?);
    ((1..=12).contains(&mn) && (1..=31).contains(&dn)).then(|| format!("{y}-{m}-{d}"))
}

/// Words of the name without the `-podkasty-…` suffix and the `YYYY-MM-DD` date.
fn words(stem: &str) -> Vec<&str> {
    let lower = stem.to_lowercase();
    // An index in `lower` matches an index in `stem` only for ASCII; otherwise don't cut.
    let base = match lower.find("-podkasty-") {
        Some(i) if stem.is_ascii() => &stem[..i],
        _ => stem,
    };
    let mut words: Vec<&str> = base
        .split(['-', '_', ' '])
        .filter(|w| !w.is_empty())
        .collect();
    if let [.., y, m, d] = words[..]
        && is_digits(y, 4)
        && is_digits(m, 2)
        && is_digits(d, 2)
    {
        words.truncate(words.len() - 3);
    }
    words
}

/// The first (leftmost, then longest) run of words matching `pred`.
fn find_span(words: &[&str], pred: impl Fn(&str) -> bool) -> Option<(usize, usize)> {
    (0..words.len()).find_map(|i| {
        (i + 1..=words.len())
            .rev()
            .find(|&j| pred(&normalize(&words[i..j].concat())))
            .map(|j| (i, j))
    })
}

fn is_digits(s: &str, len: usize) -> bool {
    s.len() == len && s.bytes().all(|b| b.is_ascii_digit())
}

/// Capitalized words joined by spaces; an empty list → `None`.
fn join(words: &[&str]) -> Option<String> {
    let s = words
        .iter()
        .map(|w| capitalize(w))
        .collect::<Vec<_>>()
        .join(" ");
    (!s.is_empty()).then_some(s)
}

fn capitalize(w: &str) -> String {
    let mut c = w.chars();
    match c.next() {
        Some(f) => f.to_uppercase().chain(c).collect(),
        None => String::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const RULES: &str = r#"
        [[podcast]]
        name = "Best Club Show"
        tags = ["Best Clubs Show"]
        [[podcast]]
        name = "Preview"
        [[podcast]]
        name = "&Friends"
        tags = ["and friends"]
        [[podcast]]
        name = "Kiss FM Dance Marafon"
        [[podcast]]
        name = "Zebra"
        tags = ["Bastards ZEBRA"]
        [[podcast]]
        name = "Buy One Get One Free"
        tags = ["BOGOF"]
    "#;

    fn p(stem: &str, podcast: Option<&str>) -> (Option<String>, String) {
        let rules = PodcastRules::parse(RULES).unwrap();
        let r = parse(stem, podcast, &rules);
        (r.artist, r.title)
    }

    fn some(s: &str) -> Option<String> {
        Some(s.to_owned())
    }

    #[test]
    fn podcast_first() {
        assert_eq!(
            p(
                "best-clubs-show-harkov-misto-2015-07-06-podkasty-06.07.15-kissfm.ua",
                Some("Best Club Show")
            ),
            (some("Harkov Misto"), "Best Club Show".to_owned())
        );
    }

    #[test]
    fn artist_first_with_tail() {
        assert_eq!(
            p(
                "dj-lutique-preview-radioshow-20-vol4-podkasty-06.11.18-kissfm.ua",
                Some("Preview")
            ),
            (some("Dj Lutique"), "Preview Radioshow 20 Vol4".to_owned())
        );
        assert_eq!(
            p(
                "mrsunny-falkon-andfriends-2016-07-18-podkasty-18.07.16-kissfm.ua",
                Some("&Friends")
            ),
            (some("Mrsunny Falkon"), "&Friends".to_owned())
        );
    }

    #[test]
    fn canonical_name_preferred() {
        assert_eq!(
            p("mordax-bastards-zebra-2016-09-17", Some("Zebra")),
            (some("Mordax Bastards"), "Zebra".to_owned())
        );
    }

    #[test]
    fn repeated_podcast_is_not_artist() {
        assert_eq!(
            p(
                "buy-one-get-one-free-bogof-2015-04-20",
                Some("Buy One Get One Free")
            ),
            (None, "Buy One Get One Free".to_owned())
        );
    }

    #[test]
    fn numeric_tail_stays_in_title() {
        assert_eq!(
            p(
                "kiss-fm-dance-marafon-2003-podkasty-17.12.14-kissfm.ua",
                Some("Kiss FM Dance Marafon")
            ),
            (None, "Kiss FM Dance Marafon 2003".to_owned())
        );
    }

    #[test]
    fn podcast_not_in_name() {
        assert_eq!(
            p("mrsunny--podkasty-28.03.16-kissfm.ua", Some("&Friends")),
            (some("Mrsunny"), "&Friends".to_owned())
        );
    }

    #[test]
    fn dates() {
        assert_eq!(
            date("mrsunny--podkasty-28.03.16-kissfm.ua").as_deref(),
            Some("2016-03-28")
        );
        // the publication date wins over the date in the name
        assert_eq!(
            date("zebra-2015-04-04-podkasty-05.04.15-kissfm.ua").as_deref(),
            Some("2015-04-05")
        );
        assert_eq!(date("show-2016-01-02").as_deref(), Some("2016-01-02"));
        assert_eq!(date("show-2016-13-02"), None);
        assert_eq!(date("kissfm-top-2016-mega-podcast-100"), None);
        assert_eq!(date("x-podkasty-ab.cd.ef"), None);
    }

    #[test]
    fn no_podcast() {
        assert_eq!(
            p("some-show-2016-01-02-podkasty-02.01.16-kissfm.ua", None),
            (None, "Some Show".to_owned())
        );
    }
}
