//! Build a curated daily-grid LIBRARY (themeless): generate many valid grids,
//! fill them in parallel, keep only the clean fills (high mean, few/no weak
//! entries), deduplicate, rank by quality, and emit a JSON artifact (template,
//! fill, numbered answers + scores). The vetted source the daily-puzzle pipeline
//! (clue writing, scheduling) draws from.
//!
//! Usage: library [--wordlist PATH] [--size N] [--blocks N] [--candidates N]
//!                [--time SECS] [--keep-mean F] [--max-iffy N] [--top N]
//!                [--seed N] [--workers N] [--out PATH]
//!
//! `--size` is the grid dimension (default 15; e.g. 5 or 10 for minis). The
//! default block count scales with the grid area when `--blocks` is omitted.

use std::collections::HashSet;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Mutex;
use std::time::Instant;
use xfill_core::library::{build_lib_grid, write_json, LibGrid};
use xfill_core::util::Rng;
use xfill_core::{gen, Puzzle, SolveConfig, Solver, Wordlist};

fn arg<T: std::str::FromStr>(name: &str, default: T) -> T {
    let mut it = std::env::args().skip(1);
    while let Some(a) = it.next() {
        if a == name {
            if let Some(v) = it.next() {
                if let Ok(p) = v.parse() {
                    return p;
                }
            }
        }
    }
    default
}

fn main() {
    let wordlist: String = arg("--wordlist", "data/xwordlist.dict".to_string());
    let size: usize = arg("--size", 15);
    // Default block count scales with grid area (~16%): 15→36, 10→16, 5→4.
    let blocks: usize = arg("--blocks", size * size * 16 / 100);
    let candidates: usize = arg("--candidates", 200);
    let time: f64 = arg("--time", 2.0);
    let keep_mean: f64 = arg("--keep-mean", 72.0);
    let max_iffy: usize = arg("--max-iffy", 3);
    let top: usize = arg("--top", 25);
    let seed: u64 = arg("--seed", 1);
    let out: String = arg("--out", "../out/libraries/grid-library.json".to_string());
    let workers: usize = arg(
        "--workers",
        std::thread::available_parallelism()
            .map(|n| n.get())
            .unwrap_or(1),
    );

    let t = Instant::now();
    let wl = Wordlist::load(&wordlist, 40).expect("load wordlist");
    eprintln!("wordlist loaded in {:.1}s", t.elapsed().as_secs_f64());

    // Phase 1: generate templates + fixed per-grid seeds (deterministic).
    let mut rng = Rng::new(seed);
    let mut jobs: Vec<(String, u64)> = Vec::with_capacity(candidates);
    // Bound the search so an infeasible size/blocks combo fails fast instead of
    // spinning forever (generate() returns None when it can't satisfy the spec).
    let mut tries = 0usize;
    let max_tries = candidates.saturating_mul(200).max(20_000);
    while jobs.len() < candidates && tries < max_tries {
        tries += 1;
        let jseed = rng.next_u64();
        let Some(tpl) = gen::generate(size, blocks, &mut rng, 4000) else {
            continue;
        };
        if Puzzle::from_template(&tpl).orphan_cells() > 0 {
            continue;
        }
        jobs.push((tpl, jseed));
    }
    if jobs.is_empty() {
        eprintln!(
            "error: could not generate any valid {size}x{size} grids with {blocks} blocks \
             after {tries} tries. Try fewer --blocks or a different --size."
        );
        std::process::exit(1);
    }
    eprintln!(
        "generated {} candidate {size}x{size} grids; filling with {workers} workers...",
        jobs.len()
    );

    // Phase 2: parallel fill, keep only clean fills. Workers stop early once
    // the library has plenty of clean grids (2x what we'll keep).
    let stop_at = top * 2;
    let next = AtomicUsize::new(0);
    let done = AtomicUsize::new(0);
    let kept: Mutex<Vec<LibGrid>> = Mutex::new(Vec::new());
    let kept_count = AtomicUsize::new(0);
    let filled = AtomicUsize::new(0);
    let no_theme: HashSet<usize> = HashSet::new();
    let t0 = Instant::now();

    std::thread::scope(|s| {
        for _ in 0..workers {
            s.spawn(|| loop {
                if kept_count.load(Ordering::Relaxed) >= stop_at {
                    break;
                }
                let i = next.fetch_add(1, Ordering::Relaxed);
                if i >= jobs.len() {
                    break;
                }
                let (tpl, jseed) = &jobs[i];
                let p = Puzzle::from_template(tpl);
                let cfg = SolveConfig {
                    time_limit_s: time,
                    tiers: vec![40, 50],
                    seed: *jseed,
                    ..Default::default()
                };
                let r = Solver::new(&p, &wl).solve(&cfg);
                let d = done.fetch_add(1, Ordering::Relaxed) + 1;
                if d.is_multiple_of(50) {
                    eprintln!("  filled {d}/{} ...", jobs.len());
                }
                let Some(g) = build_lib_grid(&p, &r, &no_theme) else {
                    continue;
                };
                filled.fetch_add(1, Ordering::Relaxed);
                if g.mean < keep_mean || g.iffy > max_iffy {
                    continue;
                }
                kept.lock().unwrap().push(g);
                kept_count.fetch_add(1, Ordering::Relaxed);
            });
        }
    });
    let dur = t0.elapsed().as_secs_f64();

    let mut kept = kept.into_inner().unwrap();
    let mut seen = HashSet::new();
    kept.retain(|g| seen.insert(g.template.join("\n")));
    kept.sort_by(|a, b| b.mean.partial_cmp(&a.mean).unwrap());
    kept.truncate(top);

    let filled_n = filled.load(Ordering::Relaxed);
    let done_n = done.load(Ordering::Relaxed);
    eprintln!(
        "\nfilled {filled_n}/{done_n} in {dur:.1}s; {} clean grids kept (mean>={keep_mean}, iffy<={max_iffy})",
        kept.len()
    );
    if done_n < jobs.len() {
        eprintln!(
            "(early stop: {stop_at} clean grids kept after {done_n} of {} candidates)",
            jobs.len()
        );
    }

    write_json(&out, &kept, &wordlist, blocks, &[]).expect("write library file");
    eprintln!("wrote {} grids to {out}", kept.len());
    if let Some(best) = kept.first() {
        eprintln!(
            "best: mean={:.1} iffy={} blocks={}",
            best.mean, best.iffy, best.blocks
        );
    }
}
