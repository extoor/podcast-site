//! sitegen CLI: reads `build/catalog.json` and writes `build/gopher/**/gophermap`
//! and `build/www/**/index.html`.

#![forbid(unsafe_code)]

use anyhow::{Context, Result, bail};
use podscan::catalog::{Catalog, FORMAT_VERSION};
use sitegen::gopher::{self, Server};
use sitegen::html;
use std::fs;
use std::path::{Path, PathBuf};

const USAGE: &str = "\
Использование: sitegen [ПАРАМЕТРЫ]

Параметры:
  --build <ПАПКА>       каталог сборки с catalog.json (по умолчанию build)
  --gopher-host <ИМЯ>   имя Gopher-сервера в меню
                        (по умолчанию $GOPHER_HOST или localhost)
  --gopher-port <ПОРТ>  порт Gopher-сервера в меню
                        (по умолчанию $GOPHER_PORT или 7070)
  -h, --help            эта справка";

struct Args {
    build: PathBuf,
    server: Server,
}

fn parse_args() -> Result<Option<Args>> {
    let mut build = PathBuf::from("build");
    let mut host = std::env::var("GOPHER_HOST").unwrap_or_else(|_| "localhost".into());
    let mut port = std::env::var("GOPHER_PORT").unwrap_or_else(|_| "7070".into());

    let mut it = std::env::args().skip(1);
    while let Some(arg) = it.next() {
        let mut value = |name: &str| it.next().with_context(|| format!("{name}: нужно значение"));
        match arg.as_str() {
            "--build" => build = value("--build")?.into(),
            "--gopher-host" => host = value("--gopher-host")?,
            "--gopher-port" => port = value("--gopher-port")?,
            "-h" | "--help" => {
                println!("{USAGE}");
                return Ok(None);
            }
            other => bail!("неизвестный параметр: {other}\n\n{USAGE}"),
        }
    }
    // The host goes into every menu line: it must not break the line format.
    if host.is_empty() || host.chars().any(|c| c.is_whitespace() || c.is_control()) {
        bail!("некорректный --gopher-host: {host:?}");
    }
    let port: u16 = port
        .parse()
        .ok()
        .filter(|&p| p > 0)
        .with_context(|| format!("некорректный --gopher-port: {port:?}"))?;
    Ok(Some(Args {
        build,
        server: Server { host, port },
    }))
}

fn main() -> Result<()> {
    let Some(args) = parse_args()? else {
        return Ok(());
    };
    let path = args.build.join("catalog.json");
    let text = fs::read_to_string(&path)
        .with_context(|| format!("не удалось прочитать {}", path.display()))?;
    let catalog: Catalog =
        serde_json::from_str(&text).with_context(|| format!("ошибка в {}", path.display()))?;
    if catalog.format_version != FORMAT_VERSION {
        bail!(
            "{}: версия формата {}, ожидается {FORMAT_VERSION} — перезапустите podscan",
            path.display(),
            catalog.format_version
        );
    }

    let menus = gopher::menus(&catalog, &args.server)?;
    let pages = html::pages(&catalog, &args.server)?;
    replace_dir(&args.build.join("gopher"), &menus)?;
    replace_dir(&args.build.join("www"), &pages)?;
    println!(
        "Готово: меню Gopher {} → {} (сервер в меню: {}:{}), страниц HTML {} → {}",
        menus.len(),
        args.build.join("gopher").display(),
        args.server.host,
        args.server.port,
        pages.len(),
        args.build.join("www").display()
    );
    Ok(())
}

/// Writes `files` into a fresh directory next to `dir` and swaps it in, so a running
/// server sees either the old or the new set of menus, not a half-written one.
fn replace_dir(dir: &Path, files: &[(String, String)]) -> Result<()> {
    let new = dir.with_extension("new");
    let old = dir.with_extension("old");
    for d in [&new, &old] {
        if d.exists() {
            fs::remove_dir_all(d).with_context(|| format!("не удалось удалить {}", d.display()))?;
        }
    }
    for (rel, text) in files {
        let path = new.join(rel);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)
                .with_context(|| format!("не удалось создать {}", parent.display()))?;
        }
        fs::write(&path, text)
            .with_context(|| format!("не удалось записать {}", path.display()))?;
    }
    if dir.exists() {
        fs::rename(dir, &old)
            .with_context(|| format!("не удалось переименовать {}", dir.display()))?;
    }
    fs::rename(&new, dir).with_context(|| format!("не удалось переименовать {}", new.display()))?;
    if old.exists() {
        fs::remove_dir_all(&old)
            .with_context(|| format!("не удалось удалить {}", old.display()))?;
    }
    Ok(())
}
