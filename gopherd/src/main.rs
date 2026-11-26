//! gopherd CLI.

#![forbid(unsafe_code)]

use anyhow::{Context, Result, bail};
use gopherd::server::{self, Config};
use std::sync::Arc;
use std::time::Duration;
use tokio::net::TcpListener;

const USAGE: &str = "\
Использование: gopherd [ПАРАМЕТРЫ]

Параметры:
  --listen <АДРЕС:ПОРТ>  где слушать (по умолчанию 127.0.0.1:7070)
  --build <ПАПКА>        каталог сборки: меню в gopher/, плейлисты в playlists/
                         (по умолчанию build)
  --media <ПАПКА>        корень медиа (по умолчанию /media/podcasts)
  --max-conns <N>        одновременных соединений (по умолчанию 256)
  -h, --help             эта справка";

fn parse_args() -> Result<Option<(String, Config)>> {
    let mut listen = "127.0.0.1:7070".to_owned();
    let mut build = std::path::PathBuf::from("build");
    let mut media = std::path::PathBuf::from("/media/podcasts");
    let mut max_conns = 256usize;

    let mut it = std::env::args().skip(1);
    while let Some(arg) = it.next() {
        let mut value = |name: &str| it.next().with_context(|| format!("{name}: нужно значение"));
        match arg.as_str() {
            "--listen" => listen = value("--listen")?,
            "--build" => build = value("--build")?.into(),
            "--media" => media = value("--media")?.into(),
            "--max-conns" => {
                let v = value("--max-conns")?;
                max_conns = v
                    .parse()
                    .ok()
                    .filter(|&n| n > 0)
                    .with_context(|| format!("некорректный --max-conns: {v:?}"))?;
            }
            "-h" | "--help" => {
                println!("{USAGE}");
                return Ok(None);
            }
            other => bail!("неизвестный параметр: {other}\n\n{USAGE}"),
        }
    }
    let cfg = Config {
        menus: build.join("gopher"),
        playlists: build.join("playlists"),
        media,
        max_conns,
        read_timeout: Duration::from_secs(10),
        io_timeout: Duration::from_secs(60),
    };
    Ok(Some((listen, cfg)))
}

#[tokio::main]
async fn main() -> Result<()> {
    let Some((listen, cfg)) = parse_args()? else {
        return Ok(());
    };
    let cfg = cfg
        .canonicalize()
        .context("не найдены каталоги меню, плейлистов или медиа")?;
    let listener = TcpListener::bind(&listen)
        .await
        .with_context(|| format!("не удалось слушать {listen}"))?;
    eprintln!(
        "gopherd слушает {listen}: меню {}, плейлисты {}, медиа {}",
        cfg.menus.display(),
        cfg.playlists.display(),
        cfg.media.display()
    );
    server::serve(listener, Arc::new(cfg), shutdown_signal()).await;
    eprintln!("gopherd остановлен");
    Ok(())
}

/// Ctrl-C or SIGTERM (how containers are stopped).
async fn shutdown_signal() {
    let ctrl_c = async {
        let _ = tokio::signal::ctrl_c().await;
    };
    #[cfg(unix)]
    let term = async {
        match tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate()) {
            Ok(mut s) => {
                s.recv().await;
            }
            Err(_) => std::future::pending::<()>().await,
        }
    };
    #[cfg(not(unix))]
    let term = std::future::pending::<()>();
    tokio::select! {
        () = ctrl_c => {}
        () = term => {}
    }
}
