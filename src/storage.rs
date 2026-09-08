//! Where the note and config live on disk, and how they are written safely.

use std::io::Write;
use std::path::{Path, PathBuf};

pub fn note_path() -> PathBuf {
    match dirs::data_dir() {
        Some(dir) => dir.join("noted").join("notes.md"),
        None => PathBuf::from("notes.md"),
    }
}

fn config_path() -> PathBuf {
    note_path().with_file_name("config")
}

/// Crash-safe write: roll a `.bak`, write to a temp file, fsync, then rename
/// over the target (rename is atomic on the same filesystem).
pub fn atomic_save(path: &Path, contents: &str) -> std::io::Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    if path.exists() {
        let _ = std::fs::copy(path, path.with_extension("bak"));
    }
    let tmp = path.with_extension("tmp");
    {
        let mut f = std::fs::File::create(&tmp)?;
        f.write_all(contents.as_bytes())?;
        f.sync_all()?;
    }
    std::fs::rename(&tmp, path)
}

/// Persisted settings. Plain `key=value` lines; unknown keys are ignored.
#[derive(Default)]
pub struct Config {
    pub theme: u8,
    pub pos: Option<(f32, f32)>,
}

impl Config {
    pub fn load() -> Self {
        let mut c = Config::default();
        let Ok(s) = std::fs::read_to_string(config_path()) else {
            return c;
        };
        for line in s.lines() {
            if let Some(v) = line.strip_prefix("theme=") {
                if let Ok(n) = v.trim().parse() {
                    c.theme = n;
                }
            } else if let Some(v) = line.strip_prefix("pos=") {
                let mut it = v.split(',').map(|n| n.trim().parse::<f32>());
                if let (Some(Ok(x)), Some(Ok(y))) = (it.next(), it.next()) {
                    c.pos = Some((x, y));
                }
            }
        }
        c
    }

    pub fn save(&self) {
        let mut s = format!("theme={}\n", self.theme);
        if let Some((x, y)) = self.pos {
            s.push_str(&format!("pos={x},{y}\n"));
        }
        let _ = atomic_save(&config_path(), &s);
    }
}
