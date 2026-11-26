//! Artist and title from the file name, for files without a title tag.
//!
//! KissFM file names look like this:
//! `<artist>-<program>-<YYYY-MM-DD>-podkasty-<DD.MM.YY>-kissfm.ua.mp3`,
//! but sometimes artist and program are swapped
//! (`best-clubs-show-harkov-misto-…`). So we look for the program among the words
//! (already determined via `programs.toml`) and treat the rest as the artist.
//! The title becomes the canonical program name plus the tail of the name
//! (episode number etc.):
//!
//! - `van-gogh-colours-of-house-…`       → «Van Gogh» — «Colours of House»;
//! - `best-clubs-show-harkov-misto-…`    → «Harkov Misto» — «Best Club Show»;
//! - `dj-lutique-preview-radioshow-20-…` → «Dj Lutique» — «Preview Radioshow 20 …»;
//! - `kissfm-top-2016-mega-podcast-100`  → no artist — «Kiss FM Top 2016 Mega Podcast 100».
//!
//! If no program is found in the name, the whole name is the artist and the
//! title becomes the program name. The artist's case is later refined from the tags
//! of other files, and the title and artist are adjusted to match tagged episodes
//! of the same program (see `scan`).

use crate::programs::{Programs, normalize};

/// Result of parsing a file name.
#[derive(Debug, PartialEq, Eq)]
pub struct Parsed {
    pub artist: Option<String>,
    pub title: String,
    /// The part of the title after the program name (episode number etc.),
    /// when the title is `<program> <tail>`.
    pub tail: Option<String>,
}

/// Parses a file name without extension. `program` is the already determined program.
pub fn parse(stem: &str, program: Option<&str>, programs: &Programs) -> Parsed {
    let words = words(stem);
    let Some(program) = program else {
        return Parsed {
            artist: None,
            title: join(&words).unwrap_or_else(|| stem.to_owned()),
            tail: None,
        };
    };

    // First look for the exact program name, then for spellings from `tags`:
    // in `mordax-bastards-zebra` the right match is «zebra», not the variant «bastards zebra».
    let canonical = normalize(program);
    let Some((i, j)) = find_span(&words, |norm| norm == canonical)
        .or_else(|| find_span(&words, |norm| programs.matches(program, norm)))
    else {
        return Parsed {
            artist: join(&words),
            title: program.to_owned(),
            tail: None,
        };
    };
    let (before, after) = (&words[..i], &words[j..]);
    let after_is_program = programs.matches(program, &normalize(&after.concat()));
    let after_starts_with_word = after
        .first()
        .and_then(|w| w.chars().next())
        .is_some_and(char::is_alphabetic);

    let (artist, tail): (&[&str], &[&str]) = if !before.is_empty() {
        (before, after)
    } else if after_is_program {
        // `buy-one-get-one-free-bogof`, `kissfm-club-kissfm-club`: the name is repeated.
        (&[], &[])
    } else if after_starts_with_word {
        // Program at the start of the name: the artist follows it.
        (after, &[])
    } else {
        // `kiss-fm-dance-marafon-2003`, `kissfm-top-2016-…`: the tail is the episode number.
        (&[], after)
    };
    let tail = join(tail);
    let title = match &tail {
        Some(t) => format!("{program} {t}"),
        None => program.to_owned(),
    };
    Parsed {
        artist: join(artist),
        title,
        tail,
    }
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
        [[program]]
        name = "Best Club Show"
        tags = ["Best Clubs Show"]
        [[program]]
        name = "Preview"
        [[program]]
        name = "&Friends"
        tags = ["and friends"]
        [[program]]
        name = "Kiss FM Dance Marafon"
        [[program]]
        name = "Zebra"
        tags = ["Bastards ZEBRA"]
        [[program]]
        name = "Buy One Get One Free"
        tags = ["BOGOF"]
    "#;

    fn p(stem: &str, program: Option<&str>) -> (Option<String>, String) {
        let rules = Programs::parse(RULES).unwrap();
        let r = parse(stem, program, &rules);
        (r.artist, r.title)
    }

    fn some(s: &str) -> Option<String> {
        Some(s.to_owned())
    }

    #[test]
    fn program_first() {
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
    fn repeated_program_is_not_artist() {
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
    fn program_not_in_name() {
        assert_eq!(
            p("mrsunny--podkasty-28.03.16-kissfm.ua", Some("&Friends")),
            (some("Mrsunny"), "&Friends".to_owned())
        );
    }

    #[test]
    fn no_program() {
        assert_eq!(
            p("some-show-2016-01-02-podkasty-02.01.16-kissfm.ua", None),
            (None, "Some Show".to_owned())
        );
    }
}
