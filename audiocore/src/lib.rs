//! Audio engine of the text player (`web/textplayer`), built to WebAssembly and run in an
//! AudioWorklet. It turns bytes, fed as they download, into PCM:
//!
//! - [`mp3::Mp3Stream`]: MP3 with the encoder delay and padding cut off (gapless), so the
//!   next track can start on the very next sample.
//!
//! Tracker formats (MOD, S3M, XM, PT2, PT3) come next. The WebAssembly interface is in
//! `wasm` (only on wasm32); everything else is plain Rust, tested on the host.

#![deny(unsafe_code)]

pub mod mp3;

#[cfg(target_arch = "wasm32")]
mod wasm;
