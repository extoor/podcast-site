# Changelog

All notable changes to audiocore, the text player's audio engine. Versions follow
[Semantic Versioning](https://semver.org/): MAJOR for incompatible changes to the exported
WebAssembly functions (the worklet depends on them), MINOR for new features (formats,
functions), PATCH for fixes. The version is the one in `Cargo.toml`; CI checks that this
file starts with it and that a change to the crate raises it.

## 0.1.0

- Streaming MP3 (`mp3::Mp3Stream`) on nanomp3 (a bit-exact port of minimp3): bytes go in
  as they download, PCM comes out; decoding is lazy, so a stream holds compressed bytes
  only.
- Gapless: the encoder delay and padding from the LAME/Lavc tag are cut off exactly as
  `nanomp3::decode_all` does; trailing ID3v1/APE tags are cut before decoding, so the last
  frame is not lost. After a seek, `trim_end(padding)` keeps the end cut exact.
- WebAssembly interface without generated glue: `mp3_new`, `input_buffer`, `mp3_push`,
  `mp3_finish`, `output_buffer`, `mp3_read` (stereo f32), `mp3_sample_rate`,
  `mp3_total_frames`, `mp3_bitrate`, `mp3_audio_start`, `mp3_end_padding`,
  `mp3_trim_end`, `mp3_pending_bytes`, `mp3_ended`, `mp3_free`.
