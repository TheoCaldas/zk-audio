//! Terminal prompt helpers. Thin by design; the operations live elsewhere.

use std::io::{self, Write};

/// Reads one trimmed line. `None` on EOF, so a piped stdin ends the session
/// instead of looping forever.
pub fn line(question: &str) -> Option<String> {
    print!("{question}");
    io::stdout().flush().ok();
    let mut buf = String::new();
    match io::stdin().read_line(&mut buf) {
        Ok(0) => None,
        Ok(_) => Some(buf.trim().to_string()),
        Err(_) => None,
    }
}

/// Reads a non-empty line, re-prompting while it is blank.
pub fn text(question: &str) -> Option<String> {
    loop {
        let v = line(question)?;
        if !v.is_empty() {
            return Some(v);
        }
        println!("  (required)");
    }
}

/// Reads a line, substituting `default` when blank.
pub fn text_or(question: &str, default: &str) -> Option<String> {
    let v = line(&format!("{question} [{default}]: "))?;
    Some(if v.is_empty() { default.to_string() } else { v })
}

/// Reads a path that must exist, re-prompting otherwise.
pub fn existing_path(question: &str) -> Option<String> {
    loop {
        let v = text(question)?;
        let p = v.trim_matches('"').trim_matches('\'').to_string();
        if std::path::Path::new(&p).exists() {
            return Some(p);
        }
        println!("  no such file: {p}");
    }
}

/// Reads a usize, substituting `default` when blank.
pub fn usize_or(question: &str, default: usize) -> Option<usize> {
    loop {
        let v = line(&format!("{question} [{default}]: "))?;
        if v.is_empty() {
            return Some(default);
        }
        match v.parse() {
            Ok(n) => return Some(n),
            Err(_) => println!("  not a number: {v}"),
        }
    }
}

/// Yes/no, defaulting to no.
pub fn confirm(question: &str) -> Option<bool> {
    let v = line(&format!("{question} [y/N]: "))?;
    Some(matches!(v.to_ascii_lowercase().as_str(), "y" | "yes"))
}

/// Presents a numbered list and returns the chosen index.
pub fn pick(question: &str, options: &[String]) -> Option<usize> {
    println!("{question}");
    for (i, o) in options.iter().enumerate() {
        println!("  {:>2}) {o}", i + 1);
    }
    loop {
        let v = text("> ")?;
        if let Ok(n) = v.parse::<usize>() {
            if n >= 1 && n <= options.len() {
                return Some(n - 1);
            }
        }
        println!("  enter 1-{}", options.len());
    }
}
