//! Benchmark sweep CLI.
//!
//! cargo run --release --bin bench -- --max-n 6000 --batch-sizes 10,100

use zk_audio::bench::{
    config::{registry, SuiteConfig, SUPPORTED_BIT_DEPTHS},
    run_suite,
};

const USAGE: &str = "\
zk-audio benchmark suite

USAGE:
    cargo run --release --bin bench -- --max-n <N> [OPTIONS]
    cargo run --release --bin bench -- --durations <secs,...> [OPTIONS]

REQUIRED (one of):
    --max-n <N>              Cap for the N ladder (60, 600, 6000, ...).
                             Drives total runtime, so there is no default.
    --durations <list>       Comma-separated audio durations in SECONDS.
                             N is computed as secs * sample-rate * channels,
                             replacing the ladder entirely. Fractions allowed.

OPTIONS:
    --sample-rate <hz>       Rate assumed by --durations  [default: 44100]
    --channels <n>           Channels assumed by --durations        [default: 2]
    --batch-sizes <list>     Comma-separated B values    [default: 10,40,100,200]
    --bit-depth <n>          Sample bit depth            [default: 16]
    --input <path>           WAV input
                             [default: raw_audios/2c_44sr_16bd_01.wav]
    --edit <name>            Edit to prove               [default: gain2]
    --out <dir>              Output root                 [default: logs/bench]
    --repeat <n>             Repetitions per case        [default: 3]
    --skip-compressed        Recursive phase only (fast sweeps)
    --no-cache-inputs        Delete each serialized input after its case
    --dry-run                Print the case matrix and exit
    -h, --help               Show this help

NOTES:
    A clean case needs N divisible by B * comp_size (comp_size = 15 at b=16).
    B=150 never divides the 6*10^k ladder; use B in {10,40,100,200}.

    --durations counts every channel, since the circuit folds over the
    interleaved stream: 1 s of 44.1 kHz stereo is 88200 samples, not 44100.
    It errors out if the WAV's real rate/channels disagree with the assumed
    ones, rather than quietly proving a different length than reported.
";

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();

    if args.iter().any(|a| a == "-h" || a == "--help") {
        print!("{}", USAGE);
        return;
    }

    match parse(&args) {
        Ok(cfg) => {
            if let Err(e) = run_suite(&cfg) {
                eprintln!("error: {}", e);
                std::process::exit(1);
            }
        }
        Err(e) => {
            eprintln!("error: {}\n", e);
            eprint!("{}", USAGE);
            std::process::exit(2);
        }
    }
}

fn parse(args: &[String]) -> Result<SuiteConfig, String> {
    let mut cfg = SuiteConfig::default();
    let mut saw_max_n = false;
    let mut i = 0;

    while i < args.len() {
        let arg = args[i].as_str();
        match arg {
            "--skip-compressed" => cfg.skip_compressed = true,
            "--no-cache-inputs" => cfg.cache_inputs = false,
            "--dry-run" => cfg.dry_run = true,
            "--max-n" => {
                cfg.max_n = parse_usize(next(args, &mut i, arg)?, arg)?;
                saw_max_n = true;
            }
            "--durations" => {
                let raw = next(args, &mut i, arg)?;
                let mut v = Vec::new();
                for part in raw.split(',') {
                    let part = part.trim();
                    if part.is_empty() {
                        continue;
                    }
                    let d = part
                        .parse::<f64>()
                        .map_err(|_| format!("{}: '{}' is not a number", arg, part))?;
                    if !d.is_finite() || d <= 0.0 {
                        return Err("--durations entries must be positive".to_string());
                    }
                    v.push(d);
                }
                if v.is_empty() {
                    return Err("--durations needs at least one value".to_string());
                }
                cfg.durations = v;
            }
            "--sample-rate" => {
                let sr = parse_usize(next(args, &mut i, arg)?, arg)?;
                if sr == 0 {
                    return Err("--sample-rate must be > 0".to_string());
                }
                cfg.sample_rate = sr;
            }
            "--channels" => {
                let ch = parse_usize(next(args, &mut i, arg)?, arg)?;
                if ch == 0 {
                    return Err("--channels must be > 0".to_string());
                }
                cfg.channels = ch;
            }
            "--batch-sizes" => {
                let raw = next(args, &mut i, arg)?;
                let mut v = Vec::new();
                for part in raw.split(',') {
                    let part = part.trim();
                    if part.is_empty() {
                        continue;
                    }
                    let b = parse_usize(part, arg)?;
                    if b == 0 {
                        return Err("--batch-sizes entries must be > 0".to_string());
                    }
                    v.push(b);
                }
                if v.is_empty() {
                    return Err("--batch-sizes needs at least one value".to_string());
                }
                v.sort_unstable();
                v.dedup();
                cfg.batch_sizes = v;
            }
            "--bit-depth" => {
                let bd = parse_usize(next(args, &mut i, arg)?, arg)?;
                if !SUPPORTED_BIT_DEPTHS.contains(&bd) {
                    return Err(format!(
                        "bit-depth {} not supported yet (compiled: {:?})",
                        bd, SUPPORTED_BIT_DEPTHS
                    ));
                }
                cfg.bit_depth = bd;
            }
            "--input" => cfg.audio_input = next(args, &mut i, arg)?.to_string(),
            "--edit" => {
                let name = next(args, &mut i, arg)?.to_string();
                if registry().iter().all(|e| e.name != name) {
                    return Err(format!(
                        "unknown edit '{}'; available: {:?}",
                        name,
                        registry().iter().map(|e| e.name).collect::<Vec<_>>()
                    ));
                }
                cfg.edit = name;
            }
            "--out" => cfg.out_dir = next(args, &mut i, arg)?.to_string(),
            "--repeat" => {
                let r = parse_usize(next(args, &mut i, arg)?, arg)?;
                if r == 0 {
                    return Err("--repeat must be at least 1".to_string());
                }
                cfg.repeat = r;
            }
            other => return Err(format!("unrecognized argument '{}'", other)),
        }
        i += 1;
    }

    if !saw_max_n && cfg.durations.is_empty() {
        return Err("--max-n or --durations is required".to_string());
    }
    if saw_max_n && !cfg.durations.is_empty() {
        return Err("--max-n and --durations are mutually exclusive".to_string());
    }
    if saw_max_n && cfg.max_n < 60 {
        return Err("--max-n must be at least 60 (the first ladder step)".to_string());
    }

    Ok(cfg)
}

fn next<'a>(args: &'a [String], i: &mut usize, flag: &str) -> Result<&'a str, String> {
    *i += 1;
    args.get(*i)
        .map(|s| s.as_str())
        .ok_or_else(|| format!("{} requires a value", flag))
}

fn parse_usize(s: &str, flag: &str) -> Result<usize, String> {
    s.parse::<usize>()
        .map_err(|_| format!("{}: '{}' is not a non-negative integer", flag, s))
}
