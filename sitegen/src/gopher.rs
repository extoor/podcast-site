//! Gopher menus (`gophermap` files) from the catalog.
//!
//! The files are complete menus, ready to be sent as is: every line is
//! `<type><text>\t<selector>\t<host>\t<port>\r\n` and the menu ends with `.\r\n`.
//! The server only maps selectors to files and never edits them.
//!
//! Selectors:
//! - `/`, `/podcast/<slug>/`, `/dir/<slug>/` — menus (`build/gopher/**/gophermap`);
//! - `/playlists/…` — playlists from `build/playlists`; menus link the `.m3u` twins
//!   of the `.m3u8` files (see `podscan::m3u::m3u_path`);
//! - `/media/<path>` — audio files from the media root.

use crate::banner;
use anyhow::Result;
use podscan::catalog::{AudioInfo, Catalog, Episode};
use podscan::m3u::m3u_path;
use std::collections::HashMap;
use std::fmt::Write;

/// Longest display text of a menu line, in characters.
pub const MAX_TEXT: usize = 70;

/// Where the menus point to: this server's host name and port.
#[derive(Debug, Clone)]
pub struct Server {
    pub host: String,
    pub port: u16,
}

/// A menu under construction.
struct Menu<'a> {
    server: &'a Server,
    out: String,
}

impl<'a> Menu<'a> {
    fn new(server: &'a Server) -> Self {
        Self {
            server,
            out: String::new(),
        }
    }

    /// A line of any type. Text is cleaned and cut to [`MAX_TEXT`]; a selector with
    /// a tab or a line break would corrupt the menu, so such a line is skipped.
    fn line(&mut self, kind: char, text: &str, selector: &str) {
        if selector.contains(['\t', '\r', '\n']) {
            return;
        }
        let (host, port) = (&self.server.host, self.server.port);
        // write! to a String cannot fail.
        let _ = write!(
            self.out,
            "{kind}{}\t{selector}\t{host}\t{port}\r\n",
            fit(text)
        );
    }

    fn info(&mut self, text: &str) {
        self.line('i', text, "");
    }

    fn blank(&mut self) {
        self.info("");
    }

    fn menu(&mut self, text: &str, selector: &str) {
        self.line('1', text, selector);
    }

    fn binary(&mut self, text: &str, selector: &str) {
        self.line('9', text, selector);
    }

    /// External link (type `h`, `URL:` selector), e.g. to the web version.
    fn url(&mut self, text: &str, url: &str) {
        self.line('h', text, &format!("URL:{url}"));
    }

    fn heading(&mut self, text: &str) {
        self.info(text);
        self.info(&"═".repeat(text.chars().count()));
    }

    /// The banner: each word block centered within [`MAX_TEXT`], like on the web.
    fn banner(&mut self, blocks: &[Vec<String>]) {
        for lines in blocks {
            let width = lines.iter().map(|l| l.chars().count()).max().unwrap_or(0);
            let pad = " ".repeat(MAX_TEXT.saturating_sub(width) / 2);
            for l in lines {
                self.info(&format!("{pad}{l}"));
            }
            self.blank();
        }
    }

    fn finish(mut self) -> String {
        self.out.push_str(".\r\n");
        self.out
    }
}

/// The upper half (0x80..0xFF) of CP866, the code page of DOS and of Gopher
/// browsers on the ZX Spectrum: they show anything else as "?".
const CP866_HIGH: &str = "АБВГДЕЖЗИЙКЛМНОПРСТУФХЦЧШЩЪЫЬЭЮЯабвгдежзийклмноп░▒▓│┤╡╢╖╕╣║╗╝╜╛┐└┴┬├─┼╞╟╚╔╩╦╠═╬╧╨╤╥╙╘╒╓╫╪┘┌█▄▌▐▀рстуфхцчшщъыьэюяЁёЄєЇїЎў°∙·√№¤■\u{a0}";

/// A character as text that CP866 can show: itself, an ASCII stand-in, or `?`.
fn to_cp866(c: char, out: &mut String) {
    match c {
        c if c.is_control() => out.push(' '),
        c if c.is_ascii() || CP866_HIGH.contains(c) => out.push(c),
        '—' | '–' | '‒' | '−' => out.push('-'),
        '…' => out.push_str("..."),
        '«' | '»' | '“' | '”' | '„' => out.push('"'),
        '‘' | '’' | '‚' => out.push('\''),
        '◄' | '←' => out.push('<'),
        '►' | '→' => out.push('>'),
        'І' => out.push('I'), // Ukrainian I is not in CP866: the Latin one looks the same
        'і' => out.push('i'),
        _ => out.push('?'),
    }
}

/// Display text: control characters (tabs, line breaks) become spaces, characters
/// outside CP866 become ASCII stand-ins, and the text is cut to [`MAX_TEXT`].
fn fit(text: &str) -> String {
    let mut clean = String::with_capacity(text.len());
    for c in text.chars() {
        to_cp866(c, &mut clean);
    }
    if clean.chars().count() <= MAX_TEXT {
        return clean;
    }
    let mut cut: String = clean.chars().take(MAX_TEXT - 3).collect();
    cut.push_str("...");
    cut
}

fn duration(secs: u64) -> String {
    format!("{}:{:02}:{:02}", secs / 3600, secs % 3600 / 60, secs % 60)
}

/// `MPEG-1 L3 192k` — short stream info for the episode line.
/// Size like a file list: `86.4 MB`, `431.4 KB`.
fn size(bytes: u64) -> String {
    #[allow(clippy::cast_precision_loss)] // sizes far below 2^52
    let b = bytes as f64;
    if bytes >= 1_000_000 {
        format!("{:.1} MB", b / 1e6)
    } else {
        format!("{:.1} KB", b / 1e3)
    }
}

/// Bitrate column: `192k`, `~160k` for the average of a VBR stream.
fn bitrate(a: &AudioInfo) -> String {
    let vbr = if a.vbr { "~" } else { "" };
    format!("{vbr}{}k", a.bitrate_kbps)
}

/// `left` cut or padded so that `right` ends exactly at [`MAX_TEXT`]: columns line
/// up from row to row. Both are display text (one column per character).
fn columns(left: &str, right: &str) -> String {
    let left = fit(left);
    let room = MAX_TEXT.saturating_sub(right.chars().count() + 2);
    let n = left.chars().count();
    let left = if n > room {
        let mut cut: String = left.chars().take(room.saturating_sub(3)).collect();
        cut.push_str("...");
        cut
    } else {
        format!("{left}{}", " ".repeat(room - n))
    };
    format!("{left}  {right}")
}

/// Generated menus: path relative to the gopher root → file contents.
pub type Files = Vec<(String, String)>;

/// Builds all menus.
pub fn menus(catalog: &Catalog, server: &Server) -> Result<Files> {
    let banner: Vec<Vec<String>> = ["KISS FM", "PODCASTS"]
        .into_iter()
        .map(|t| banner::bitmap(t).map(|b| banner::half_blocks(&b)))
        .collect::<Result<_>>()?;
    let episodes: HashMap<&str, &Episode> = catalog
        .folders
        .iter()
        .flat_map(|p| &p.episodes)
        .map(|e| (e.path.as_str(), e))
        .collect();
    let web = &catalog.base_url;

    let mut files = Vec::new();

    // Root menu.
    let mut m = Menu::new(server);
    m.banner(&banner);
    m.binary(
        &format!("Все эпизоды одним плейлистом [m3u] ({})", episodes.len()),
        &format!("/{}", m3u_path(&catalog.all_playlist)),
    );
    m.blank();
    m.heading("ПОДКАСТЫ");
    for p in &catalog.podcasts {
        let name = p.name.as_deref().unwrap_or("Без подкаста");
        m.menu(
            &format!("{name} ({})", p.episodes.len()),
            &format!("/podcast/{}/", p.slug),
        );
    }
    m.blank();
    m.heading("ПАПКИ");
    for p in &catalog.folders {
        m.menu(
            &format!("{} ({})", p.title, p.episodes.len()),
            &format!("/dir/{}/", p.slug),
        );
    }
    m.blank();
    m.url("Веб-версия", &format!("{web}/"));
    m.info("Плейлисты M3U открываются в VLC, foobar2000, mpv.");
    m.info(&format!("Версия: {}", crate::COMMIT));
    files.push(("gophermap".to_owned(), m.finish()));

    // A menu per podcast (podcast) and per folder.
    for p in &catalog.podcasts {
        let name = p.name.as_deref().unwrap_or("Без подкаста");
        let eps = p
            .episodes
            .iter()
            .filter_map(|path| episodes.get(path.as_str()).copied());
        let menu = episode_menu(server, &banner, name, &p.playlist, eps);
        files.push((format!("podcast/{}/gophermap", p.slug), menu));
    }
    for p in &catalog.folders {
        let menu = episode_menu(server, &banner, &p.title, &p.playlist, p.episodes.iter());
        files.push((format!("dir/{}/gophermap", p.slug), menu));
    }
    Ok(files)
}

/// A list of episodes: one line each (type 9, the file over Gopher) with columns.
fn episode_menu<'a>(
    server: &Server,
    banner: &[Vec<String>],
    title: &str,
    playlist: &str,
    episodes: impl Iterator<Item = &'a Episode>,
) -> String {
    let episodes: Vec<&Episode> = episodes.collect();
    let width = episodes.len().to_string().len();
    let mut m = Menu::new(server);
    m.banner(banner);
    m.menu("< все подкасты", "/");
    m.blank();
    m.heading(&title.to_uppercase());
    m.binary(
        &format!("Скачать плейлист [m3u] ({})", episodes.len()),
        &format!("/{}", m3u_path(playlist)),
    );
    m.blank();
    for (i, ep) in episodes.iter().enumerate() {
        let name = match &ep.artist {
            Some(a) => format!("{a} - {}", ep.title),
            None => ep.title.clone(),
        };
        // One line per episode, like a file list: name, then fixed-width columns
        // date | duration | size | bitrate (a missing value keeps its column blank).
        let right = format!(
            "{:<10} | {:>7} | {:>8} | {:>5}",
            ep.date.as_deref().map(crate::show_date).unwrap_or_default(),
            ep.duration_secs.map(duration).unwrap_or_default(),
            size(ep.size),
            ep.audio.as_ref().map(bitrate).unwrap_or_default(),
        );
        m.binary(
            &columns(&format!("{:0width$}. {name}", i + 1), &right),
            &format!("/media/{}", ep.path),
        );
    }
    m.finish()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn server() -> Server {
        Server {
            host: "localhost".into(),
            port: 7070,
        }
    }

    #[test]
    fn line_format() {
        let s = server();
        let mut m = Menu::new(&s);
        m.menu("Подкасты", "/podcast/x/");
        m.url("Веб", "https://example.org/");
        m.info("text\twith\ttabs");
        assert_eq!(
            m.finish(),
            "1Подкасты\t/podcast/x/\tlocalhost\t7070\r\n\
             hВеб\tURL:https://example.org/\tlocalhost\t7070\r\n\
             itext with tabs\t\tlocalhost\t7070\r\n\
             .\r\n"
        );
    }

    #[test]
    fn bad_selector_is_skipped() {
        let s = server();
        let mut m = Menu::new(&s);
        m.binary("x", "/media/a\tb.mp3");
        m.binary("y", "/media/a\r\nb.mp3");
        assert_eq!(m.finish(), ".\r\n");
    }

    #[test]
    fn long_text_is_cut() {
        let long = "я".repeat(100);
        let f = fit(&long);
        assert_eq!(f.chars().count(), MAX_TEXT);
        assert!(f.ends_with("..."));
    }

    #[test]
    fn episode_columns() {
        let right = "20.07.2015 | 0:59:43 |  86.4 MB |  192k";
        let short = columns("01. A - B", right);
        let long = columns(&format!("02. {}", "x".repeat(80)), right);
        for line in [&short, &long] {
            assert_eq!(line.chars().count(), MAX_TEXT);
            assert!(line.ends_with(right));
        }
        assert!(long.contains("...  20.07"));
        assert_eq!(size(86_399_813), "86.4 MB");
        assert_eq!(size(431_400), "431.4 KB");
    }

    #[test]
    fn cp866_only() {
        assert_eq!(
            fit("Привет — ёж «рус» Їжак ░█ … ◄"),
            "Привет - ёж \"рус\" Їжак ░█ ... <"
        );
        assert_eq!(fit("Київ і Іра ★"), "Київ i Iра ?");
        for c in fit(&(0x20u32..0x3000)
            .filter_map(char::from_u32)
            .collect::<String>())
        .chars()
        {
            assert!(c.is_ascii() || CP866_HIGH.contains(c), "{c:?}");
        }
    }
}
