//! sitegen CLI: reads `build/catalog.json` and writes `build/gopher/**/gophermap`
//! and `build/www/**/index.html`. With `--scan` it first runs the podscan scan itself,
//! so one command (one container, no shell) builds everything.

#![forbid(unsafe_code)]

use anyhow::{Context, Result, bail};
use podscan::build::{self, Options};
use podscan::catalog::{Catalog, FORMAT_VERSION};
use sitegen::gopher::{self, Server};
use sitegen::html;
use std::fs;
use std::path::{Path, PathBuf};

const USAGE: &str = "\
Использование: sitegen [ПАРАМЕТРЫ]

Параметры:
  --build <ПАПКА>       каталог сборки с catalog.json (по умолчанию build)
  --scan                сначала просканировать медиатеку (как podscan): плейлисты
                        и catalog.json пишутся в каталог сборки
  --media <ПАПКА>       с --scan: корень медиа (по умолчанию /media/podcasts)
  --rules <ФАЙЛ>        с --scan: правила подкастов (по умолчанию podcasts.toml)
  --base-url <URL>      с --scan: базовый URL сайта для плейлистов
                        (по умолчанию $PODCAST_BASE_URL или http://localhost:8080)
  --gopher-host <ИМЯ>   имя Gopher-сервера в меню
                        (по умолчанию $GOPHER_HOST или localhost)
  --gopher-port <ПОРТ>  порт Gopher-сервера в меню
                        (по умолчанию $GOPHER_PORT или 7070)
  -h, --help            эта справка";

struct Args {
    build: PathBuf,
    server: Server,
    /// `--scan`: scan the library first (else read the existing `catalog.json`).
    scan: Option<Options>,
}

fn parse_args() -> Result<Option<Args>> {
    let mut build = PathBuf::from("build");
    let mut host = std::env::var("GOPHER_HOST").unwrap_or_else(|_| "localhost".into());
    let mut port = std::env::var("GOPHER_PORT").unwrap_or_else(|_| "7070".into());
    let mut scan = false;
    let mut media = PathBuf::from("/media/podcasts");
    let mut rules = PathBuf::from("podcasts.toml");
    let mut base_url = std::env::var("PODCAST_BASE_URL").ok();

    let mut it = std::env::args().skip(1);
    while let Some(arg) = it.next() {
        let mut value = |name: &str| it.next().with_context(|| format!("{name}: нужно значение"));
        match arg.as_str() {
            "--build" => build = value("--build")?.into(),
            "--gopher-host" => host = value("--gopher-host")?,
            "--gopher-port" => port = value("--gopher-port")?,
            "--scan" => scan = true,
            "--media" => media = value("--media")?.into(),
            "--rules" => rules = value("--rules")?.into(),
            "--base-url" => base_url = Some(value("--base-url")?),
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
    let scan = if scan {
        let base_url = base_url.unwrap_or_else(|| {
            eprintln!("предупреждение: --base-url не задан, используется http://localhost:8080");
            "http://localhost:8080".to_owned()
        });
        Some(Options {
            media,
            out: build.clone(),
            rules,
            base_url: build::base_url(&base_url)?,
        })
    } else {
        None
    };
    Ok(Some(Args {
        build,
        server: Server { host, port },
        scan,
    }))
}

fn main() -> Result<()> {
    let Some(args) = parse_args()? else {
        return Ok(());
    };
    let catalog = match &args.scan {
        Some(o) => {
            let catalog = build::run(o)?;
            println!("Скан: {}", build::summary(&catalog));
            catalog
        }
        None => read_catalog(&args.build.join("catalog.json"))?,
    };

    let mut menus = gopher::menus(&catalog, &args.server)?;
    menus.push(("robots.txt".to_owned(), sitegen::ROBOTS_TXT.to_owned()));
    let pages = html::pages(&catalog, &args.server)?;
    replace_dir(&args.build.join("gopher"), &menus)?;
    let mut www: Vec<(String, Vec<u8>)> = pages
        .iter()
        .map(|(p, t)| (p.clone(), t.clone().into_bytes()))
        .collect();
    www.extend(sitegen::icon::files()?);
    www.push((
        "robots.txt".to_owned(),
        sitegen::ROBOTS_TXT.as_bytes().to_vec(),
    ));
    replace_dir(&args.build.join("www"), &www)?;
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

fn read_catalog(path: &Path) -> Result<Catalog> {
    let text = fs::read_to_string(path)
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
    Ok(catalog)
}

/// Writes `files` into a fresh directory next to `dir` and swaps it in, so a running
/// server sees either the old or the new set of menus, not a half-written one.
fn replace_dir<T: AsRef<[u8]>>(dir: &Path, files: &[(String, T)]) -> Result<()> {
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
