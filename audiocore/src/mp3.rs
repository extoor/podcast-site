//! Streaming, gapless MP3 decoding: bytes go in as they arrive, interleaved samples come
//! out. Decoding is lazy: `read` decodes only what it hands out, so a stream holds its
//! compressed bytes, never an hour of PCM.
//!
//! Trimming matches `nanomp3::decode_all` (minimp3_ex): with a Xing/Info tag carrying the
//! LAME/Lavc extension, the encoder delay (plus the decoder's own 529 samples) is cut
//! from the start and the padding from the end.

use nanomp3::{Decoder, MAX_SAMPLES_PER_FRAME, VbrTag};
use std::collections::VecDeque;

/// Bytes kept ahead of the decoder: a frame is only recognised once the frames after it
/// are seen (minimp3's rule), so it must not run dry before the end of the stream.
const LOOKAHEAD: usize = 16 * 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Phase {
    /// Checking for an ID3v2 tag (0), or skipping one: this many bytes still to go.
    Tag(usize),
    /// Looking for the first frame (and its VBR tag).
    Start,
    Decode,
}

/// One MP3 stream.
pub struct Mp3Stream {
    dec: Decoder,
    phase: Phase,
    /// Input not decoded yet, from `pos`.
    input: Vec<u8>,
    pos: usize,
    /// Bytes of the file dropped from the front of `input` so far.
    dropped: u64,
    eof: bool,
    pcm: Vec<f32>,
    /// Decoded interleaved samples not read yet.
    out: VecDeque<f32>,
    /// Interleaved samples still to drop at the start (the encoder delay).
    to_skip: u64,
    /// Interleaved samples the stream gives in all, when its tag says so.
    total: Option<u64>,
    /// End padding from the tag, samples per channel (to pass on to a stream after a seek).
    padding: u64,
    /// Samples per channel held back and dropped at the end (`trim_end`).
    hold: u64,
    /// Interleaved samples produced so far, after `to_skip`.
    produced: u64,
    /// File offset of the first audio frame (after tags and the Xing/Info frame).
    audio_start: Option<u64>,
    channels: u8,
    sample_rate: u32,
    bitrate: u32,
}

impl Default for Mp3Stream {
    fn default() -> Self {
        Self::new()
    }
}

impl Mp3Stream {
    pub fn new() -> Self {
        Self {
            dec: Decoder::new(),
            phase: Phase::Tag(0),
            input: Vec::new(),
            pos: 0,
            dropped: 0,
            eof: false,
            pcm: vec![0.0; MAX_SAMPLES_PER_FRAME],
            out: VecDeque::new(),
            to_skip: 0,
            total: None,
            padding: 0,
            hold: 0,
            produced: 0,
            audio_start: None,
            channels: 0,
            sample_rate: 0,
            bitrate: 0,
        }
    }

    /// Feeds the next bytes of the file. Nothing is decoded until `read`.
    pub fn push(&mut self, bytes: &[u8]) {
        // drop what has been decoded, so the buffer stays small
        if self.pos > 0 && self.pos >= self.input.len() / 2 {
            self.input.drain(..self.pos);
            self.dropped += self.pos as u64;
            self.pos = 0;
        }
        self.input.extend_from_slice(bytes);
    }

    /// No more bytes will come. Trailing tags (ID3v1, APEv2) are cut off: minimp3 only
    /// accepts a frame followed by another frame or by the end of the data, so a tag
    /// after the last frame would lose it (the lookahead keeps the tail undecoded).
    pub fn finish(&mut self) {
        if !self.eof {
            let audio = nanomp3::strip_trailing_tags(self.pending()).len();
            self.input.truncate(self.pos + audio);
        }
        self.eof = true;
    }

    /// Drops the last `frames` samples per channel. A stream started mid-file (a seek)
    /// has no tag: given the padding of the original (`end_padding`), its end is cut
    /// exactly as the original's, whatever frame it started at. The samples are held
    /// back until the stream ends.
    pub fn trim_end(&mut self, frames: u64) {
        self.hold = frames;
    }

    /// The end padding the tag asks to cut, samples per channel (0 without one).
    pub fn end_padding(&self) -> u64 {
        self.padding
    }

    /// Channels (1 or 2) and sample rate, once the first frame is decoded.
    pub fn format(&self) -> Option<(u8, u32)> {
        (self.channels > 0).then_some((self.channels, self.sample_rate))
    }

    /// Bitrate of the first audio frame, kbps (0 until decoded, or free format).
    pub fn bitrate(&self) -> u32 {
        self.bitrate
    }

    /// File offset of the first audio frame, once found.
    pub fn audio_start(&self) -> Option<u64> {
        self.audio_start
    }

    /// Samples per channel of the whole stream after trimming, if its tag says so.
    pub fn total_frames(&self) -> Option<u64> {
        let ch = u64::from(self.channels.max(1));
        self.total.map(|t| t / ch)
    }

    /// Bytes received and not decoded yet.
    pub fn pending_bytes(&self) -> usize {
        self.input.len() - self.pos
    }

    /// True once everything has been decoded and read.
    pub fn ended(&self) -> bool {
        self.eof && self.out.is_empty() && !self.can_decode()
    }

    /// Decodes as needed and moves up to `dst.len()` interleaved samples into `dst`;
    /// returns how many. Fewer than asked (even 0) means the bytes have run out for now:
    /// a frame is decoded only with enough bytes after it, unless the stream is finished.
    pub fn read(&mut self, dst: &mut [f32]) -> usize {
        let held = |s: &Self| (s.hold * u64::from(s.channels)) as usize;
        while self.out.len() < dst.len() + held(self) && self.step() {}
        let held = held(self);
        if held > 0 && self.eof && !self.can_decode() {
            // the end: the held-back samples are the padding
            let keep = self.out.len().saturating_sub(held);
            self.out.truncate(keep);
            self.hold = 0;
        }
        let n = dst.len().min(
            self.out
                .len()
                .saturating_sub((self.hold * u64::from(self.channels)) as usize),
        );
        for (d, s) in dst.iter_mut().zip(self.out.drain(..n)) {
            *d = s;
        }
        n
    }

    fn pending(&self) -> &[u8] {
        &self.input[self.pos..]
    }

    fn can_decode(&self) -> bool {
        let done = self.total.is_some_and(|t| self.produced >= t);
        !done && (self.pending().len() >= LOOKAHEAD || (self.eof && self.pos < self.input.len()))
    }

    /// One step of work: a tag, the first frame or one decoded frame. False when nothing
    /// can be done until more bytes come (or ever, at the end).
    fn step(&mut self) -> bool {
        match self.phase {
            Phase::Tag(0) => {
                // an ID3v2 tag is recognised from its 10-byte header
                if self.pending().len() < 10 && !self.eof {
                    return false;
                }
                let len = nanomp3::id3v2_len(self.pending());
                self.phase = if len > 0 {
                    Phase::Tag(len)
                } else {
                    Phase::Start
                };
                true
            }
            Phase::Tag(left) => {
                let n = left.min(self.pending().len());
                self.pos += n;
                if n < left && !self.eof {
                    self.phase = Phase::Tag(left - n);
                    return n > 0;
                }
                self.phase = Phase::Start;
                true
            }
            Phase::Start => {
                if self.pending().len() < LOOKAHEAD && !self.eof {
                    return false;
                }
                self.start();
                self.audio_start = Some(self.dropped + self.pos as u64);
                self.phase = Phase::Decode;
                true
            }
            Phase::Decode => self.decode_frame(),
        }
    }

    /// The first frame: skip junk before it, read its Xing/Info tag (the frame is then
    /// not audio and is skipped too).
    fn start(&mut self) {
        let Some(frame) = nanomp3::frames(self.pending()).next() else {
            return;
        };
        let (offset, len) = (frame.offset, frame.data.len());
        if frame.layer == 3 {
            let ch = u64::from(frame.channels.num());
            let tag = VbrTag::parse(frame.data);
            if let Some(VbrTag {
                frames: Some(frames),
                encoder_delay_padding,
            }) = tag
            {
                // as VbrTag::delay_padding: the decoder adds 529 samples of its own
                let (delay, padding) = encoder_delay_padding
                    .map_or((0, 0), |(d, p)| (i64::from(d) + 529, i64::from(p) - 529));
                let skip = delay.max(0) as u64 * ch;
                let mut total = frame.samples as u64 * ch * u64::from(frames);
                total = total.saturating_sub(skip);
                if padding > 0 {
                    total = total.saturating_sub(padding as u64 * ch);
                }
                self.to_skip = skip;
                self.total = Some(total);
                self.padding = padding.max(0) as u64;
            }
            if tag.is_some() {
                self.pos += offset + len;
                return;
            }
        }
        self.pos += offset;
    }

    /// Decodes the next frame into `out`. False when there is nothing to decode for now
    /// (too few bytes ahead) or at all.
    fn decode_frame(&mut self) -> bool {
        if !self.can_decode() {
            if self.total.is_some_and(|t| self.produced >= t) {
                // the padding and anything after it (trailing tags) is not audio
                self.pos = self.input.len();
            }
            return false;
        }
        let (used, result) = self.dec.decode(&self.input[self.pos..], &mut self.pcm);
        self.pos += used;
        let Ok(info) = result else {
            return used > 0;
        };
        self.channels = info.channels.num();
        self.sample_rate = info.sample_rate;
        if self.bitrate == 0 {
            self.bitrate = info.bitrate;
        }
        let mut samples = &self.pcm[..info.samples_produced * usize::from(self.channels)];
        let skip = (self.to_skip as usize).min(samples.len());
        samples = &samples[skip..];
        self.to_skip -= skip as u64;
        if let Some(total) = self.total {
            let left = (total - self.produced) as usize;
            samples = &samples[..samples.len().min(left)];
        }
        self.produced += samples.len() as u64;
        self.out.extend(samples);
        true
    }
}
