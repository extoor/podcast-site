//! sitegen: builds the site from `catalog.json` — Gopher menus and HTML pages.

#![forbid(unsafe_code)]

pub mod banner;
pub mod engine;
pub mod gopher;
pub mod html;
pub mod icon;

/// The commit the generator was built from (short hash, "-dirty" for a local build with
/// uncommitted changes, "dev" without git): the site's version. See build.rs.
pub const COMMIT: &str = env!("SITE_COMMIT");

/// Where a commit can be looked at; only for a clean commit hash.
pub fn commit_url(commit: &str) -> Option<String> {
    (commit.len() >= 7 && commit.chars().all(|c| c.is_ascii_hexdigit()))
        .then(|| format!("https://github.com/extoor/podcast-site/commit/{commit}"))
}

/// `robots.txt` for the web root and the Gopher root (Gopher crawlers such as
/// Veronica-2 read it too): no crawler is welcome anywhere, so the site stays out
/// of search indexes and their caches.
pub const ROBOTS_TXT: &str = "# This site is not for search engines: no crawling, no indexing, no cache.\n\
User-agent: *\n\
Disallow: /\n";

/// A catalog date `YYYY-MM-DD` as shown to people: `DD.MM.YYYY`.
/// Anything else is returned as is.
pub fn show_date(iso: &str) -> String {
    match iso.split('-').collect::<Vec<_>>()[..] {
        [y, m, d] if y.len() == 4 && m.len() == 2 && d.len() == 2 => format!("{d}.{m}.{y}"),
        _ => iso.to_owned(),
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn commit_url() {
        assert_eq!(
            super::commit_url("a1b2c3d").as_deref(),
            Some("https://github.com/extoor/podcast-site/commit/a1b2c3d")
        );
        assert_eq!(super::commit_url("a1b2c3d-dirty"), None);
        assert_eq!(super::commit_url("dev"), None);
        assert!(!super::COMMIT.is_empty());
    }

    #[test]
    fn show_date() {
        assert_eq!(super::show_date("2015-07-20"), "20.07.2015");
        assert_eq!(super::show_date("garbage"), "garbage");
    }
}
