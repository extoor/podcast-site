//! The WebAssembly interface, for the AudioWorklet (`web/textplayer`). Plain exported
//! functions, no generated glue: streams are numbered handles, bytes and samples pass
//! through two buffers in the module's memory whose addresses JS asks for before each
//! use (they move when they grow).
//!
//! Output is always stereo, interleaved f32: mono is doubled.

// Exports need #[unsafe(no_mangle)]; nothing else here is unsafe.
#![allow(unsafe_code)]

use crate::mp3::Mp3Stream;
use std::sync::Mutex;

struct State {
    streams: Vec<Option<Mp3Stream>>,
    input: Vec<u8>,
    output: Vec<f32>,
    scratch: Vec<f32>,
}

static STATE: Mutex<State> = Mutex::new(State {
    streams: Vec::new(),
    input: Vec::new(),
    output: Vec::new(),
    scratch: Vec::new(),
});

fn with<R>(f: impl FnOnce(&mut State) -> R) -> R {
    f(&mut STATE.lock().unwrap_or_else(|e| e.into_inner()))
}

fn stream<R>(h: u32, default: R, f: impl FnOnce(&mut Mp3Stream) -> R) -> R {
    with(|st| match st.streams.get_mut(h as usize) {
        Some(Some(s)) => f(s),
        _ => default,
    })
}

/// A new MP3 stream; returns its handle.
#[unsafe(no_mangle)]
pub extern "C" fn mp3_new() -> u32 {
    with(|st| {
        let s = Some(Mp3Stream::new());
        match st.streams.iter().position(Option::is_none) {
            Some(i) => {
                st.streams[i] = s;
                i as u32
            }
            None => {
                st.streams.push(s);
                (st.streams.len() - 1) as u32
            }
        }
    })
}

#[unsafe(no_mangle)]
pub extern "C" fn mp3_free(h: u32) {
    with(|st| {
        if let Some(slot) = st.streams.get_mut(h as usize) {
            *slot = None;
        }
    });
}

/// Address of a buffer for `len` input bytes: write them there, then call `mp3_push`.
#[unsafe(no_mangle)]
pub extern "C" fn input_buffer(len: u32) -> *mut u8 {
    with(|st| {
        st.input.resize(len as usize, 0);
        st.input.as_mut_ptr()
    })
}

/// Feeds the first `len` bytes of the input buffer to stream `h`.
#[unsafe(no_mangle)]
pub extern "C" fn mp3_push(h: u32, len: u32) {
    with(|st| {
        let State { streams, input, .. } = st;
        if let Some(Some(s)) = streams.get_mut(h as usize) {
            s.push(&input[..(len as usize).min(input.len())]);
        }
    });
}

/// The file is complete.
#[unsafe(no_mangle)]
pub extern "C" fn mp3_finish(h: u32) {
    stream(h, (), Mp3Stream::finish);
}

/// Address of the output buffer, room for `frames` stereo frames.
#[unsafe(no_mangle)]
pub extern "C" fn output_buffer(frames: u32) -> *mut f32 {
    with(|st| {
        st.output.resize(frames as usize * 2, 0.0);
        st.output.as_mut_ptr()
    })
}

/// Reads up to `frames` stereo frames of stream `h` into the output buffer (sized by
/// `output_buffer` first); returns how many were written.
#[unsafe(no_mangle)]
pub extern "C" fn mp3_read(h: u32, frames: u32) -> u32 {
    with(|st| {
        let State {
            streams,
            output,
            scratch,
            ..
        } = st;
        let Some(Some(s)) = streams.get_mut(h as usize) else {
            return 0;
        };
        let frames = (frames as usize).min(output.len() / 2);
        match s.format() {
            Some((1, _)) => {
                scratch.resize(frames, 0.0);
                let n = s.read(scratch);
                for (i, &v) in scratch[..n].iter().enumerate() {
                    output[2 * i] = v;
                    output[2 * i + 1] = v;
                }
                n as u32
            }
            _ => (s.read(&mut output[..frames * 2]) / 2) as u32,
        }
    })
}

/// Bytes received by stream `h` and not decoded yet (for download pacing).
#[unsafe(no_mangle)]
pub extern "C" fn mp3_pending_bytes(h: u32) -> u32 {
    stream(h, 0, |s| s.pending_bytes() as u32)
}

/// Stream `h` drops its last `frames` frames: after a seek, the original's end padding
/// (`mp3_end_padding`), so the gapless end cut stays exact.
#[unsafe(no_mangle)]
pub extern "C" fn mp3_trim_end(h: u32, frames: u32) {
    stream(h, (), |s| s.trim_end(u64::from(frames)));
}

/// The end padding from stream `h`'s tag, frames (0 without one).
#[unsafe(no_mangle)]
pub extern "C" fn mp3_end_padding(h: u32) -> u32 {
    stream(h, 0, |s| s.end_padding() as u32)
}

/// Bitrate of the first audio frame, kbps (0 until decoded).
#[unsafe(no_mangle)]
pub extern "C" fn mp3_bitrate(h: u32) -> u32 {
    stream(h, 0, |s| s.bitrate())
}

/// File offset of the first audio frame; -1 until found.
#[unsafe(no_mangle)]
pub extern "C" fn mp3_audio_start(h: u32) -> f64 {
    stream(h, -1.0, |s| s.audio_start().map_or(-1.0, |o| o as f64))
}

/// Sample rate (0 until the first frame is decoded).
#[unsafe(no_mangle)]
pub extern "C" fn mp3_sample_rate(h: u32) -> u32 {
    stream(h, 0, |s| s.format().map_or(0, |(_, r)| r))
}

/// Length in frames after gapless trimming, from the stream's tag; -1 if unknown.
#[unsafe(no_mangle)]
pub extern "C" fn mp3_total_frames(h: u32) -> f64 {
    stream(h, -1.0, |s| s.total_frames().map_or(-1.0, |t| t as f64))
}

/// 1 once everything is decoded and read.
#[unsafe(no_mangle)]
pub extern "C" fn mp3_ended(h: u32) -> u32 {
    stream(h, 1, |s| u32::from(s.ended()))
}
