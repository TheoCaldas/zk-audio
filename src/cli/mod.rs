//! Interactive CLI for the three roles in the protocol: mock-sign a source,
//! prove an edit, verify a bundle.
//!
//! Run with `cargo run --release --bin zk-audio-cli`. Proving is slow, so a
//! release build matters. See README.md.

pub mod bundle;
pub mod chain;
mod prompt;
mod sign;
mod prove;
pub mod verify;

use std::time::Instant;

/// Wall-clock timer for the phases the CLI reports.
///
/// Timings are logged to the terminal and deliberately kept out of the proof
/// bundle: they are not part of the statement. The bench suite is where
/// timings are recorded to disk.
pub struct Timer(Instant);

impl Timer {
    pub fn start() -> Self {
        Self(Instant::now())
    }

    /// Elapsed time, formatted for a log line.
    pub fn elapsed(&self) -> String {
        fmt_secs(self.secs())
    }

    pub fn secs(&self) -> f64 {
        self.0.elapsed().as_secs_f64()
    }
}

/// A duration formatted for a log line: "84ms", "3.2s", "2m05s".
pub fn fmt_secs(s: f64) -> String {
    if s < 1.0 {
        format!("{:.0}ms", s * 1000.0)
    } else if s < 90.0 {
        format!("{s:.1}s")
    } else {
        format!("{:.0}m{:02.0}s", s / 60.0, s % 60.0)
    }
}

/// The file stem, for naming outputs after their input.
pub fn stem(path: &str) -> String {
    std::path::Path::new(path)
        .file_stem()
        .map(|s| s.to_string_lossy().to_string())
        .unwrap_or_else(|| "output".to_string())
}

/// The most recently modified private key in `signers/`.
///
/// `generate.keys.sh` names keys by the day they were made, so the id cannot
/// be hardcoded as a default.
pub fn newest_key() -> Option<String> {
    let mut keys: Vec<_> = std::fs::read_dir("signers")
        .ok()?
        .flatten()
        .map(|e| e.path())
        .filter(|p| {
            p.file_name()
                .map(|n| {
                    let n = n.to_string_lossy();
                    n.starts_with("private_") && n.ends_with(".pem")
                })
                .unwrap_or(false)
        })
        .collect();
    keys.sort_by_key(|p| {
        std::fs::metadata(p)
            .and_then(|m| m.modified())
            .unwrap_or(std::time::SystemTime::UNIX_EPOCH)
    });
    keys.last().map(|p| p.to_string_lossy().to_string())
}

/// Runs the menu loop until the user exits or stdin closes.
pub fn run() {
    println!("zk-audio - proving audio edits without revealing the audio");

    loop {
        println!();
        println!("  A) mock-sign a source");
        println!("  B) prove an edit");
        println!("  C) verify a bundle");
        println!("  D) prove a chain of edits");
        println!("  E) verify a chain bundle");
        println!("  exit");

        let choice = match prompt::line("> ") {
            Some(c) => c,
            None => break,
        };

        let result = match choice.to_ascii_lowercase().as_str() {
            "a" => sign::run(),
            "b" => prove::run(),
            "c" => verify::run(),
            "d" => chain::prove(),
            "e" => chain::verify(),
            "exit" | "quit" | "q" => break,
            "" => continue,
            other => {
                println!("unknown option: {other}");
                continue;
            }
        };

        if let Err(e) = result {
            println!();
            println!("error: {e}");
        }
    }

    println!("bye");
}
