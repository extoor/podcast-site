//! The site's version: the short commit the generator was built from, shown on every
//! page and in the Gopher root menu. CI passes it as GIT_COMMIT (the Docker build has
//! no .git); a local build asks git, adding "-dirty" for uncommitted changes.

use std::process::Command;

fn git(args: &[&str]) -> Option<String> {
    let out = Command::new("git").args(args).output().ok()?;
    out.status
        .success()
        .then(|| String::from_utf8_lossy(&out.stdout).trim().to_owned())
}

fn main() {
    println!("cargo:rerun-if-env-changed=GIT_COMMIT");
    let commit = match std::env::var("GIT_COMMIT") {
        Ok(sha) if !sha.trim().is_empty() => sha.trim().chars().take(7).collect(),
        _ => {
            // rebuild when HEAD moves or the working tree is staged
            if let Some(dir) = git(&["rev-parse", "--git-dir"]) {
                for f in ["HEAD", "index"] {
                    println!("cargo:rerun-if-changed={dir}/{f}");
                }
            }
            match git(&["rev-parse", "--short=7", "HEAD"]) {
                Some(sha) => match git(&["status", "--porcelain", "--untracked-files=no"]) {
                    Some(s) if !s.is_empty() => format!("{sha}-dirty"),
                    _ => sha,
                },
                None => "dev".to_owned(),
            }
        }
    };
    // only what is safe in HTML, Gopher menus and URLs
    let commit: String = commit
        .chars()
        .filter(|c| c.is_ascii_alphanumeric() || *c == '-')
        .collect();
    println!("cargo:rustc-env=SITE_COMMIT={commit}");
}
