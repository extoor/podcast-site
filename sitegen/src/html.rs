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
const NO_PODCAST: &str = "Без подкаста";

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

/// Pages: path relative to the www root → contents.
pub type Files = Vec<(String, String)>;

/// Builds all pages. `gopher` is shown in the footer.
pub fn pages(catalog: &Catalog, gopher: &Server) -> Result<Files> {
    let layout = Layout::new(gopher)?;
    let episodes: HashMap<&str, &Episode> = catalog
        .folders
        .iter()
        .flat_map(|p| &p.episodes)
        .map(|e| (e.path.as_str(), e))
        .collect();

    let mut files = Vec::new();

    // Index.
    let mut body = String::new();
    let _ = write!(
        body,
        r#"<p class="lead">♫ <a href="/{}" download>Все эпизоды одним плейлистом [m3u]</a> <span class="meta">({})</span></p>"#,
        esc(&catalog.all_playlist),
        episodes.len()
    );
    body.push_str("<h2>ПОДКАСТЫ</h2><ul>");
    for p in &catalog.podcasts {
        let name = p.name.as_deref().unwrap_or(NO_PODCAST);
        list_row(
            &mut body,
            &format!("/podcast/{}/", p.slug),
            name,
            p.episodes.len(),
            &p.playlist,
        );
    }
    body.push_str("</ul><h2>ПАПКИ</h2><ul>");
    for p in &catalog.folders {
        list_row(
            &mut body,
            &format!("/dir/{}/", p.slug),
            &p.title,
            p.episodes.len(),
            &p.playlist,
        );
    }
    body.push_str("</ul>");
    body.push_str(&layout.keynav);
    body.push_str(r#"<script>KeyNav.attach({ items: "ul li > a:first-child" });</script>"#);
    files.push(("index.html".to_owned(), layout.page(SITE_TITLE, &body)));

    // A page per podcast (podcast) and per folder.
    for p in &catalog.podcasts {
        let name = p.name.as_deref().unwrap_or(NO_PODCAST);
        let eps: Vec<&Episode> = p
            .episodes
            .iter()
            .filter_map(|path| episodes.get(path.as_str()).copied())
            .collect();
        files.push((
            format!("podcast/{}/index.html", p.slug),
            layout.episode_page(name, &p.playlist, &eps),
        ));
    }
    for p in &catalog.folders {
        let eps: Vec<&Episode> = p.episodes.iter().collect();
        files.push((
            format!("dir/{}/index.html", p.slug),
            layout.episode_page(&p.title, &p.playlist, &eps),
        ));
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

/// The parts every page shares.
struct Layout {
    banner: String,
    footer: String,
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
        let footer = format!(
            "<footer><span class=\"keys\">[↑↓] выбор  [Enter] открыть / играть  [Backspace] назад</span><br>\n\
             ░▒▓ также по Gopher: gopher://{}:{}/ ▓▒░<br>\n\
             &gt; плейлисты M3U открываются в VLC, foobar2000, mpv<span class=\"blink\">█</span></footer>",
            esc(&gopher.host),
            gopher.port
        );
        Ok(Self {
            banner,
            footer,
            textplayer: inline_script(TEXTPLAYER_JS)?,
            keynav: inline_script(KEYNAV_JS)?,
        })
    }

    fn page(&self, title: &str, body: &str) -> String {
        format!(
            r#"<!doctype html>
<html lang="ru">
<head>
<meta charset="utf-8">
<meta name="viewport" content="width=device-width, initial-scale=1">
<meta name="robots" content="noindex, nofollow, noarchive, nosnippet, noimageindex">
<title>{title}</title>
<link rel="preconnect" href="https://fonts.googleapis.com">
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
            title = esc(title),
            banner = self.banner,
            footer = self.footer,
        )
    }

    /// A list of episodes played by the text-mode player.
    fn episode_page(&self, title: &str, playlist: &str, episodes: &[&Episode]) -> String {
        let width = episodes.len().to_string().len();
        let mut body = format!(
            r#"<p class="nav"><a href="/">◄ все подкасты</a></p><h2>{}</h2><p class="lead">♫ <a href="/{}" download>Скачать плейлист [m3u]</a> <span class="meta">({})</span></p><ol class="episodes">"#,
            esc(&title.to_uppercase()),
            esc(playlist),
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
        body.push_str(
            r#"<script>KeyNav.attach({ items: "ol.episodes a[data-file]", back: "/" });</script>"#,
        );
        self.page(&format!("{title} — {SITE_TITLE}"), &body)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
