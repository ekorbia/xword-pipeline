//! Grid screener: generate many valid grids, fill each under a short budget,
//! and report the quality distribution. Demonstrates the product thesis — most
//! random grids fill ugly, but the best ones fill cleanly, so we generate-many
//! and keep the clean fills.
//!
//! Parallel (M3): grids are filled across worker threads via atomic
//! work-stealing. Per-grid seeds are fixed up front, so results are
//! deterministic regardless of how work is scheduled.
//!
//! Usage: screen [--wordlist PATH] [--size N] [--blocks N] [--count N]
//!               [--time SECS] [--seed N] [--keep-mean F] [--workers N]

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Mutex;
use std::time::Instant;
use xfill_core::gen;
use xfill_core::util::Rng;
use xfill_core::{Puzzle, SolveConfig, Solver, Wordlist};

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

struct Outcome {
    mean: f64,
    iffy: usize,
    tpl: String,
    render: String,
}

fn main() {
    let wordlist: String = arg("--wordlist", "data/xwordlist.dict".to_string());
    let size: usize = arg("--size", 15);
    // Default block count scales with grid area (~16%): 15→36, 10→16, 5→4.
    let blocks: usize = arg("--blocks", size * size * 16 / 100);
    let count: usize = arg("--count", 50);
    let time: f64 = arg("--time", 2.0);
    let seed: u64 = arg("--seed", 1);
    let keep_mean: f64 = arg("--keep-mean", 68.0);
    let workers: usize = arg(
        "--workers",
        std::thread::available_parallelism()
            .map(|n| n.get())
            .unwrap_or(1),
    );

    let t = Instant::now();
    let wl = Wordlist::load(&wordlist, 40).expect("load wordlist");
    eprintln!("wordlist loaded in {:.1}s", t.elapsed().as_secs_f64());

    // Phase 1 (single-thread, deterministic): generate templates + fixed seeds.
    let mut rng = Rng::new(seed);
    let mut jobs: Vec<(String, u64)> = Vec::with_capacity(count);
    // Bound the search so an infeasible size/blocks combo fails fast.
    let mut tries = 0usize;
    let max_tries = count.saturating_mul(200).max(20_000);
    while jobs.len() < count && tries < max_tries {
        tries += 1;
        let attempt_seed = rng.next_u64();
        let Some(tpl) = gen::generate(size, blocks, &mut rng, 4000) else {
            continue;
        };
        // skip structurally degenerate grids early
        if Puzzle::from_template(&tpl).orphan_cells() > 0 {
            continue;
        }
        jobs.push((tpl, attempt_seed));
    }
    if jobs.is_empty() {
        eprintln!(
            "error: could not generate any valid {size}x{size} grids with {blocks} blocks \
             after {tries} tries. Try fewer --blocks or a different --size."
        );
        std::process::exit(1);
    }
    eprintln!(
        "generated {} valid {size}x{size} grids; filling with {workers} workers...",
        jobs.len()
    );

    // Phase 2 (parallel): work-steal across grids.
    let next = AtomicUsize::new(0);
    let done = AtomicUsize::new(0);
    let results: Mutex<Vec<Outcome>> = Mutex::new(Vec::new());
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
                let mut solver = Solver::new(&p, &wl);
                let cfg = SolveConfig {
                    time_limit_s: time,
                    tiers: vec![40, 50],
                    seed: *jseed,
                    ..Default::default()
                };
                let r = solver.solve(&cfg);
                if let (Some(mean), Some(iffy)) = (r.mean_score, r.iffy_count) {
                    let render = p.render_boxed(r.letters.as_deref());
                    results.lock().unwrap().push(Outcome {
                        mean,
                        iffy,
                        tpl: tpl.clone(),
                        render,
                    });
                }
                let d = done.fetch_add(1, Ordering::Relaxed) + 1;
                if d.is_multiple_of(25) {
                    eprintln!("  filled {d}/{} ...", jobs.len());
                }
            });
        }
    });
    let dur = t0.elapsed().as_secs_f64();

    let outcomes = results.into_inner().unwrap();
    let filled = outcomes.len();
    let mut means: Vec<f64> = outcomes.iter().map(|o| o.mean).collect();
    means.sort_by(|a, b| a.partial_cmp(b).unwrap());
    let keepers = means.iter().filter(|&&m| m >= keep_mean).count();
    let best = outcomes
        .iter()
        .max_by(|a, b| a.mean.partial_cmp(&b.mean).unwrap());

    let pct = |q: f64| -> f64 {
        if means.is_empty() {
            return 0.0;
        }
        means[(((means.len() - 1) as f64) * q).round() as usize]
    };

    println!(
        "\n===== SCREEN: {count} grids @ {blocks} blocks, {time}s budget, {workers} workers ====="
    );
    println!(
        "filled in {dur:.1}s wall ({:.1} grids/sec)",
        count as f64 / dur
    );
    println!(
        "filled: {filled}/{count} ({:.0}%)",
        100.0 * filled as f64 / count as f64
    );
    if !means.is_empty() {
        println!(
            "mean-score distribution: min={:.1}  p25={:.1}  median={:.1}  p75={:.1}  p90={:.1}  max={:.1}",
            means[0], pct(0.25), pct(0.5), pct(0.75), pct(0.9), means[means.len() - 1]
        );
        println!(
            "keepers (mean >= {keep_mean}): {keepers} ({:.0}% of filled)",
            100.0 * keepers as f64 / filled as f64
        );
    }
    if let Some(b) = best {
        println!(
            "\nbest grid found: mean={:.1}  iffy(<50)={}",
            b.mean, b.iffy
        );
        println!("\n{}", b.render);
        println!("reusable template:\n{}", b.tpl.trim_end());
    }
}
