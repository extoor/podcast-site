//! End-to-end tests: a real server on a random port, fixture directories with
//! a secret file outside the roots and symlinks pointing at it.

use gopherd::server::{self, Config};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::oneshot;

struct Fixture {
    dir: PathBuf,
    addr: std::net::SocketAddr,
    _stop: oneshot::Sender<()>,
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

fn write(path: &Path, text: &str) {
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, text).unwrap();
}

async fn start(name: &str, max_conns: usize) -> Fixture {
    let dir = std::env::temp_dir().join(format!("gopherd-{}-{name}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let (menus, playlists, media) = (dir.join("gopher"), dir.join("playlists"), dir.join("media"));
    write(&menus.join("gophermap"), "iroot\t\th\t1\r\n.\r\n");
    write(
        &menus.join("podcast/x/gophermap"),
        "ipodcast x\t\th\t1\r\n.\r\n",
    );
    write(&playlists.join("all.m3u8"), "#EXTM3U\n");
    write(&media.join("Show/ep 1.mp3"), "MP3DATA");
    write(&dir.join("secret.txt"), "SECRET");
    #[cfg(unix)]
    {
        use std::os::unix::fs::symlink;
        symlink(dir.join("secret.txt"), media.join("Show/link.mp3")).unwrap();
        symlink(&dir, media.join("escape")).unwrap();
        symlink(dir.join("secret.txt"), menus.join("podcast/x/evil")).unwrap();
        std::fs::create_dir_all(menus.join("podcast/y")).unwrap();
        symlink(dir.join("secret.txt"), menus.join("podcast/y/gophermap")).unwrap();
    }

    let cfg = Config {
        menus,
        playlists,
        media,
        max_conns,
        read_timeout: Duration::from_millis(300),
        io_timeout: Duration::from_secs(2),
        log_probes: false,
    }
    .canonicalize()
    .unwrap();
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let (stop, stopped) = oneshot::channel::<()>();
    tokio::spawn(server::serve(listener, Arc::new(cfg), async {
        let _ = stopped.await;
    }));
    Fixture {
        dir,
        addr,
        _stop: stop,
    }
}

async fn get(f: &Fixture, request: &[u8]) -> String {
    let mut s = TcpStream::connect(f.addr).await.unwrap();
    s.write_all(request).await.unwrap();
    let mut out = Vec::new();
    tokio::time::timeout(Duration::from_secs(5), s.read_to_end(&mut out))
        .await
        .unwrap()
        .unwrap();
    String::from_utf8_lossy(&out).into_owned()
}

const NOT_FOUND: &str = "3Не найдено\t\terror.host\t1\r\n.\r\n";
const BAD: &str = "3Неверный запрос\t\terror.host\t1\r\n.\r\n";

#[tokio::test]
async fn health_selector_is_the_root_menu() {
    let f = start("health", 8).await;
    assert_eq!(get(&f, b"/health\r\n").await, get(&f, b"\r\n").await);
    assert!(get(&f, b"/health\r\n").await.starts_with("iroot"));
    // only the exact selector: below it is an ordinary (missing) path
    assert_eq!(get(&f, b"/health/x\r\n").await, NOT_FOUND);
}

#[tokio::test]
async fn serves_menus_playlists_media() {
    let f = start("ok", 8).await;
    assert_eq!(get(&f, b"\r\n").await, "iroot\t\th\t1\r\n.\r\n");
    assert_eq!(get(&f, b"/\r\n").await, "iroot\t\th\t1\r\n.\r\n");
    assert_eq!(
        get(&f, b"/podcast/x/\r\n").await,
        "ipodcast x\t\th\t1\r\n.\r\n"
    );
    assert_eq!(
        get(&f, b"/podcast/x\n").await,
        "ipodcast x\t\th\t1\r\n.\r\n"
    );
    assert_eq!(get(&f, b"/playlists/all.m3u8\r\n").await, "#EXTM3U\n");
    assert_eq!(get(&f, b"/media/Show/ep 1.mp3\r\n").await, "MP3DATA");
    // Gopher+ marker after a tab is ignored
    assert_eq!(get(&f, b"/media/Show/ep 1.mp3\t+\r\n").await, "MP3DATA");
}

#[tokio::test]
async fn not_found() {
    let f = start("nf", 8).await;
    assert_eq!(get(&f, b"/podcast/nope/\r\n").await, NOT_FOUND);
    assert_eq!(get(&f, b"/media/Show/nope.mp3\r\n").await, NOT_FOUND);
    // a media directory is not a file
    assert_eq!(get(&f, b"/media/Show\r\n").await, NOT_FOUND);
}

#[tokio::test]
async fn traversal_is_rejected() {
    let f = start("trav", 8).await;
    for req in [
        &b"/../secret.txt\r\n"[..],
        b"/media/../secret.txt\r\n",
        b"/media/../../../../etc/passwd\r\n",
        b"..\r\n",
        b"/media/Show/%2e%2e/x\r\n",
        b"/a\x00b\r\n",
        b"/media/Show\\..\\..\\secret.txt\r\n",
    ] {
        let got = get(&f, req).await;
        assert!(got == BAD || got == NOT_FOUND, "{req:?} → {got:?}");
        assert!(!got.contains("SECRET"));
    }
}

#[cfg(unix)]
#[tokio::test]
async fn symlinks_outside_root_are_rejected() {
    let f = start("sym", 8).await;
    for req in [
        &b"/media/Show/link.mp3\r\n"[..],
        b"/media/escape/secret.txt\r\n",
        b"/podcast/x/evil\r\n",
        b"/podcast/y/\r\n",
    ] {
        assert_eq!(get(&f, req).await, NOT_FOUND, "{req:?}");
    }
}

#[tokio::test]
async fn long_request_is_rejected() {
    let f = start("long", 8).await;
    let mut long = vec![b'/'];
    long.extend(std::iter::repeat_n(b'a', 300));
    long.extend(b"\r\n");
    assert_eq!(get(&f, &long).await, BAD);
    // endless line without a line end: cut off, not buffered forever
    let big = vec![b'a'; 100_000];
    assert_eq!(get(&f, &big).await, BAD);
}

#[tokio::test]
async fn silent_client_is_dropped() {
    let f = start("slow", 8).await;
    let mut s = TcpStream::connect(f.addr).await.unwrap();
    let mut out = Vec::new();
    // read_timeout is 300 ms: the server closes the connection without a reply
    let n = tokio::time::timeout(Duration::from_secs(3), s.read_to_end(&mut out))
        .await
        .expect("server did not close a silent connection");
    assert_eq!(n.unwrap(), 0);
}

#[tokio::test]
async fn connection_limit() {
    let f = start("limit", 1).await;
    // occupy the only slot
    let _busy = TcpStream::connect(f.addr).await.unwrap();
    tokio::time::sleep(Duration::from_millis(50)).await;
    let got = get(&f, b"/\r\n").await;
    assert!(got.starts_with("3Сервер перегружен"), "{got:?}");
}
