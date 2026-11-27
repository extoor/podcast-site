//! HTML pages from the catalog: the index and a page per podcast (podcast) and
//! per folder, in the text-only retro style.
//!
//! Everything a page needs is inside it: styles, the player (`web/textplayer`) and
//! keyboard navigation (`web/keynav`) are inlined; only fonts come from a CDN.
//! Links are root-relative (`/media/…`, `/playlists/…`), so the pages work on any host.

use crate::banner;
use crate::gopher::Server;
use anyhow::{Result, bail};
use podscan::catalog::{AudioInfo, Catalog, Episode};
use podscan::text::encode_path;
use std::collections::HashMap;
use std::fmt::Write;

const TEXTPLAYER_JS: &str = include_str!("../../web/textplayer/textplayer.js");
const KEYNAV_JS: &str = include_str!("../../web/keynav/keynav.js");

const SITE_TITLE: &str = "KISS FM PODCASTS";

const CSS: &str = r#"
:root {
  --bg: #000; --fg: #c8c8c8; --dim: #5a5a5a; --link: #6ef0ff; --hi: #fff35c;
  --rainbow: linear-gradient(90deg, #ff3b30, #ff9a3c, #fff35c, #8dff5c, #5cffc8, #5cd8ff, #3b6bff, #1a2cff);
  color-scheme: dark;
}
* { box-sizing: border-box; }
body { margin: 0; background: var(--bg); color: var(--fg);
  font: 15px/1.6 "JetBrains Mono", "DejaVu Sans Mono", Menlo, Consolas, monospace; }
main { max-width: 52rem; margin: 0 auto; padding: 2rem 1rem 4rem; }
.sr { position: absolute; width: 1px; height: 1px; overflow: hidden; clip-path: inset(50%); }

/* banner: three text layers, fill on top of two outline copies */
.banners { display: grid; justify-items: center; gap: .6em; margin-bottom: 2.5rem;
  font-size: min(10px, calc((100vw - 2rem) / 80)); }
.banner { position: relative; width: calc(var(--w) * 1ch + 1ch); }
.banner pre { margin: 0; font: inherit; line-height: normal;
  font-family: "DejaVu Sans Mono", Menlo, Consolas, "Liberation Mono", monospace;
  background: var(--rainbow); background-size: calc(126ch) 100%;
  -webkit-background-clip: text; background-clip: text; color: transparent; }
.banner .ol { position: absolute; left: 0; top: 0; }
.banner .ol1 { transform: translate(-.15ch, -.2lh); opacity: .9; }
.banner .ol2 { transform: translate(.25ch, .15lh); opacity: .55; }
.banner .fill { position: relative; }

h2 { font: 12px/1.6 "Press Start 2P", monospace; color: var(--hi); margin: 2.5rem 0 1rem; }
h2::after { content: ""; display: block; margin-top: .6rem; border-top: 3px double var(--dim); }
ul, ol { list-style: none; padding: 0; margin: 0; }
li { display: flex; white-space: nowrap; gap: 0; }
ul li::before { content: "► "; color: var(--dim); white-space: pre; }
li:hover::before, li:hover .num, li:focus-within::before, li:focus-within .num { color: var(--hi); }
.keys { color: var(--dim); }
.num { color: var(--dim); white-space: pre; }
.dots { color: var(--dim); overflow: hidden; flex: 1; min-width: 1ch; padding: 0 .5ch; }
.dots::before { content: "................................................................................................................................................"; }
.n { color: var(--fg); white-space: pre; }
.meta { color: var(--dim); white-space: pre; padding-left: 1ch; }
.artist { color: var(--fg); }
a { color: var(--link); text-decoration: none; overflow: hidden; text-overflow: ellipsis; }
a:hover, a:focus-visible { background: var(--link); color: var(--bg); outline: none; }
a.m3u { color: var(--dim); margin-left: 1ch; flex: none; }
a.m3u:hover { color: var(--bg); background: var(--dim); }
.nav { color: var(--dim); margin: 0 0 1rem; }
.navrow { display: flex; justify-content: space-between; align-items: baseline; gap: 2ch; margin: 0 0 1rem; }
.navrow .nav, .navrow .lead { margin: 0; }
.lang { color: var(--dim); white-space: nowrap; }
.lang .cur { color: var(--hi); }
.lang .sep { white-space: pre; }
.lead { margin: 0 0 .5rem; }
footer { margin-top: 3rem; color: var(--dim); font-size: 13px; }
.blink { animation: blink 1s steps(1) infinite; }
@keyframes blink { 50% { opacity: 0; } }
li.tp-playing .num { color: var(--hi); }
li.tp-playing a { background: var(--link); color: var(--bg); }
"#;

/// Escapes text for HTML content and attribute values.
fn esc(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&#39;"),
            c => out.push(c),
        }
    }
    out
}

/// A script to inline: a closing script tag inside would end the script early.
fn inline_script(js: &str) -> Result<String> {
    if js.to_ascii_lowercase().contains("</script") {
        bail!("встраиваемый скрипт содержит закрывающий тег script");
    }
    Ok(format!("<script>{js}</script>"))
}

/// `MPEG-1 Layer III  192 kbps CBR  44.1 kHz  Stereo` — the player's stream line.
fn audio_line(a: &AudioInfo) -> String {
    let layer = match a.layer {
        1 => "I".to_owned(),
        2 => "II".to_owned(),
        3 => "III".to_owned(),
        n => n.to_string(),
    };
    let khz = f64::from(a.sample_rate_hz) / 1000.0;
    let mode = if a.vbr { "VBR" } else { "CBR" };
    format!(
        "{} Layer {layer}  {} kbps {mode}  {khz} kHz  {}",
        a.version, a.bitrate_kbps, a.channel_mode
    )
}

fn duration(secs: u64) -> String {
    format!("{}:{:02}:{:02}", secs / 3600, secs % 3600 / 60, secs % 60)
}

fn media_url(ep: &Episode) -> String {
    format!("/media/{}", encode_path(&ep.path))
}

/// A language of the site: where its pages live and every text they show.
pub struct Lang {
    /// `lang` attribute and the switcher label.
    pub code: &'static str,
    pub label: &'static str,
    /// URL prefix: "" for the main language, "/ru" etc. for the others.
    pub prefix: &'static str,
    all_playlist: &'static str,
    podcasts: &'static str,
    folders: &'static str,
    no_podcast: &'static str,
    back: &'static str,
    download: &'static str,
    keys: &'static str,
    gopher: &'static str,
    players: &'static str,
}

/// English is the main language (the site root); Ukrainian and Russian under /ua, /ru.
/// The order here is the order of the language switcher.
pub const LANGS: [Lang; 3] = [
    Lang {
        code: "en",
        label: "EN",
        prefix: "",
        all_playlist: "All episodes in one playlist [m3u]",
        podcasts: "PODCASTS",
        folders: "FOLDERS",
        no_podcast: "No podcast",
        back: "◄ all podcasts",
        download: "Download the playlist [m3u]",
        keys: "[↑↓] select  [Enter] open / play  [Backspace] back",
        gopher: "also over Gopher",
        players: "M3U playlists open in VLC, foobar2000, mpv",
    },
    Lang {
        code: "uk", // ISO 639-1 for Ukrainian (UA is the country)
        label: "UA",
        prefix: "/ua",
        all_playlist: "Усі епізоди одним плейлистом [m3u]",
        podcasts: "ПОДКАСТИ",
        folders: "ПАПКИ",
        no_podcast: "Без подкасту",
        back: "◄ усі подкасти",
        download: "Завантажити плейлист [m3u]",
        keys: "[↑↓] вибір  [Enter] відкрити / грати  [Backspace] назад",
        gopher: "також через Gopher",
        players: "плейлисти M3U відкриваються у VLC, foobar2000, mpv",
    },
    Lang {
        code: "ru",
        label: "RU",
        prefix: "/ru",
        all_playlist: "Все эпизоды одним плейлистом [m3u]",
        podcasts: "ПОДКАСТЫ",
        folders: "ПАПКИ",
        no_podcast: "Без подкаста",
        back: "◄ все подкасты",
        download: "Скачать плейлист [m3u]",
        keys: "[↑↓] выбор  [Enter] открыть / играть  [Backspace] назад",
        gopher: "также по Gopher",
        players: "плейлисты M3U открываются в VLC, foobar2000, mpv",
    },
];

/// Pages: path relative to the www root → contents.
pub type Files = Vec<(String, String)>;

/// Builds all pages in every language. `gopher` is shown in the footer.
pub fn pages(catalog: &Catalog, gopher: &Server) -> Result<Files> {
    let layout = Layout::new(gopher)?;
    let episodes: HashMap<&str, &Episode> = catalog
        .folders
        .iter()
        .flat_map(|p| &p.episodes)
        .map(|e| (e.path.as_str(), e))
        .collect();

    let mut files = Vec::new();
    for lang in &LANGS {
        // the directory of this language's pages: "" or "ru/"
        let dir = lang.prefix.trim_start_matches('/');
        let dir = if dir.is_empty() {
            String::new()
        } else {
            format!("{dir}/")
        };

        // Index.
        // The playlist of all episodes shares the row with the language switcher.
        let mut body = String::new();
        let _ = write!(
            body,
            r#"<div class="navrow"><p class="lead">♫ <a href="/{}" download>{}</a> <span class="meta">({})</span></p><!--nav--></div>"#,
            esc(&catalog.all_playlist),
            esc(lang.all_playlist),
            episodes.len()
        );
        let _ = write!(body, "<h2>{}</h2><ul>", esc(lang.podcasts));
        for p in &catalog.podcasts {
            let name = p.name.as_deref().unwrap_or(lang.no_podcast);
            let href = format!("{}/podcast/{}/", lang.prefix, p.slug);
            list_row(&mut body, &href, name, p.episodes.len(), &p.playlist);
        }
        let _ = write!(body, "</ul><h2>{}</h2><ul>", esc(lang.folders));
        for p in &catalog.folders {
            let href = format!("{}/dir/{}/", lang.prefix, p.slug);
            list_row(&mut body, &href, &p.title, p.episodes.len(), &p.playlist);
        }
        body.push_str("</ul>");
        body.push_str(&layout.keynav);
        body.push_str(r#"<script>KeyNav.attach({ items: "ul li > a:first-child" });</script>"#);
        files.push((
            format!("{dir}index.html"),
            layout.page(lang, "", SITE_TITLE, &body),
        ));

        // A page per podcast and per folder.
        for p in &catalog.podcasts {
            let name = p.name.as_deref().unwrap_or(lang.no_podcast);
            let eps: Vec<&Episode> = p
                .episodes
                .iter()
                .filter_map(|path| episodes.get(path.as_str()).copied())
                .collect();
            let path = format!("podcast/{}/", p.slug);
            let page = layout.episode_page(lang, &path, name, &p.playlist, &eps);
            files.push((format!("{dir}{path}index.html"), page));
        }
        for p in &catalog.folders {
            let eps: Vec<&Episode> = p.episodes.iter().collect();
            let path = format!("dir/{}/", p.slug);
            let page = layout.episode_page(lang, &path, &p.title, &p.playlist, &eps);
            files.push((format!("{dir}{path}index.html"), page));
        }
    }
    Ok(files)
}

/// A row of the index: link, dots, episode count, playlist.
fn list_row(out: &mut String, href: &str, name: &str, n: usize, playlist: &str) {
    let _ = write!(
        out,
        r#"<li><a href="{}">{}</a><span class="dots"></span><span class="n">{n:>3}</span><a class="m3u" href="/{}" download>[m3u]</a></li>"#,
        esc(href),
        esc(name),
        esc(playlist)
    );
}

/// The parts every page shares, whatever its language.
struct Layout {
    banner: String,
    gopher: String,
    textplayer: String,
    keynav: String,
}

impl Layout {
    fn new(gopher: &Server) -> Result<Self> {
        let mut banner = format!(r#"<h1 class="sr">{SITE_TITLE}</h1>"#);
        banner.push_str("\n<div class=\"banners\">\n");
        for text in ["KISS FM", "PODCASTS"] {
            let bm = banner::bitmap(text)?;
            let width = 2 * bm.first().map_or(0, Vec::len);
            let (fill, ol) = (banner::fill_layer(&bm), banner::outline_layer(&bm));
            let _ = writeln!(
                banner,
                r#"<div class="banner" style="--w:{width}"><pre class="ol ol2" aria-hidden="true">{ol}</pre><pre class="ol ol1" aria-hidden="true">{ol}</pre><pre class="fill" aria-hidden="true">{fill}</pre></div>"#
            );
        }
        banner.push_str("</div>\n");
        Ok(Self {
            banner,
            gopher: format!("gopher://{}:{}/", esc(&gopher.host), gopher.port),
            textplayer: inline_script(TEXTPLAYER_JS)?,
            keynav: inline_script(KEYNAV_JS)?,
        })
    }

    /// A whole page. `path` is the page within a language ("", "podcast/x/"): the
    /// language switcher and the hreflang links point to the same page in the others.
    fn page(&self, lang: &Lang, path: &str, title: &str, body: &str) -> String {
        let url = |l: &Lang| format!("{}/{path}", l.prefix);
        let mut alternates = String::new();
        let mut switch = String::from(r#"<nav class="lang" aria-label="Language">"#);
        for (i, l) in LANGS.iter().enumerate() {
            let _ = writeln!(
                alternates,
                r#"<link rel="alternate" hreflang="{}" href="{}">"#,
                l.code,
                esc(&url(l))
            );
            if i > 0 {
                switch.push_str(r#"<span class="sep"> | </span>"#);
            }
            if l.code == lang.code {
                let _ = write!(
                    switch,
                    r#"<span class="cur" aria-current="true">{}</span>"#,
                    l.label
                );
            } else {
                let _ = write!(
                    switch,
                    r#"<a href="{}" hreflang="{}" lang="{}">{}</a>"#,
                    esc(&url(l)),
                    l.code,
                    l.code,
                    l.label
                );
            }
        }
        switch.push_str("</nav>");
        // The nav row under the banner: "◄ all podcasts" (sub-pages) left, languages right.
        let body = body.replacen("<!--nav-->", &switch, 1);
        let footer = format!(
            "<footer><span class=\"keys\">{}</span><br>\n\
             ░▒▓ {}: {} ▓▒░<br>\n\
             &gt; {}<span class=\"blink\">█</span></footer>",
            esc(lang.keys),
            esc(lang.gopher),
            self.gopher,
            esc(lang.players)
        );
        format!(
            r#"<!doctype html>
<html lang="{code}">
<head>
<meta charset="utf-8">
<meta name="viewport" content="width=device-width, initial-scale=1">
<meta name="robots" content="noindex, nofollow, noarchive, nosnippet, noimageindex">
<title>{title}</title>
{alternates}<link rel="preconnect" href="https://fonts.googleapis.com">
<link rel="preconnect" href="https://fonts.gstatic.com" crossorigin>
<link rel="stylesheet" href="https://fonts.googleapis.com/css2?family=Press+Start+2P&family=JetBrains+Mono:wght@400;700&display=swap">
<link rel="icon" href="/favicon.ico" sizes="16x16 32x32 48x48">
<link rel="icon" type="image/png" href="/icon-192.png" sizes="192x192">
<link rel="apple-touch-icon" href="/apple-touch-icon.png">
<style>{CSS}</style>
</head>
<body>
<main>
{banner}{body}
{footer}
</main>
</body>
</html>
"#,
            code = lang.code,
            title = esc(title),
            banner = self.banner,
        )
    }

    /// A list of episodes played by the text-mode player.
    fn episode_page(
        &self,
        lang: &Lang,
        path: &str,
        title: &str,
        playlist: &str,
        episodes: &[&Episode],
    ) -> String {
        let width = episodes.len().to_string().len();
        let home = format!("{}/", lang.prefix);
        let mut body = format!(
            r#"<div class="navrow"><p class="nav"><a href="{}">{}</a></p><!--nav--></div><h2>{}</h2><p class="lead">♫ <a href="/{}" download>{}</a> <span class="meta">({})</span></p><ol class="episodes">"#,
            esc(&home),
            esc(lang.back),
            esc(&title.to_uppercase()),
            esc(playlist),
            esc(lang.download),
            episodes.len()
        );
        for (i, ep) in episodes.iter().enumerate() {
            let file = ep.path.rsplit('/').next().unwrap_or(&ep.path);
            let artist = ep
                .artist
                .as_deref()
                .map(|a| format!(r#"<span class="artist">{} — </span>"#, esc(a)))
                .unwrap_or_default();
            // Fixed-width columns with equal gaps: date (10), duration (7, up to
            // 9:59:59), bitrate (`192k`; for VBR the average), size (up to 999 MB).
            // A missing value keeps its column blank, so the columns line up.
            let meta = [
                format!(
                    "{:<10}",
                    ep.date.as_deref().map(crate::show_date).unwrap_or_default()
                ),
                format!("{:>7}", ep.duration_secs.map(duration).unwrap_or_default()),
                format!(
                    "{:>4}",
                    ep.audio
                        .as_ref()
                        .map(|a| format!("{}k", a.bitrate_kbps))
                        .unwrap_or_default()
                ),
                format!("{:>3} MB", (ep.size + 500_000) / 1_000_000),
            ];
            let _ = write!(
                body,
                r#"<li><span class="num">{num:0width$}. </span><a href="{href}" title="{path}" data-file="{file}" data-title="{t}" data-artist="{a}" data-podcast="{prog}" data-date="{date}" data-size="{size}" data-format="{fmt}">{artist}{t}</a><span class="dots"></span><span class="meta">{meta}</span></li>"#,
                num = i + 1,
                href = esc(&media_url(ep)),
                path = esc(&ep.path),
                file = esc(file),
                t = esc(&ep.title),
                a = esc(ep.artist.as_deref().unwrap_or("")),
                prog = esc(ep.podcast.as_deref().unwrap_or("")),
                date = esc(&ep.date.as_deref().map(crate::show_date).unwrap_or_default()),
                size = ep.size,
                fmt = esc(&ep.audio.as_ref().map(audio_line).unwrap_or_default()),
                meta = esc(&meta.join("   ")),
            );
        }
        body.push_str("</ol>");
        body.push_str(&self.textplayer);
        body.push_str(
            r#"<script>TextPlayer.attach({ links: "ol.episodes a[data-file]" });</script>"#,
        );
        body.push_str(&self.keynav);
        let _ = write!(
            body,
            r#"<script>KeyNav.attach({{ items: "ol.episodes a[data-file]", back: "{}" }});</script>"#,
            esc(&home)
        );
        self.page(lang, path, &format!("{title} — {SITE_TITLE}"), &body)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn languages_complete() {
        for l in &LANGS {
            for t in [
                l.all_playlist,
                l.podcasts,
                l.folders,
                l.no_podcast,
                l.back,
                l.download,
                l.keys,
                l.gopher,
                l.players,
            ] {
                assert!(!t.trim().is_empty(), "{}", l.code);
            }
        }
        assert_eq!(LANGS[0].prefix, "", "the main language is at the root");
    }

    #[test]
    fn escaping() {
        assert_eq!(
            esc(r#"<img src=x onerror="a('b')">&"#),
            "&lt;img src=x onerror=&quot;a(&#39;b&#39;)&quot;&gt;&amp;"
        );
    }

    #[test]
    fn scripts_can_be_inlined() {
        assert!(inline_script(TEXTPLAYER_JS).is_ok());
        assert!(inline_script(KEYNAV_JS).is_ok());
        assert!(inline_script("a</SCRIPT>b").is_err());
    }

    #[test]
    fn stream_line() {
        let a = AudioInfo {
            version: "MPEG-1".into(),
            layer: 3,
            bitrate_kbps: 192,
            vbr: false,
            sample_rate_hz: 44100,
            channel_mode: "Joint Stereo".into(),
        };
        assert_eq!(
            audio_line(&a),
            "MPEG-1 Layer III  192 kbps CBR  44.1 kHz  Joint Stereo"
        );
    }
}
