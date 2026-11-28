# Changelog

All notable changes to textplayer. Versions follow [Semantic Versioning](https://semver.org/):
MAJOR for incompatible changes to options, the API or the CSS variables, MINOR for new
features, PATCH for fixes. The version lives in one place, `VERSION` in `textplayer.js`;
CI checks that every change to the file bumps it and that this file has an entry for it.

## 0.4.0

- A WebAssembly engine for MP3 (option `engine: { wasm, worklet }`): the page downloads
  the file in range requests of 512 KB, only as needed to stay about two minutes ahead
  (no connection held open for minutes; a failed request is retried with a growing
  delay), and an AudioWorklet (`worklet.js`) decodes it with audiocore (Rust,
  minimp3-exact). It stands in for the `<audio>` element; without
  the option, in a browser without AudioWorklet or WebAssembly, or if the engine fails
  to load, the player uses the element as before.
- Gapless playback with the engine (option `gapless`, on by default): the encoder delay
  and padding are cut off, and the next track is downloaded ahead and starts on the very
  next sample. Off, the next track starts when the current one has ended.
- Seeking with the engine is a new range request; the end of the track stays exact.
- `Decoder:` shows `audiocore` while the engine plays, the browser's name with `<audio>`.
- `O` opens the options in place of the key line, like the `H` help: gapless playback,
  the analyzer (shown / hidden) and its theme. `↑↓` pick, `←→`, Space or Enter change, `O`
  or `Esc` close. Every choice is kept in a cookie: the new `tp_gapless` (option
  `gaplessCookie`; it wins over the page's `gapless`), and the analyzer's two. In English,
  Russian or Ukrainian, after `lang`. The key line ends with `O:opts | H:help` instead of `H:help | Esc:exit`
  (the help still lists Esc).
- The titles of the help and the options sit in the middle of the separator above them,
  next to a theme's name on the right; the panels start right with their rows.

## 0.3.0

- `H` opens a detailed help instead of the key line: every key and what it does, and the
  mouse. In English, Russian or Ukrainian, after `<html lang>` or the
  new `lang` option. `H` or `Esc` closes it; with the help open, `Esc` and Backspace close
  the help, not the player.
- The key line shows `H:help` instead of `A:spec`.
- One `VERSION` constant: the default title (`Browser Player v0.3.0`) and
  `TextPlayer.version` show it.

## 0.2.0

- Color themes of the spectrum analyzer: Classic, Fire, Ice, Rainbow, Matrix, Amber,
  Synthwave. `T` / `Shift+T` switch them, the choice is kept in the `tp_spectrum_theme`
  cookie; for a second the separator under the analyzer names the new theme. Options `spectrumTheme`,
  `spectrumThemeCookie`; `TextPlayer.themes`.
- The info line of the stream is labelled `Codec:` instead of `Type:`.

## 0.1.0

- First version: a DOS-style window for plain links to audio files, keyboard and mouse
  control, buffer and volume bars, the spectrum analyzer (`A`, `tp_spectrum` cookie).
