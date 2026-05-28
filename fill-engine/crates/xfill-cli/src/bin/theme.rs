//! Theme-aware generator: given theme answers, auto-place them as symmetric
//! across entries, generate valid themed grids, lock the answers in, and fill
//! around them in parallel — keeping the cleanest fill.
//!
//! Usage: theme --theme ANSWER [--theme ANSWER ...] [--blocks N] [--candidates N]
//!              [--time SECS] [--seed N] [--workers N] [--wordlist PATH]

use std::collections::HashSet;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Mutex;
use std::time::Instant;
use xfill_core::gen;
use xfill_core::grid::Dir;
use xfill_core::library::{build_lib_grid, write_json, LibGrid};
use xfill_core::util::Rng;
use xfill_core::{Lock, Puzzle, SolveConfig, Solver, Wordlist};

fn opt<T: std::str::FromStr>(name: &str, default: T) -> T {
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
    // collect repeated --theme answers (uppercased, A-Z only)
    let mut themes: Vec<String> = Vec::new();
    let mut it = std::env::args().skip(1);
    while let Some(a) = it.next() {
        if a == "--theme" {
            if let Some(v) = it.next() {
                themes.push(v.to_ascii_uppercase());
            }
        }
    }
    if themes.is_empty() {
        eprintln!("usage: theme --theme ANSWER [--theme ANSWER ...] [--blocks N] [--candidates N] [--time SECS] [--seed N] [--workers N]");
        std::process::exit(2);
    }
    for t in &themes {
        if !t.chars().all(|c| c.is_ascii_uppercase()) {
            eprintln!("theme answer '{t}' must be letters A-Z only");
            std::process::exit(2);
        }
    }

    let wordlist: String = opt("--wordlist", "data/xwordlist.dict".to_string());
    let blocks: usize = opt("--blocks", 40);
    let candidates: usize = opt("--candidates", 100);
    let time: f64 = opt("--time", 3.0);
    let seed: u64 = opt("--seed", 1);
    let keep_mean: f64 = opt("--keep-mean", 68.0);
    let max_iffy: usize = opt("--max-iffy", 18);
    let top: usize = opt("--top", 10);
    let out: String = opt("--out", "../out/libraries/theme-library.json".to_string());
    let workers: usize = opt(
        "--workers",
        std::thread::available_parallelism()
            .map(|n| n.get())
            .unwrap_or(1),
    );

    // Auto-place theme answers as across slots.
    let lengths: Vec<usize> = themes.iter().map(|t| t.len()).collect();
    let placements = match gen::place_themes(15, &lengths) {
        Some(p) => p,
        None => {
            eprintln!(
                "could not place {} theme answers of lengths {:?} (supported: 1-4 themes; mirror-row pairs need equal lengths)",
                themes.len(), lengths
            );
            std::process::exit(1);
        }
    };
    eprintln!("theme placements (row,col,len):");
    for (i, &(r, c, l)) in placements.iter().enumerate() {
        eprintln!("  {} -> r{r}c{c} A len{l}   {}", themes[i], themes[i]);
    }

    let t = Instant::now();
    let wl = Wordlist::load(&wordlist, 40).expect("load wordlist");
    eprintln!("wordlist loaded in {:.1}s", t.elapsed().as_secs_f64());

    // Phase 1: generate valid themed grids (deterministic, single-thread).
    let mut rng = Rng::new(seed);
    let mut jobs: Vec<(String, u64)> = Vec::new();
    let mut tries = 0;
    while jobs.len() < candidates && tries < candidates * 40 {
        tries += 1;
        let jseed = rng.next_u64();
        if let Some(tpl) = gen::generate_themed(15, &placements, blocks, &mut rng, 200) {
            jobs.push((tpl, jseed));
        }
    }
    if jobs.is_empty() {
        eprintln!(
            "failed to generate any themed grid at {blocks} blocks — try a different --blocks"
        );
        std::process::exit(1);
    }
    eprintln!(
        "generated {} themed grids; filling with {workers} workers...",
        jobs.len()
    );

    // Locks: lock each theme answer into its across slot.
    let locks: Vec<Lock> = placements
        .iter()
        .enumerate()
        .map(|(i, &(r, c, _))| Lock {
            row: r,
            col: c,
            dir: Dir::Across,
            answer: themes[i].clone(),
        })
        .collect();

    // Phase 2: parallel fill; keep clean themed fills for the library, plus the
    // single best fill (regardless of threshold) to display.
    let next = AtomicUsize::new(0);
    let done = AtomicUsize::new(0);
    let filled = AtomicUsize::new(0);
    let kept: Mutex<Vec<LibGrid>> = Mutex::new(Vec::new());
    let best_boxed: Mutex<Option<(f64, usize, String)>> = Mutex::new(None);
    let t0 = Instant::now();

    std::thread::scope(|s| {
        for _ in 0..workers {
            s.spawn(|| loop {
                let i = next.fetch_add(1, Ordering::Relaxed);
                if i >= jobs.len() {
                    break;
                }
                let (tpl, jseed) = &jobs[i];
                let p = Puzzle::from_template(tpl);
                let mut solver = match Solver::with_locks(&p, &wl, &locks) {
                    Ok(s) => s,
                    Err(_) => continue,
                };
                // Identify the locked theme entry ids for this grid.
                let theme_ids: HashSet<usize> = placements
                    .iter()
                    .filter_map(|&(r, c, _)| p.find_entry(r, c, Dir::Across))
                    .collect();
                let cfg = SolveConfig {
                    time_limit_s: time,
                    tiers: vec![40, 50],
                    seed: *jseed,
                    ..Default::default()
                };
                let r = solver.solve(&cfg);
                let d = done.fetch_add(1, Ordering::Relaxed) + 1;
                if d.is_multiple_of(20) {
                    eprintln!("  filled {d}/{} ...", jobs.len());
                }
                let Some(g) = build_lib_grid(&p, &r, &theme_ids) else {
                    continue;
                };
                filled.fetch_add(1, Ordering::Relaxed);
                {
                    let boxed = p.render_boxed(r.letters.as_deref());
                    let mut b = best_boxed.lock().unwrap();
                    if b.as_ref().is_none_or(|(m, _, _)| g.mean > *m) {
                        *b = Some((g.mean, g.iffy, boxed));
                    }
                }
                if g.mean >= keep_mean && g.iffy <= max_iffy {
                    kept.lock().unwrap().push(g);
                }
            });
        }
    });
    let dur = t0.elapsed().as_secs_f64();

    // Dedup + rank the kept clean grids, write the themed library.
    let mut kept = kept.into_inner().unwrap();
    let mut seen = HashSet::new();
    kept.retain(|g| seen.insert(g.template.join("\n")));
    kept.sort_by(|a, b| b.mean.partial_cmp(&a.mean).unwrap());
    kept.truncate(top);
    write_json(&out, &kept, &wordlist, blocks, &themes).expect("write themed library");

    let filled_n = filled.load(Ordering::Relaxed);
    println!("\n===== THEMED: {} answers, {blocks} blocks, {} grids, {time}s each, {workers} workers =====", themes.len(), jobs.len());
    println!("filled {filled_n}/{} themed grids in {dur:.1}s", jobs.len());
    println!(
        "clean grids kept (mean>={keep_mean}, iffy<={max_iffy}): {} -> wrote {out}",
        kept.len()
    );
    match best_boxed.into_inner().unwrap() {
        Some((mean, iffy, boxed)) => {
            println!(
                "best themed fill: mean={mean:.1} iffy(<50)={iffy} (over non-theme entries)\n"
            );
            println!("{boxed}");
        }
        None => println!(
            "no themed grid could be filled — try more --candidates or a different --blocks"
        ),
    }
}
