//! Site icons: the letters "FM" in the pixel font of the banner, rainbow colors,
//! a dark offset copy for depth. PNG and ICO are written here directly (stored
//! deflate blocks, no compression), so no image crate is needed.

use crate::banner;
use anyhow::Result;

/// The banner gradient (see the `--rainbow` CSS variable), left to right.
const RAINBOW: [[u8; 3]; 8] = [
    [0xff, 0x3b, 0x30],
    [0xff, 0x9a, 0x3c],
    [0xff, 0xf3, 0x5c],
    [0x8d, 0xff, 0x5c],
    [0x5c, 0xff, 0xc8],
    [0x5c, 0xd8, 0xff],
    [0x3b, 0x6b, 0xff],
    [0x1a, 0x2c, 0xff],
];

/// Color of the gradient at `t` (0..=1).
fn rainbow(t: f32) -> [u8; 3] {
    let t = t.clamp(0.0, 1.0) * (RAINBOW.len() - 1) as f32;
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)] // 0..=7
    let i = (t.floor() as usize).min(RAINBOW.len() - 2);
    let f = t - i as f32;
    let (a, b) = (RAINBOW[i], RAINBOW[i + 1]);
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)] // stays in 0..=255
    std::array::from_fn(|k| {
        (f32::from(a[k]) + (f32::from(b[k]) - f32::from(a[k])) * f).round() as u8
    })
}

/// RGBA pixels of a `size`×`size` icon: black background, the letters scaled by a
/// whole number (pixels stay sharp), centered, with a shadow from 32 px up.
pub fn render(size: usize) -> Result<Vec<u8>> {
    let bm = banner::bitmap("FM")?;
    let (gw, gh) = (bm[0].len(), bm.len()); // 15 × 8
    let mut px = vec![0u8; size * size * 4];
    for p in px.chunks_mut(4) {
        p.copy_from_slice(&[0, 0, 0, 255]);
    }
    // Pixels are square on screen, but the font is meant for 2:1 text cells: draw
    // each glyph pixel twice as high as wide, as the page does (`██` per pixel).
    // Touch icons get rounded corners: keep a margin there.
    let room = if size >= 128 {
        size - 2 * (size / 10)
    } else {
        size
    };
    let scale = (room / gw).min(room / (gh * 2)).max(1);
    let (w, h) = (gw * scale, gh * 2 * scale);
    let shadow = if size >= 32 { (scale / 2).max(1) } else { 0 };
    let x0 = size.saturating_sub(w + shadow) / 2;
    let y0 = size.saturating_sub(h + shadow) / 2;
    let mut put = |x: usize, y: usize, c: [u8; 3]| {
        if x < size && y < size {
            let i = (y * size + x) * 4;
            px[i..i + 3].copy_from_slice(&c);
        }
    };
    for pass in [true, false] {
        let off = if pass { shadow } else { 0 };
        if pass && shadow == 0 {
            continue;
        }
        for (gy, row) in bm.iter().enumerate() {
            for (gx, &on) in row.iter().enumerate() {
                if !on {
                    continue;
                }
                for dy in 0..2 * scale {
                    for dx in 0..scale {
                        let (x, y) = (x0 + gx * scale + dx, y0 + gy * 2 * scale + dy);
                        #[allow(clippy::cast_precision_loss)] // small sizes
                        let c = rainbow((x - x0) as f32 / w.max(2).saturating_sub(1) as f32);
                        // the shadow: the same color, dimmed, offset down-right
                        let c = if pass { c.map(|v| v / 3) } else { c };
                        put(x + off, y + off, c);
                    }
                }
            }
        }
    }
    Ok(px)
}

// --- PNG -------------------------------------------------------------------------

fn crc32(data: &[u8]) -> u32 {
    let mut crc = 0xFFFF_FFFFu32;
    for &b in data {
        crc ^= u32::from(b);
        for _ in 0..8 {
            crc = if crc & 1 != 0 {
                (crc >> 1) ^ 0xEDB8_8320
            } else {
                crc >> 1
            };
        }
    }
    !crc
}

fn adler32(data: &[u8]) -> u32 {
    let (mut a, mut b) = (1u32, 0u32);
    for &x in data {
        a = (a + u32::from(x)) % 65521;
        b = (b + a) % 65521;
    }
    (b << 16) | a
}

fn chunk(out: &mut Vec<u8>, kind: &[u8; 4], data: &[u8]) {
    let len = u32::try_from(data.len()).unwrap_or(u32::MAX);
    out.extend_from_slice(&len.to_be_bytes());
    let start = out.len();
    out.extend_from_slice(kind);
    out.extend_from_slice(data);
    let crc = crc32(&out[start..]);
    out.extend_from_slice(&crc.to_be_bytes());
}

/// A PNG of `size`×`size` RGBA pixels.
pub fn png(size: usize, rgba: &[u8]) -> Vec<u8> {
    let side = u32::try_from(size).unwrap_or(u32::MAX);
    let mut ihdr = Vec::with_capacity(13);
    ihdr.extend_from_slice(&side.to_be_bytes());
    ihdr.extend_from_slice(&side.to_be_bytes());
    ihdr.extend_from_slice(&[8, 6, 0, 0, 0]); // 8 bit, RGBA, deflate, no filter, no interlace

    // scanlines, each with filter type 0
    let mut raw = Vec::with_capacity(size * (size * 4 + 1));
    for row in rgba.chunks(size * 4) {
        raw.push(0);
        raw.extend_from_slice(row);
    }
    // zlib stream of stored (uncompressed) deflate blocks
    let mut z = vec![0x78, 0x01];
    let blocks: Vec<&[u8]> = raw.chunks(65_535).collect();
    for (i, block) in blocks.iter().enumerate() {
        z.push(u8::from(i + 1 == blocks.len()));
        let n = u16::try_from(block.len()).unwrap_or(u16::MAX);
        z.extend_from_slice(&n.to_le_bytes());
        z.extend_from_slice(&(!n).to_le_bytes());
        z.extend_from_slice(block);
    }
    z.extend_from_slice(&adler32(&raw).to_be_bytes());

    let mut out = b"\x89PNG\r\n\x1a\n".to_vec();
    chunk(&mut out, b"IHDR", &ihdr);
    chunk(&mut out, b"IDAT", &z);
    chunk(&mut out, b"IEND", &[]);
    out
}

/// An ICO holding PNG images (allowed since Windows Vista, read by all browsers).
pub fn ico(images: &[(usize, Vec<u8>)]) -> Vec<u8> {
    let count = u16::try_from(images.len()).unwrap_or(u16::MAX);
    let mut out = Vec::new();
    out.extend_from_slice(&[0, 0, 1, 0]);
    out.extend_from_slice(&count.to_le_bytes());
    let mut offset = 6 + 16 * images.len();
    for (size, data) in images {
        let side = u8::try_from(*size).unwrap_or(0); // 0 means 256
        out.extend_from_slice(&[side, side, 0, 0, 1, 0, 32, 0]);
        out.extend_from_slice(&u32::try_from(data.len()).unwrap_or(u32::MAX).to_le_bytes());
        out.extend_from_slice(&u32::try_from(offset).unwrap_or(u32::MAX).to_le_bytes());
        offset += data.len();
    }
    for (_, data) in images {
        out.extend_from_slice(data);
    }
    out
}

/// All icon files: path relative to the www root → contents.
pub fn files() -> Result<Vec<(String, Vec<u8>)>> {
    let mut ico_images = Vec::new();
    for size in [16, 32, 48] {
        ico_images.push((size, png(size, &render(size)?)));
    }
    Ok(vec![
        ("favicon.ico".to_owned(), ico(&ico_images)),
        ("icon-192.png".to_owned(), png(192, &render(192)?)),
        ("apple-touch-icon.png".to_owned(), png(180, &render(180)?)),
    ])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn checksums() {
        assert_eq!(crc32(b"IEND"), 0xAE42_6082);
        assert_eq!(adler32(b"Wikipedia"), 0x11E6_0398);
    }

    #[test]
    fn png_and_ico_headers() {
        let p = png(16, &render(16).unwrap());
        assert!(p.starts_with(b"\x89PNG\r\n\x1a\n"));
        assert!(p.ends_with(&[0xAE, 0x42, 0x60, 0x82])); // IEND crc
        let i = ico(&[(16, p.clone())]);
        assert_eq!(&i[..6], &[0, 0, 1, 0, 1, 0]);
        assert_eq!(&i[22..], &p[..]);
    }

    #[test]
    fn letters_fill_16px() {
        // 15 × 8 pixels; at 16 px the glyphs are 1 wide and 2 high: 15 × 16
        let px = render(16).unwrap();
        let lit = px.chunks(4).filter(|p| p[..3] != [0, 0, 0]).count();
        let font = banner::bitmap("FM").unwrap();
        let on = font.iter().flatten().filter(|&&b| b).count();
        assert_eq!(lit, on * 2);
    }
}
