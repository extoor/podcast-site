//! The server: accepts connections, reads one request line, sends one file.
//!
//! Read-only by construction: files are only opened for reading, nothing is executed
//! or written. Every path is resolved (symlinks included) and must stay inside its
//! root. Limits: request line length, read and write timeouts, concurrent connections.

use crate::selector::{self, Reject, Root, Route};
use std::future::Future;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;
use tokio::fs::{self, File};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::Semaphore;
use tokio::time::timeout;

/// Longest request line, bytes: a selector (≤ 255) plus whatever a client sends after
/// a tab (Gopher+ markers, search terms), which is ignored.
const MAX_LINE: usize = 1024;

/// Chunk size for sending files.
const CHUNK: usize = 64 * 1024;

#[derive(Debug, Clone)]
pub struct Config {
    /// Generated menus (`build/gopher`).
    pub menus: PathBuf,
    /// Playlists (`build/playlists`).
    pub playlists: PathBuf,
    /// Audio library (read-only mount).
    pub media: PathBuf,
    /// Concurrent connections; extra clients get an error and are disconnected.
    pub max_conns: usize,
    /// Time to receive the whole request line.
    pub read_timeout: Duration,
    /// Time allowed for each chunk to be read from disk and written to the client.
    pub io_timeout: Duration,
}

impl Config {
    /// Resolves the roots once, so that every request is checked against real paths.
    pub fn canonicalize(mut self) -> io::Result<Self> {
        self.menus = std::fs::canonicalize(&self.menus)?;
        self.playlists = std::fs::canonicalize(&self.playlists)?;
        self.media = std::fs::canonicalize(&self.media)?;
        Ok(self)
    }

    fn root(&self, root: Root) -> &Path {
        match root {
            Root::Menus => &self.menus,
            Root::Playlists => &self.playlists,
            Root::Media => &self.media,
        }
    }
}

/// Connections being told "server busy" at the same time, beyond `max_conns`.
const REJECT_SLOTS: usize = 64;

/// How long a "server busy" reply may take.
const REJECT_TIME: Duration = Duration::from_secs(1);

/// Tells a client the server is busy. Closing a socket with an unread request makes
/// TCP send a reset, which can destroy the reply, so: write, end the sending side,
/// then drain whatever the client sent until it closes (bounded by the caller).
async fn reject(mut stream: TcpStream) -> io::Result<()> {
    stream
        .write_all(error_menu("Сервер перегружен, попробуйте позже").as_bytes())
        .await?;
    stream.shutdown().await?;
    let mut sink = [0u8; 512];
    while stream.read(&mut sink).await? > 0 {}
    Ok(())
}

/// Gopher error menu (type 3).
fn error_menu(msg: &str) -> String {
    format!("3{msg}\t\terror.host\t1\r\n.\r\n")
}

/// Accepts connections until `shutdown` completes.
pub async fn serve(listener: TcpListener, cfg: Arc<Config>, shutdown: impl Future<Output = ()>) {
    let slots = Arc::new(Semaphore::new(cfg.max_conns));
    // Rejecting a client politely also takes a task; cap those as well.
    let reject_slots = Arc::new(Semaphore::new(REJECT_SLOTS));
    tokio::pin!(shutdown);
    loop {
        let (stream, peer) = tokio::select! {
            () = &mut shutdown => break,
            accepted = listener.accept() => match accepted {
                Ok(a) => a,
                Err(e) => {
                    // e.g. too many open files: back off instead of spinning
                    eprintln!("ошибка accept: {e}");
                    tokio::time::sleep(Duration::from_millis(100)).await;
                    continue;
                }
            },
        };
        let Ok(permit) = Arc::clone(&slots).try_acquire_owned() else {
            eprintln!("{peer} отклонено: лимит соединений");
            // If even the reject slots are taken, the connection is just dropped.
            if let Ok(permit) = Arc::clone(&reject_slots).try_acquire_owned() {
                tokio::spawn(async move {
                    let _permit = permit;
                    let _ = timeout(REJECT_TIME, reject(stream)).await;
                });
            }
            continue;
        };
        let cfg = Arc::clone(&cfg);
        tokio::spawn(async move {
            let _permit = permit;
            handle(stream, &cfg, peer).await;
        });
    }
}

async fn handle(mut stream: TcpStream, cfg: &Config, peer: std::net::SocketAddr) {
    let line = match timeout(cfg.read_timeout, read_line(&mut stream)).await {
        Ok(Ok(Some(line))) => line,
        Ok(Ok(None)) => return, // closed without a request
        Ok(Err(e)) => {
            eprintln!("{peer} ошибка чтения: {e}");
            return;
        }
        Err(_) => {
            eprintln!("{peer} таймаут запроса");
            return;
        }
    };
    let shown = String::from_utf8_lossy(selector::selector(&line).unwrap_or(&line)).into_owned();

    let route = match line.len().le(&MAX_LINE).then(|| selector::selector(&line)) {
        Some(Ok(sel)) => selector::route(sel),
        _ => Err(Reject::TooLong),
    };
    let result = match route {
        Ok(route) => match resolve(cfg, &route).await {
            Some(path) => send_file(&mut stream, &path, cfg.io_timeout).await,
            None => send_error(&mut stream, "Не найдено", cfg.io_timeout)
                .await
                .map(|_| 0),
        },
        Err(_) => send_error(&mut stream, "Неверный запрос", cfg.io_timeout)
            .await
            .map(|_| 0),
    };
    match result {
        Ok(bytes) => eprintln!("{peer} {shown:?} {bytes}"),
        Err(e) => eprintln!("{peer} {shown:?} прервано: {e}"),
    }
    let _ = timeout(cfg.io_timeout, stream.shutdown()).await;
}

/// Reads up to the first line end. `None` if the client closed the connection first.
/// A line longer than [`MAX_LINE`] is returned cut (and then rejected as too long).
async fn read_line(stream: &mut TcpStream) -> io::Result<Option<Vec<u8>>> {
    let mut buf = Vec::with_capacity(256);
    let mut chunk = [0u8; 512];
    loop {
        let n = stream.read(&mut chunk).await?;
        if n == 0 {
            return Ok(if buf.is_empty() { None } else { Some(buf) });
        }
        buf.extend_from_slice(&chunk[..n]);
        if let Some(end) = buf.iter().position(|&b| b == b'\n') {
            buf.truncate(end);
            if buf.last() == Some(&b'\r') {
                buf.pop();
            }
            return Ok(Some(buf));
        }
        if buf.len() > MAX_LINE {
            return Ok(Some(buf));
        }
    }
}

/// The file to send for a route, or `None` (not found or not allowed).
/// The real path, after resolving symlinks, must be inside the route's root.
async fn resolve(cfg: &Config, route: &Route) -> Option<PathBuf> {
    let root = cfg.root(route.root);
    let path = inside(root, &root.join(&route.rel)).await?;
    let meta = fs::metadata(&path).await.ok()?;
    if meta.is_file() {
        return Some(path);
    }
    if meta.is_dir() && route.root == Root::Menus {
        let map = inside(root, &path.join("gophermap")).await?;
        return fs::metadata(&map).await.ok()?.is_file().then_some(map);
    }
    None
}

async fn inside(root: &Path, path: &Path) -> Option<PathBuf> {
    let real = fs::canonicalize(path).await.ok()?;
    real.starts_with(root).then_some(real)
}

async fn send_file(stream: &mut TcpStream, path: &Path, io_timeout: Duration) -> io::Result<u64> {
    let mut file = File::open(path).await?;
    let mut buf = vec![0u8; CHUNK];
    let mut sent = 0u64;
    loop {
        let n = timeout(io_timeout, file.read(&mut buf))
            .await
            .map_err(|_| timed_out())??;
        if n == 0 {
            return Ok(sent);
        }
        timeout(io_timeout, stream.write_all(&buf[..n]))
            .await
            .map_err(|_| timed_out())??;
        sent += n as u64;
    }
}

async fn send_error(stream: &mut TcpStream, msg: &str, io_timeout: Duration) -> io::Result<()> {
    timeout(io_timeout, stream.write_all(error_menu(msg).as_bytes()))
        .await
        .map_err(|_| timed_out())?
}

fn timed_out() -> io::Error {
    io::Error::new(io::ErrorKind::TimedOut, "таймаут")
}
