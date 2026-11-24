//! Minimal native parser for pacman.conf.
//!
//! The `pacmanconf` crate shells out to `pacman-conf`, so we parse the file
//! ourselves. Only the keys needed by the health checks are handled.

use std::fs;
use std::path::{Path, PathBuf};

use alpm::SigLevel;
use anyhow::{Context, Result};

#[derive(Debug, Clone)]
pub struct Repo {
    pub name: String,
    pub servers: Vec<String>,
    pub sig_level: SigLevel,
}

#[derive(Debug, Clone)]
pub struct PacmanConf {
    pub root_dir: PathBuf,
    pub db_path: PathBuf,
    pub gpg_dir: PathBuf,
    pub architectures: Vec<String>,
    pub sig_level: SigLevel,
    pub parallel_downloads: u32,
    pub ignore_pkg: Vec<String>,
    pub repos: Vec<Repo>,
}

/// pacman's built-in default (see pacman/conf.c).
fn default_sig_level() -> SigLevel {
    SigLevel::PACKAGE
        | SigLevel::PACKAGE_OPTIONAL
        | SigLevel::DATABASE
        | SigLevel::DATABASE_OPTIONAL
}

/// Applies SigLevel tokens on top of `base`, mirroring pacman's semantics.
fn apply_sig_level(mut level: SigLevel, value: &str) -> SigLevel {
    for token in value.split_whitespace() {
        let (pkg, db, word) = if let Some(w) = token.strip_prefix("Package") {
            (true, false, w)
        } else if let Some(w) = token.strip_prefix("Database") {
            (false, true, w)
        } else {
            (true, true, token)
        };
        let mut set = |sig: SigLevel, opt: SigLevel, marg: SigLevel, unk: SigLevel| match word {
            "Never" => level.remove(sig | opt),
            "Optional" => level.insert(sig | opt),
            "Required" => {
                level.insert(sig);
                level.remove(opt);
            }
            "TrustedOnly" => level.remove(marg | unk),
            "TrustAll" => level.insert(marg | unk),
            _ => {}
        };
        if pkg {
            set(
                SigLevel::PACKAGE,
                SigLevel::PACKAGE_OPTIONAL,
                SigLevel::PACKAGE_MARGINAL_OK,
                SigLevel::PACKAGE_UNKNOWN_OK,
            );
        }
        if db {
            set(
                SigLevel::DATABASE,
                SigLevel::DATABASE_OPTIONAL,
                SigLevel::DATABASE_MARGINAL_OK,
                SigLevel::DATABASE_UNKNOWN_OK,
            );
        }
    }
    level
}

/// Expands an `Include` value; supports a `*` wildcard in the file name.
fn expand_include(pattern: &str) -> Vec<PathBuf> {
    let path = Path::new(pattern);
    let Some(file) = path.file_name().and_then(|f| f.to_str()) else {
        return vec![];
    };
    if !file.contains('*') {
        return vec![path.to_path_buf()];
    }
    let (prefix, suffix) = file.split_once('*').unwrap();
    let dir = path.parent().unwrap_or(Path::new("/"));
    let mut out: Vec<PathBuf> = fs::read_dir(dir)
        .into_iter()
        .flatten()
        .flatten()
        .filter(|e| {
            let n = e.file_name();
            let n = n.to_string_lossy();
            n.starts_with(prefix) && n.ends_with(suffix)
        })
        .map(|e| e.path())
        .collect();
    out.sort();
    out
}

#[derive(Default)]
struct RawRepo {
    name: String,
    servers: Vec<String>,
    sig_level: Option<String>,
}

struct Parser {
    section: Option<String>,
    options: Vec<(String, String)>,
    repos: Vec<RawRepo>,
}

impl Parser {
    fn parse_file(&mut self, path: &Path, depth: u8) -> Result<()> {
        anyhow::ensure!(depth < 10, "troppi Include annidati in {}", path.display());
        let text = fs::read_to_string(path)
            .with_context(|| format!("lettura di {}", path.display()))?;
        for line in text.lines() {
            let line = line.split('#').next().unwrap_or("").trim();
            if line.is_empty() {
                continue;
            }
            if let Some(name) = line.strip_prefix('[').and_then(|l| l.strip_suffix(']')) {
                self.section = Some(name.to_string());
                if name != "options" {
                    self.repos.push(RawRepo { name: name.to_string(), ..Default::default() });
                }
                continue;
            }
            let (key, value) = match line.split_once('=') {
                Some((k, v)) => (k.trim(), v.trim()),
                None => (line, ""),
            };
            if key == "Include" {
                for inc in expand_include(value) {
                    // A missing mirrorlist is not fatal for the whole config.
                    let _ = self.parse_file(&inc, depth + 1);
                }
                continue;
            }
            match self.section.as_deref() {
                Some("options") => self.options.push((key.to_string(), value.to_string())),
                Some(_) => {
                    let repo = self.repos.last_mut().unwrap();
                    match key {
                        "Server" => repo.servers.push(value.to_string()),
                        "SigLevel" => repo.sig_level = Some(value.to_string()),
                        _ => {}
                    }
                }
                None => {}
            }
        }
        Ok(())
    }
}

impl PacmanConf {
    pub fn load(path: &Path) -> Result<Self> {
        let mut p = Parser { section: None, options: vec![], repos: vec![] };
        p.parse_file(path, 0)?;

        let mut conf = PacmanConf {
            root_dir: PathBuf::from("/"),
            db_path: PathBuf::from("/var/lib/pacman/"),
            gpg_dir: PathBuf::from("/etc/pacman.d/gnupg/"),
            architectures: vec![],
            sig_level: default_sig_level(),
            parallel_downloads: 1,
            ignore_pkg: vec![],
            repos: vec![],
        };

        let machine = rustix::system::uname().machine().to_string_lossy().into_owned();
        for (key, value) in &p.options {
            match key.as_str() {
                "RootDir" => conf.root_dir = value.into(),
                "DBPath" => conf.db_path = value.into(),
                "GPGDir" => conf.gpg_dir = value.into(),
                "Architecture" => {
                    for a in value.split_whitespace() {
                        let a = if a == "auto" { machine.as_str() } else { a };
                        conf.architectures.push(a.to_string());
                    }
                }
                "SigLevel" => conf.sig_level = apply_sig_level(conf.sig_level, value),
                "ParallelDownloads" => conf.parallel_downloads = value.parse().unwrap_or(1),
                "IgnorePkg" => conf.ignore_pkg.extend(value.split_whitespace().map(String::from)),
                _ => {}
            }
        }
        if conf.architectures.is_empty() {
            conf.architectures.push(machine);
        }

        let arch = conf.architectures[0].clone();
        conf.repos = p
            .repos
            .into_iter()
            .map(|r| Repo {
                servers: r
                    .servers
                    .iter()
                    .map(|s| s.replace("$repo", &r.name).replace("$arch", &arch))
                    .collect(),
                sig_level: match &r.sig_level {
                    Some(v) => apply_sig_level(conf.sig_level, v),
                    None => conf.sig_level,
                },
                name: r.name,
            })
            .collect();
        Ok(conf)
    }
}
