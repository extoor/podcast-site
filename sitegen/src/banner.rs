//! The site banner («KISS FM / PODCASTS»): a chunky pixel font drawn with text
//! characters only. The same bitmap is used for Gopher (plain) and HTML (colored).

use anyhow::{Result, bail};

/// Glyph height in pixels.
const H: usize = 8;

/// Glyphs: `#` is a lit pixel. Widths vary; letters are separated by one column.
#[rustfmt::skip]
fn glyph(c: char) -> Option<[&'static str; H]> {
    Some(match c {
        'A' => [".#####.", "##...##", "##...##", "##...##", "#######", "##...##", "##...##", "##...##"],
        'C' => [".#####.", "##...##", "##.....", "##.....", "##.....", "##.....", "##...##", ".#####."],
        'D' => ["######.", "##...##", "##...##", "##...##", "##...##", "##...##", "##...##", "######."],
        'F' => ["#######", "##.....", "##.....", "######.", "##.....", "##.....", "##.....", "##....."],
        'I' => ["######",  "..##..",  "..##..",  "..##..",  "..##..",  "..##..",  "..##..",  "######"],
        'K' => ["##...##", "##..##.", "##.##..", "####...", "####...", "##.##..", "##..##.", "##...##"],
        'M' => ["##...##", "###.###", "#######", "##.#.##", "##...##", "##...##", "##...##", "##...##"],
        'O' => [".#####.", "##...##", "##...##", "##...##", "##...##", "##...##", "##...##", ".#####."],
        'P' => ["######.", "##...##", "##...##", "##...##", "######.", "##.....", "##.....", "##....."],
        'S' => [".#####.", "##...##", "##.....", ".#####.", ".....##", ".....##", "##...##", ".#####."],
        'T' => ["######",  "..##..",  "..##..",  "..##..",  "..##..",  "..##..",  "..##..",  "..##.."],
        ' ' => ["....",    "....",    "....",    "....",    "....",    "....",    "....",    "...."],
        _ => return None,
    })
}

/// Pixel rows of `text`, `true` = lit.
pub fn bitmap(text: &str) -> Result<Vec<Vec<bool>>> {
    let mut rows = vec![Vec::new(); H];
    for (i, c) in text.chars().enumerate() {
        let Some(g) = glyph(c) else {
            bail!("в шрифте баннера нет символа {c:?}");
        };
        for (row, line) in rows.iter_mut().zip(g) {
            if i > 0 {
                row.push(false);
            }
            row.extend(line.bytes().map(|b| b == b'#'));
        }
    }
    Ok(rows)
}

/// Text for a plain-text medium (Gopher): one character per pixel column and
/// half-block characters (`▀ ▄ █`) for two pixel rows per line, so pixels come out
/// roughly square in a terminal cell. Trailing spaces are trimmed.
pub fn half_blocks(bm: &[Vec<bool>]) -> Vec<String> {
    bm.chunks(2)
        .map(|pair| {
            let (top, bottom) = (&pair[0], pair.get(1));
            let line: String = top
                .iter()
                .enumerate()
                .map(|(x, &t)| {
                    let b = bottom.is_some_and(|r| r[x]);
                    match (t, b) {
                        (true, true) => '█',
                        (true, false) => '▀',
                        (false, true) => '▄',
                        (false, false) => ' ',
                    }
                })
                .collect();
            line.trim_end().to_owned()
        })
        .collect()
}

/// HTML fill layer: every pixel is two characters (`██`), which makes it roughly
/// square in a monospace cell. Trailing spaces are trimmed.
pub fn fill_layer(bm: &[Vec<bool>]) -> String {
    bm.iter()
        .map(|row| {
            let line: String = row.iter().map(|&p| if p { "██" } else { "  " }).collect();
            line.trim_end().to_owned()
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// HTML outline layer: the contour of the glyphs drawn with box-drawing characters on
/// the grid of pixel corners (one line per corner row, two columns per pixel). The page
/// shifts it by half a cell so the lines fall on the pixel edges.
pub fn outline_layer(bm: &[Vec<bool>]) -> String {
    let h = bm.len();
    let w = bm.first().map_or(0, Vec::len);
    let px = |x: usize, y: usize| bm.get(y).and_then(|r| r.get(x)).copied().unwrap_or(false);
    // The pixel left of / above a corner, if any.
    let left = |x: usize, y: usize| x.checked_sub(1).is_some_and(|x| px(x, y));
    let up = |x: usize, y: usize| y.checked_sub(1).is_some_and(|y| px(x, y));
    // Edge (cx,cy)–(cx+1,cy) and edge (cx,cy)–(cx,cy+1) of the contour.
    let hor = |cx: usize, cy: usize| up(cx, cy) != px(cx, cy);
    let ver = |cx: usize, cy: usize| left(cx, cy) != px(cx, cy);
    let mut lines = Vec::with_capacity(h + 1);
    for cy in 0..=h {
        let mut line = String::new();
        for cx in 0..=w {
            let u = cy > 0 && ver(cx, cy - 1);
            let d = cy < h && ver(cx, cy);
            let l = cx > 0 && hor(cx - 1, cy);
            let r = cx < w && hor(cx, cy);
            line.push(box_char(u, d, l, r));
            if cx < w {
                line.push(if r { '─' } else { ' ' });
            }
        }
        lines.push(line.trim_end().to_owned());
    }
    lines.join("\n")
}

/// Box-drawing character with lines going up, down, left, right.
fn box_char(u: bool, d: bool, l: bool, r: bool) -> char {
    match (u, d, l, r) {
        (false, false, false, false) => ' ',
        (true, true, false, false) => '│',
        (false, false, true, true) => '─',
        (false, true, false, true) => '┌',
        (false, true, true, false) => '┐',
        (true, false, false, true) => '└',
        (true, false, true, false) => '┘',
        (true, true, false, true) => '├',
        (true, true, true, false) => '┤',
        (false, true, true, true) => '┬',
        (true, false, true, true) => '┴',
        (true, true, true, true) => '┼',
        (true, false, false, false) => '╵',
        (false, true, false, false) => '╷',
        (false, false, true, false) => '╴',
        (false, false, false, true) => '╶',
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn widths() {
        // PODCASTS: 7 letters of 7 columns + T of 6 + 7 gaps
        assert_eq!(bitmap("PODCASTS").unwrap()[0].len(), 62);
        assert!(bitmap("PODCASTS").unwrap().iter().all(|r| r.len() == 62));
        assert!(bitmap("kiss").is_err());
    }

    #[test]
    fn html_layers() {
        let bm = bitmap("I").unwrap();
        let fill = fill_layer(&bm);
        assert_eq!(fill.lines().next(), Some("████████████"));
        assert_eq!(fill.lines().nth(1), Some("    ████"));
        // the outline of a 6x8 "I": 9 corner rows, closed contour
        let ol = outline_layer(&bm);
        assert_eq!(ol.lines().count(), 9);
        assert_eq!(ol.lines().next(), Some("┌───────────┐"));
        assert_eq!(ol.lines().nth(1), Some("└───┐   ┌───┘"));
        assert_eq!(ol.lines().last(), Some("└───────────┘"));
    }

    #[test]
    fn half_block_rows() {
        let lines = half_blocks(&bitmap("I").unwrap());
        assert_eq!(lines, ["▀▀██▀▀", "  ██", "  ██", "▄▄██▄▄"]);
    }
}
