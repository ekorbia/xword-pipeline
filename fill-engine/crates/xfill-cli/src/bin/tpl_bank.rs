//! Build (or grow) the template BANK: probe random full-size templates and
//! persist the ones that prove they clean-fill. See `xfill_core::bank` for
//! why: per-pattern clean-fill rates are bimodal, and reusing the proven
//! minority lifts publication keeper rates ~6.4× (2026-08 study, held-out).
//!
//! Usage: tpl_bank [--wordlist PATH] [--size 15] [--candidates 1000]
//!                 [--probes 2] [--time 1.0] [--blocks N] [--block-jitter N]
//!                 [--min-words N] [--seed N] [--workers N]
//!                 [--bank data/tpl-bank-15.txt]
//!
//! Appends keepers (templates with >= 1 clean probe fill, deduped against the
//! existing bank) with `words=`/`cleans=` metadata. Deterministic per seed.
//! Defaults span the desktop Publication reality: auto blocks + jitter 4
//! covers 36–40 blocks at 15×15, word floor per `gen::word_floor`.

use std::collections::HashSet;
use std::fs;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Mutex;
use std::time::Instant;
use xfill_core::util::Rng;
use xfill_core::{bank, gen, Puzzle, SolveConfig, Solver, Wordlist};

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
    let candidates: usize = arg("--candidates", 1000);
    // Probes per template: independent seeds; one clean fill proves the shape.
    let probes: usize = arg("--probes", 2);
    let time: f64 = arg("--time", 1.0);
    let blocks_arg: i64 = arg("--blocks", -1);
    let pinned = blocks_arg >= 0;
    let blocks: usize = if pinned {
        blocks_arg as usize
    } else {
        size * size * 16 / 100
    };
    // Wider-than-generation jitter so the bank spans the whole realistic
    // density range (36-40 at 15x15, incl. the Publication +2 bump).
    let block_jitter: usize = arg("--block-jitter", if pinned { 0 } else { 4 });
    let min_words_arg: i64 = arg("--min-words", -1);
    let min_words: usize = if min_words_arg >= 0 {
        min_words_arg as usize
    } else {
        gen::word_floor(size)
    };
    let seed: u64 = arg("--seed", 1);
    let workers: usize = arg(
        "--workers",
        std::thread::available_parallelism()
            .map(|n| n.get())
            .unwrap_or(1),
    );
    let bank_path: String = arg("--bank", format!("data/tpl-bank-{size}.txt"));

    let t = Instant::now();
    let wl = Wordlist::load(&wordlist, 40).expect("load wordlist");
    eprintln!("wordlist loaded in {:.1}s", t.elapsed().as_secs_f64());

    // Existing bank -> dedupe set (also dedupes within this run).
    let existing_text = fs::read_to_string(&bank_path).unwrap_or_default();
    let existing = bank::parse_bank(&existing_text, size);
    let mut seen: HashSet<String> = existing.iter().cloned().collect();
    eprintln!("bank: {} existing templates in {bank_path}", existing.len());

    // Generate distinct candidate templates (word floor, no relaxation — a
    // bank build WANTS only floor-passing shapes; short generation just means
    // fewer probes this run).
    let mut rng = Rng::new(seed);
    let mut jobs: Vec<(String, u64)> = Vec::with_capacity(candidates);
    let mut tries = 0usize;
    let max_tries = candidates.saturating_mul(200).max(20_000);
    while jobs.len() < candidates && tries < max_tries {
        tries += 1;
        let jseed = rng.next_u64();
        let b = gen::jittered_blocks(blocks, block_jitter, &mut rng);
        let Some(tpl) = gen::generate(size, b, &mut rng, 4000) else {
            continue;
        };
        let p = Puzzle::from_template(&tpl);
        if p.orphan_cells() > 0 || p.entries.len() < min_words {
            continue;
        }
        if !seen.insert(tpl.clone()) {
            continue; // already banked or already queued this run
        }
        jobs.push((tpl, jseed));
    }
    eprintln!(
        "probing {} fresh candidates (floor {min_words}, {probes}x{time:.1}s) with {workers} workers...",
        jobs.len()
    );

    let next = AtomicUsize::new(0);
    let done = AtomicUsize::new(0);
    let kept: Mutex<Vec<(String, usize, usize)>> = Mutex::new(Vec::new()); // (tpl, words, cleans)
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
                let mut cleans = 0usize;
                for k in 0..probes {
                    let mut solver = Solver::new(&p, &wl);
                    let cfg = SolveConfig {
                        time_limit_s: time,
                        seed: jseed.wrapping_add(k as u64),
                        ..Default::default()
                    };
                    if solver.solve(&cfg).clean.is_some() {
                        cleans += 1;
                    }
                }
                if cleans > 0 {
                    kept.lock()
                        .unwrap()
                        .push((tpl.clone(), p.entries.len(), cleans));
                }
                let d = done.fetch_add(1, Ordering::Relaxed) + 1;
                if d.is_multiple_of(100) {
                    eprintln!("  probed {d}/{} ...", jobs.len());
                }
            });
        }
    });

    let mut kept = kept.into_inner().unwrap();
    kept.sort(); // deterministic file order per seed
    let dur = t0.elapsed().as_secs_f64();
    eprintln!(
        "probed {} in {dur:.1}s; {} clean-proven ({:.0}%)",
        jobs.len(),
        kept.len(),
        100.0 * kept.len() as f64 / jobs.len().max(1) as f64
    );

    let mut out = existing_text;
    if out.is_empty() {
        out.push_str(
            "# tpl-bank — templates proven to clean-fill (see xfill_core::bank docs).\n\
             # GENERATED by the tpl-bank bin; safe to hand-prune. rows joined by '/'.\n",
        );
    }
    if !out.is_empty() && !out.ends_with('\n') {
        out.push('\n');
    }
    for (tpl, words, cleans) in &kept {
        out.push_str(&bank::encode_line(tpl, *words, *cleans));
        out.push('\n');
    }
    fs::write(&bank_path, &out).expect("write bank");
    eprintln!(
        "bank now {} templates -> {bank_path}",
        existing.len() + kept.len()
    );
}
