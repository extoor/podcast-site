//! podscan CLI: scans the media library, writes playlists and `catalog.json`.

#![forbid(unsafe_code)]

use anyhow::{Context, Result, bail};
use podscan::catalog::{Catalog, FORMAT_VERSION};
use podscan::programs::{self, Programs};
use podscan::{m3u, scan};
use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

const USAGE: &str = "\
Использование: podscan [ПАРАМЕТРЫ]

Параметры:
  --media <ПАПКА>     корень медиа (по умолчанию /media/podcasts)
  --out <ПАПКА>       каталог сборки (по умолчанию build)
  --programs <ФАЙЛ>   правила программ (по умолчанию programs.toml)
  --base-url <URL>    базовый URL сайта, например https://example.org
                      (по умолчанию $PODCAST_BASE_URL или http://localhost:8080)
  -h, --help          эта справка";

struct Args {
    media: PathBuf,
    out: PathBuf,
    programs: PathBuf,
    base_url: String,
}

fn parse_args() -> Result<Option<Args>> {
    let mut media = PathBuf::from("/media/podcasts");
    let mut out = PathBuf::from("build");
    let mut programs = PathBuf::from("programs.toml");
    let mut base_url = std::env::var("PODCAST_BASE_URL").ok();

    let mut it = std::env::args().skip(1);
    while let Some(arg) = it.next() {
        let mut value = |name: &str| it.next().with_context(|| format!("{name}: нужно значение"));
        match arg.as_str() {
            "--media" => media = value("--media")?.into(),
            "--out" => out = value("--out")?.into(),
            "--programs" => programs = value("--programs")?.into(),
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
    let base_url = base_url.trim_end_matches('/').to_owned();
    if !(base_url.starts_with("https://") || base_url.starts_with("http://"))
        || base_url
            .chars()
            .any(|c| c.is_whitespace() || c.is_control())
    {
        bail!("некорректный --base-url: {base_url:?}");
    }
    Ok(Some(Args {
        media,
        out,
        programs,
        base_url,
    }))
}

fn main() -> Result<()> {
    let Some(args) = parse_args()? else {
        return Ok(());
    };

    let rules = Programs::load(&args.programs)?;
    let mut podcasts = scan::scan(&args.media, &args.base_url, &rules)?;
    let program_list = programs::group(&mut podcasts);
    let catalog = Catalog {
        format_version: FORMAT_VERSION,
        generated_at: SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0),
        base_url: args.base_url,
        all_playlist: "playlists/all.m3u8".to_owned(),
        programs: program_list,
        podcasts,
    };

    let all_episodes = || catalog.podcasts.iter().flat_map(|p| &p.episodes);
    let by_path: HashMap<&str, _> = all_episodes().map(|e| (e.path.as_str(), e)).collect();

    let mut playlists: Vec<(String, String)> = Vec::new();
    playlists.push((
        catalog.all_playlist.clone(),
        m3u::render("Все эпизоды", all_episodes()),
    ));
    for p in &catalog.podcasts {
        playlists.push((p.playlist.clone(), m3u::render(&p.title, &p.episodes)));
    }
    for a in &catalog.programs {
        let title = a.name.as_deref().unwrap_or("Без программы");
        // Paths come from this same catalog, so all of them are found.
        let eps = a
            .episodes
            .iter()
            .filter_map(|path| by_path.get(path.as_str()).copied());
        playlists.push((a.playlist.clone(), m3u::render(title, eps)));
    }

    // Write playlists and remove stale ones in each directory.
    let mut keep: HashMap<PathBuf, HashSet<String>> = HashMap::new();
    for sub in ["playlists", "playlists/dir", "playlists/program"] {
        let dir = args.out.join(sub);
        fs::create_dir_all(&dir)
            .with_context(|| format!("не удалось создать {}", dir.display()))?;
        keep.insert(dir, HashSet::new());
    }
    for (rel, text) in &playlists {
        let path = args.out.join(rel);
        write_atomic(&path, text.as_bytes())?;
        if let (Some(dir), Some(name)) = (path.parent(), path.file_name().and_then(|n| n.to_str()))
        {
            keep.entry(dir.to_path_buf())
                .or_default()
                .insert(name.to_owned());
        }
    }
    for (dir, names) in &keep {
        remove_stale(dir, names)?;
    }

    let json = serde_json::to_vec_pretty(&catalog)?;
    write_atomic(&args.out.join("catalog.json"), &json)?;

    let episodes: usize = catalog.podcasts.iter().map(|p| p.episodes.len()).sum();
    let untagged: usize = catalog
        .podcasts
        .iter()
        .flat_map(|p| &p.episodes)
        .filter(|e| !e.tagged)
        .count();
    let no_duration: usize = catalog
        .podcasts
        .iter()
        .flat_map(|p| &p.episodes)
        .filter(|e| e.duration_secs.is_none())
        .count();
    let no_program = catalog
        .programs
        .iter()
        .find(|a| a.name.is_none())
        .map_or(0, |a| a.episodes.len());
    println!(
        "Готово: подкастов {}, программ {} (без программы: {no_program}), эпизодов {episodes} (без тега названия: {untagged}, без длительности: {no_duration}) → {}",
        catalog.podcasts.len(),
        catalog.programs.iter().filter(|a| a.name.is_some()).count(),
        args.out.display()
    );
    Ok(())
}

/// Writes to a temporary file next to `path` and renames it, so readers (servers)
/// never see a half-written file.
fn write_atomic(path: &Path, data: &[u8]) -> Result<()> {
    let mut tmp = path.as_os_str().to_owned();
    tmp.push(".tmp");
    let tmp = PathBuf::from(tmp);
    fs::write(&tmp, data).with_context(|| format!("не удалось записать {}", tmp.display()))?;
    fs::rename(&tmp, path).with_context(|| format!("не удалось переименовать в {}", path.display()))
}

/// Removes playlists of podcasts that are no longer in the catalog.
fn remove_stale(dir: &Path, keep: &HashSet<String>) -> Result<()> {
    for entry in fs::read_dir(dir)? {
        let entry = entry?;
        let name = entry.file_name();
        let Some(name) = name.to_str() else { continue };
        if name.ends_with(".m3u8") && !keep.contains(name) {
            fs::remove_file(entry.path())
                .with_context(|| format!("не удалось удалить {}", entry.path().display()))?;
            eprintln!("удалён устаревший плейлист: {name}");
        }
    }
    Ok(())
}
