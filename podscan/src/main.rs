//! podscan CLI: scans the media library, writes playlists and `catalog.json`.

#![forbid(unsafe_code)]

use anyhow::{Context, Result, bail};
use podscan::build::{self, Options};
use std::path::PathBuf;

const USAGE: &str = "\
Использование: podscan [ПАРАМЕТРЫ]

Параметры:
  --media <ПАПКА>     корень медиа (по умолчанию /media/podcasts)
  --out <ПАПКА>       каталог сборки (по умолчанию build)
  --rules <ФАЙЛ>      правила подкастов (по умолчанию podcasts.toml)
  --base-url <URL>    базовый URL сайта, например https://example.org
                      (по умолчанию $PODCAST_BASE_URL или http://localhost:8080)
  -h, --help          эта справка";

fn parse_args() -> Result<Option<Options>> {
    let mut media = PathBuf::from("/media/podcasts");
    let mut out = PathBuf::from("build");
    let mut rules = PathBuf::from("podcasts.toml");
    let mut base_url = std::env::var("PODCAST_BASE_URL").ok();

    let mut it = std::env::args().skip(1);
    while let Some(arg) = it.next() {
        let mut value = |name: &str| it.next().with_context(|| format!("{name}: нужно значение"));
        match arg.as_str() {
            "--media" => media = value("--media")?.into(),
            "--out" => out = value("--out")?.into(),
            "--rules" => rules = value("--rules")?.into(),
            "--base-url" => base_url = Some(value("--base-url")?),
            "-h" | "--help" => {
                println!("{USAGE}");
                return Ok(None);
            }
            other => bail!("неизвестный параметр: {other}\n\n{USAGE}"),
        }
    }

    let base_url = base_url.unwrap_or_else(|| {
        eprintln!("предупреждение: --base-url не задан, используется http://localhost:8080");
        "http://localhost:8080".to_owned()
    });
    Ok(Some(Options {
        media,
        out,
        rules,
        base_url: build::base_url(&base_url)?,
    }))
}

fn main() -> Result<()> {
    let Some(o) = parse_args()? else {
        return Ok(());
    };
    let catalog = build::run(&o)?;
    println!("Готово: {} → {}", build::summary(&catalog), o.out.display());
    Ok(())
}
