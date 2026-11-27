//! sitegen: builds the site from `catalog.json` — Gopher menus and HTML pages.

#![forbid(unsafe_code)]

pub mod banner;
pub mod gopher;
pub mod html;
pub mod icon;

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
    fn show_date() {
        assert_eq!(super::show_date("2015-07-20"), "20.07.2015");
        assert_eq!(super::show_date("garbage"), "garbage");
    }
}
