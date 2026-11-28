//! The streaming decoder against nanomp3's whole-file decoder: the same samples, bit for
//! bit, whatever the sizes of the chunks the bytes arrive in.
//!
//! Real files are not in the repository: set AUDIOCORE_MP3 to a list of MP3 paths
//! (separated by ':'), or the test checks only a tiny stream built here.

use audiocore::mp3::Mp3Stream;

/// Feeds `data` in chunks of pseudo-random sizes (1 byte .. 64 KiB), reading samples in
/// between as a player would.
fn stream(data: &[u8], seed: u64) -> (Vec<f32>, Mp3Stream) {
    let mut s = Mp3Stream::new();
    let mut out = Vec::new();
    let mut buf = vec![0f32; 4096];
    let mut rng = seed;
    let mut at = 0;
    while at < data.len() {
        rng = rng
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        let n = 1 + (rng >> 33) as usize % (64 * 1024);
        let end = (at + n).min(data.len());
        s.push(&data[at..end]);
        at = end;
        loop {
            let k = s.read(&mut buf);
            if k == 0 {
                break;
            }
            out.extend_from_slice(&buf[..k]);
        }
    }
    s.finish();
    loop {
        let k = s.read(&mut buf);
        if k == 0 {
            break;
        }
        out.extend_from_slice(&buf[..k]);
    }
    (out, s)
}

fn check(name: &str, data: &[u8]) {
    let reference = nanomp3::decode_all::<f32>(data);
    for seed in [1, 2, 3] {
        let (samples, s) = stream(data, seed);
        assert_eq!(
            samples.len(),
            reference.samples.len(),
            "{name}: sample count (seed {seed})"
        );
        let same = samples
            .iter()
            .zip(&reference.samples)
            .all(|(a, b)| a.to_bits() == b.to_bits());
        assert!(same, "{name}: samples differ (seed {seed})");
        assert!(s.ended(), "{name}: not ended");
        if let Some(total) = s.total_frames() {
            let ch = s.format().map_or(1, |(c, _)| u64::from(c));
            assert_eq!(
                total * ch,
                samples.len() as u64,
                "{name}: total from the tag"
            );
        }
    }
    let (_, s) = stream(data, 0);
    let tag = s.total_frames().map_or("no LAME tag".to_owned(), |t| {
        format!("LAME tag: {t} samples per channel")
    });
    eprintln!(
        "{name}: {} samples, identical; {tag}",
        reference.samples.len()
    );
}

/// A seek: a new stream from a byte in the middle, told the original's end padding,
/// ends exactly where the original does (the last 10 seconds are the same samples).
fn check_seek(name: &str, data: &[u8]) {
    let reference = nanomp3::decode_all::<f32>(data);
    let (_, whole) = stream(data, 0);
    let padding = whole.end_padding();
    let ch = whole.format().map_or(1, |(c, _)| usize::from(c));
    let tail = 10 * 44_100 * ch;
    for percent in [25, 50, 90] {
        let at = data.len() * percent / 100;
        let mut s = Mp3Stream::new();
        s.trim_end(padding);
        let (samples, _) = {
            let mut out = Vec::new();
            let mut buf = vec![0f32; 4096];
            for chunk in data[at..].chunks(48 * 1024) {
                s.push(chunk);
                while let k @ 1.. = s.read(&mut buf) {
                    out.extend_from_slice(&buf[..k]);
                }
            }
            s.finish();
            while let k @ 1.. = s.read(&mut buf) {
                out.extend_from_slice(&buf[..k]);
            }
            (out, ())
        };
        let want = &reference.samples[reference.samples.len() - tail..];
        let got = &samples[samples.len() - tail..];
        let same = got
            .iter()
            .zip(want)
            .all(|(a, b)| a.to_bits() == b.to_bits());
        assert!(same, "{name}: the end after a seek to {percent}% differs");
        // and it starts where the byte offset says, within a frame
        let expected = reference.samples.len() as f64 * (1.0 - percent as f64 / 100.0);
        assert!(
            (samples.len() as f64 - expected).abs() < 2.0 * 1152.0 * ch as f64 + expected * 0.01
        );
    }
    eprintln!("{name}: seeks end exactly (padding {padding})");
}

#[test]
fn matches_whole_file_decoding() {
    // junk only: nothing comes out, nothing hangs
    check("junk", &[0x55; 50_000]);
    check("empty", &[]);
    let Ok(list) = std::env::var("AUDIOCORE_MP3") else {
        eprintln!("AUDIOCORE_MP3 not set: real files skipped");
        return;
    };
    for path in list.split(':').filter(|p| !p.is_empty()) {
        let data = std::fs::read(path).unwrap();
        check(path, &data);
        check_seek(path, &data);
    }
}
