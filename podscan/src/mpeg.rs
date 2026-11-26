//! MPEG audio stream info from the start of the file: version, layer, bitrate,
//! CBR/VBR, sample rate, channel mode and duration.
//!
//! Only the beginning of the file is read, in one pass: the ID3v2 tag (it is parsed
//! from the same bytes, see [`Head`]) and up to [`PROBE`] bytes after it. Duration comes from the Xing/Info or
//! VBRI header (frame count); without one, a CBR stream's duration is computed
//! from the file size.

use crate::catalog::AudioInfo;
use std::fs::File;
use std::io::{self, Read, Seek, SeekFrom};
use std::path::Path;

/// How many bytes after the ID3v2 tag are read to find the first frame.
const PROBE: u64 = 16 * 1024;

/// Largest ID3v2 tag that is read into memory. The size comes from the file itself
/// (up to 256 MiB), so a broken or crafted file must not make us read all of it.
/// Real tags, even with cover art, are far smaller.
pub const MAX_TAG: u64 = 16 * 1024 * 1024;

/// Stream info and duration in seconds (if it can be determined).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Probe {
    pub info: AudioInfo,
    pub duration_secs: Option<u64>,
}

/// The beginning of a file: the whole ID3v2 tag (if any) and [`PROBE`] bytes after it.
/// Read in one go, since every open and read over rclone is a network round trip.
pub struct Head {
    pub bytes: Vec<u8>,
    /// Where the tag ends and the audio starts within `bytes`.
    pub audio_start: usize,
    /// Offset of `bytes[audio_start]` in the file.
    audio_offset: u64,
    /// The tag is larger than [`MAX_TAG`]: it was skipped, `bytes` has only audio.
    pub tag_too_big: bool,
}

impl Head {
    pub fn read(path: &Path) -> io::Result<Self> {
        let mut f = File::open(path)?;
        let mut bytes = Vec::new();
        (&mut f).take(10).read_to_end(&mut bytes)?;
        let tag = bytes
            .as_slice()
            .try_into()
            .map_or(0, |h: &[u8; 10]| id3v2_size(h));
        if tag > MAX_TAG {
            bytes.clear();
            f.seek(SeekFrom::Start(tag))?;
            f.take(PROBE).read_to_end(&mut bytes)?;
            return Ok(Self {
                bytes,
                audio_start: 0,
                audio_offset: tag,
                tag_too_big: true,
            });
        }
        // tag ≤ MAX_TAG, so the sum and the cast cannot overflow
        f.take((tag + PROBE).saturating_sub(10))
            .read_to_end(&mut bytes)?;
        let audio_start = usize::try_from(tag).unwrap_or(usize::MAX).min(bytes.len());
        Ok(Self {
            bytes,
            audio_start,
            audio_offset: audio_start as u64,
            tag_too_big: false,
        })
    }

    /// Stream info; `None` if no MPEG frames were found.
    pub fn probe(&self, file_size: u64) -> Option<Probe> {
        parse(
            &self.bytes[self.audio_start..],
            self.audio_offset,
            file_size,
        )
    }
}

/// Size of the ID3v2 tag including its header (and footer), 0 if there is none.
fn id3v2_size(h: &[u8; 10]) -> u64 {
    if &h[..3] != b"ID3" || h[6..].iter().any(|b| b & 0x80 != 0) {
        return 0;
    }
    let size = h[6..]
        .iter()
        .fold(0u64, |acc, &b| (acc << 7) | u64::from(b));
    let footer = if h[5] & 0x10 != 0 { 10 } else { 0 };
    10 + size + footer
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Version {
    V1,
    V2,
    V25,
}

#[derive(Debug, Clone, Copy)]
struct Header {
    version: Version,
    layer: u8,
    bitrate_kbps: u32,
    sample_rate: u32,
    padding: u32,
    mono: bool,
    channel_mode: u8,
}

impl Header {
    fn parse(b: &[u8]) -> Option<Self> {
        let &[b0, b1, b2, b3] = b.get(..4)? else {
            return None;
        };
        if b0 != 0xFF || b1 & 0xE0 != 0xE0 {
            return None;
        }
        let version = match (b1 >> 3) & 3 {
            0 => Version::V25,
            2 => Version::V2,
            3 => Version::V1,
            _ => return None,
        };
        let layer = match (b1 >> 1) & 3 {
            1 => 3,
            2 => 2,
            3 => 1,
            _ => return None,
        };
        let table: &[u32; 15] = match (version, layer) {
            (Version::V1, 1) => &[
                0, 32, 64, 96, 128, 160, 192, 224, 256, 288, 320, 352, 384, 416, 448,
            ],
            (Version::V1, 2) => &[
                0, 32, 48, 56, 64, 80, 96, 112, 128, 160, 192, 224, 256, 320, 384,
            ],
            (Version::V1, _) => &[
                0, 32, 40, 48, 56, 64, 80, 96, 112, 128, 160, 192, 224, 256, 320,
            ],
            (_, 1) => &[
                0, 32, 48, 56, 64, 80, 96, 112, 128, 144, 160, 176, 192, 224, 256,
            ],
            _ => &[0, 8, 16, 24, 32, 40, 48, 56, 64, 80, 96, 112, 128, 144, 160],
        };
        // Index 0 is "free format", 15 is invalid: neither can be measured.
        let bitrate_kbps = *table.get(usize::from(b2 >> 4))?;
        if bitrate_kbps == 0 {
            return None;
        }
        let base = match (b2 >> 2) & 3 {
            0 => 44100,
            1 => 48000,
            2 => 32000,
            _ => return None,
        };
        let sample_rate = match version {
            Version::V1 => base,
            Version::V2 => base / 2,
            Version::V25 => base / 4,
        };
        let channel_mode = b3 >> 6;
        Some(Self {
            version,
            layer,
            bitrate_kbps,
            sample_rate,
            padding: u32::from((b2 >> 1) & 1),
            mono: channel_mode == 3,
            channel_mode,
        })
    }

    /// Frame length in bytes, including the header.
    fn frame_len(&self) -> usize {
        let br = self.bitrate_kbps * 1000;
        let len = match (self.layer, self.version) {
            (1, _) => (12 * br / self.sample_rate + self.padding) * 4,
            (3, Version::V2 | Version::V25) => 72 * br / self.sample_rate + self.padding,
            _ => 144 * br / self.sample_rate + self.padding,
        };
        len as usize
    }

    fn samples_per_frame(&self) -> u64 {
        match (self.layer, self.version) {
            (1, _) => 384,
            (3, Version::V2 | Version::V25) => 576,
            _ => 1152,
        }
    }

    /// Offset of the Xing/Info header from the frame start (after the side info).
    fn xing_offset(&self) -> usize {
        match (self.version, self.mono) {
            (Version::V1, false) => 36,
            (Version::V1, true) | (_, false) => 21,
            (_, true) => 13,
        }
    }

    /// Same stream: consecutive frames must agree on these fields.
    fn same_stream(&self, other: &Self) -> bool {
        self.version == other.version
            && self.layer == other.layer
            && self.sample_rate == other.sample_rate
    }
}

/// Finds the first frame confirmed by the next one, so that a stray `0xFF 0xE?`
/// inside leftover tag data is not taken for a frame.
fn first_frame(buf: &[u8]) -> Option<(usize, Header)> {
    (0..buf.len().saturating_sub(4)).find_map(|i| {
        let h = Header::parse(&buf[i..])?;
        let next = i + h.frame_len();
        match buf.get(next..).and_then(Header::parse) {
            Some(n) if h.same_stream(&n) => Some((i, h)),
            // The next frame is beyond the probe: trust the header.
            None if next + 4 > buf.len() => Some((i, h)),
            _ => None,
        }
    })
}

fn be32(b: &[u8], at: usize) -> Option<u32> {
    let s: [u8; 4] = b.get(at..at + 4)?.try_into().ok()?;
    Some(u32::from_be_bytes(s))
}

/// Parses `buf`, the bytes starting at `start` in a file of `file_size` bytes.
pub fn parse(buf: &[u8], start: u64, file_size: u64) -> Option<Probe> {
    let (pos, h) = first_frame(buf)?;
    let frame = &buf[pos..];
    let audio_bytes = file_size.saturating_sub(start.saturating_add(pos as u64));
    let audio_bits = audio_bytes.saturating_mul(8);
    let sr = u64::from(h.sample_rate);

    // Xing (VBR) / Info (CBR, written by LAME) or VBRI (Fraunhofer, always VBR).
    let xo = h.xing_offset();
    let (frames, vbr_tag) = match frame.get(xo..xo + 4) {
        Some(b"Xing") | Some(b"Info") => {
            let flags = be32(frame, xo + 4)?;
            let frames = (flags & 1 != 0).then(|| be32(frame, xo + 8)).flatten();
            (frames, Some(frame.get(xo..xo + 4) == Some(&b"Xing"[..])))
        }
        _ if frame.get(36..40) == Some(&b"VBRI"[..]) => (be32(frame, 36 + 14), Some(true)),
        _ => (None, None),
    };

    // Without a header, the stream is VBR if the bitrate changes between frames.
    let vbr = vbr_tag.unwrap_or_else(|| {
        let mut at = pos;
        let mut seen = 0;
        while let Some(n) = buf.get(at..).and_then(Header::parse) {
            if !n.same_stream(&h) || seen > 50 {
                break;
            }
            if n.bitrate_kbps != h.bitrate_kbps {
                return true;
            }
            at += n.frame_len();
            seen += 1;
        }
        false
    });

    let duration_secs = match frames {
        Some(n) if n > 0 => Some(u64::from(n) * h.samples_per_frame() / sr),
        _ if !vbr => Some(audio_bits / (u64::from(h.bitrate_kbps) * 1000)),
        _ => None,
    };
    // For VBR the header bitrate is that of the first frame: report the average.
    let bitrate_kbps = match duration_secs {
        Some(d) if vbr && d > 0 => u32::try_from(audio_bits / d / 1000).ok()?,
        _ => h.bitrate_kbps,
    };

    Some(Probe {
        info: AudioInfo {
            version: match h.version {
                Version::V1 => "MPEG-1",
                Version::V2 => "MPEG-2",
                Version::V25 => "MPEG-2.5",
            }
            .to_owned(),
            layer: h.layer,
            bitrate_kbps,
            vbr,
            sample_rate_hz: h.sample_rate,
            channel_mode: ["Stereo", "Joint Stereo", "Dual Channel", "Mono"]
                [usize::from(h.channel_mode)]
            .to_owned(),
        },
        duration_secs,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// MPEG-1 Layer III frame, joint stereo, 44.1 kHz, bitrate index `bri`.
    fn frame(bri: u8) -> Vec<u8> {
        let h = [0xFF, 0xFB, bri << 4, 0x40];
        let len = Header::parse(&h).unwrap().frame_len();
        let mut f = vec![0u8; len];
        f[..4].copy_from_slice(&h);
        f
    }

    #[test]
    fn cbr_without_header() {
        let buf: Vec<u8> = [frame(9), frame(9), frame(9)].concat(); // 128 kbps
        let size = 100 + 16_000_000;
        let p = parse(&buf, 100, size).unwrap();
        assert_eq!(p.info.version, "MPEG-1");
        assert_eq!(p.info.layer, 3);
        assert_eq!(p.info.bitrate_kbps, 128);
        assert!(!p.info.vbr);
        assert_eq!(p.info.sample_rate_hz, 44100);
        assert_eq!(p.info.channel_mode, "Joint Stereo");
        assert_eq!(p.duration_secs, Some(1000)); // 16 MB at 128 kbps
    }

    #[test]
    fn xing_vbr() {
        let mut first = frame(9);
        first[36..40].copy_from_slice(b"Xing");
        first[40..44].copy_from_slice(&1u32.to_be_bytes()); // frames field present
        first[44..48].copy_from_slice(&38_282u32.to_be_bytes()); // ≈ 1000 s
        let buf = [first, frame(11), frame(9)].concat();
        let p = parse(&buf, 0, 20_000_000).unwrap();
        assert!(p.info.vbr);
        assert_eq!(p.duration_secs, Some(1000));
        assert_eq!(p.info.bitrate_kbps, 160); // 20 MB over 1000 s
    }

    #[test]
    fn info_tag_is_cbr() {
        let mut first = frame(11);
        first[36..40].copy_from_slice(b"Info");
        first[40..44].copy_from_slice(&1u32.to_be_bytes());
        first[44..48].copy_from_slice(&38_282u32.to_be_bytes());
        let p = parse(&[first, frame(11)].concat(), 0, 20_000_000).unwrap();
        assert!(!p.info.vbr);
        assert_eq!(p.info.bitrate_kbps, 192);
        assert_eq!(p.duration_secs, Some(1000));
    }

    #[test]
    fn varying_bitrate_without_header_is_vbr() {
        let buf = [frame(9), frame(11), frame(9)].concat();
        let p = parse(&buf, 0, 1_000_000).unwrap();
        assert!(p.info.vbr);
        assert_eq!(p.duration_secs, None);
    }

    #[test]
    fn skips_junk_and_false_sync() {
        // 0xFF 0xFB inside junk is not confirmed by a following frame.
        let buf = [vec![0, 0xFF, 0xFB, 0x90, 0x40, 1, 2, 3], frame(9), frame(9)].concat();
        let p = parse(&buf, 0, 1_000_000).unwrap();
        assert_eq!(p.info.bitrate_kbps, 128);
    }

    #[test]
    fn garbage_is_none() {
        assert_eq!(parse(&[0u8; 1000], 0, 1000), None);
        assert_eq!(parse(&[], 0, 0), None);
    }

    /// Writes `bytes` to a temporary file and reads its head.
    fn head_of(name: &str, bytes: &[u8]) -> Head {
        let path = std::env::temp_dir().join(format!("podscan-{}-{name}", std::process::id()));
        std::fs::write(&path, bytes).unwrap();
        let head = Head::read(&path).unwrap();
        std::fs::remove_file(&path).unwrap();
        head
    }

    #[test]
    fn head_reads_tag_and_audio() {
        // 20-byte tag body, then two frames
        let file = [
            b"ID3\x04\x00\x00\x00\x00\x00\x14".as_slice(),
            &[0; 20],
            &frame(9),
            &frame(9),
        ]
        .concat();
        let head = head_of("small", &file);
        assert!(!head.tag_too_big);
        assert_eq!(head.audio_start, 30);
        let p = head.probe(file.len() as u64).unwrap();
        assert_eq!(p.info.bitrate_kbps, 128);
    }

    #[test]
    fn huge_declared_tag_is_not_read() {
        // The header claims a ~200 MiB tag (syncsafe 0x0C 0x00 0x00 0x00), the file is tiny.
        let file = [b"ID3\x04\x00\x00\x0C\x00\x00\x00".as_slice(), &[0; 2000]].concat();
        let head = head_of("huge", &file);
        assert!(head.tag_too_big);
        assert!(head.bytes.is_empty()); // nothing past the end of the file
        assert_eq!(head.probe(file.len() as u64), None);
    }

    #[test]
    fn id3_size() {
        let mut h = *b"ID3\x04\x00\x00\x00\x00\x02\x01";
        assert_eq!(id3v2_size(&h), 10 + 257);
        h[5] = 0x10; // footer present
        assert_eq!(id3v2_size(&h), 10 + 257 + 10);
        assert_eq!(id3v2_size(b"\xFF\xFB\x90\x40\0\0\0\0\0\0"), 0);
    }
}
