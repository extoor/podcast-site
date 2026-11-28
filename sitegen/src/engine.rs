//! The text player's WebAssembly engine as site files: audiocore's module and the
//! worklet, under names made from their contents (`player/audiocore.<hash>.wasm`). A new
//! build of either gets a new name, so browsers may cache them for good (Caddy sends
//! `immutable`) and still never keep an old one: the pages, revalidated on every visit,
//! point at the new names.

/// The worklet that runs the module (web/textplayer/worklet.js).
const WORKLET_JS: &str = include_str!("../../web/textplayer/worklet.js");

/// Engine files and their URLs.
pub struct Engine {
    pub wasm_url: String,
    pub worklet_url: String,
    /// Path relative to the www root → contents.
    pub files: Vec<(String, Vec<u8>)>,
}

impl Engine {
    /// From the bytes of `audiocore.wasm`.
    pub fn new(wasm: Vec<u8>) -> Self {
        let wasm_path = content_name("audiocore", "wasm", &wasm);
        let worklet_path = content_name("worklet", "js", WORKLET_JS.as_bytes());
        Self {
            wasm_url: format!("/{wasm_path}"),
            worklet_url: format!("/{worklet_path}"),
            files: vec![
                (wasm_path, wasm),
                (worklet_path, WORKLET_JS.as_bytes().to_vec()),
            ],
        }
    }
}

/// `player/<stem>.<hash>.<ext>`: 64-bit FNV-1a of the contents, in hex. Not a security
/// hash: it only has to change when the contents do.
fn content_name(stem: &str, ext: &str, bytes: &[u8]) -> String {
    let hash = bytes.iter().fold(0xcbf2_9ce4_8422_2325u64, |h, &b| {
        (h ^ u64::from(b)).wrapping_mul(0x0000_0100_0000_01b3)
    });
    format!("player/{stem}.{hash:016x}.{ext}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_follow_contents() {
        let a = Engine::new(b"\0asm one".to_vec());
        let b = Engine::new(b"\0asm two".to_vec());
        assert!(a.wasm_url.starts_with("/player/audiocore.") && a.wasm_url.ends_with(".wasm"));
        assert_ne!(a.wasm_url, b.wasm_url, "another module, another name");
        assert_eq!(
            a.worklet_url, b.worklet_url,
            "the same worklet, the same name"
        );
        assert_eq!(a.files.len(), 2);
        // the name is the URL without the leading slash
        assert_eq!(format!("/{}", a.files[0].0), a.wasm_url);
        // FNV-1a test vector
        assert_eq!(content_name("x", "y", b"a"), "player/x.af63dc4c8601ec8c.y");
    }
}
