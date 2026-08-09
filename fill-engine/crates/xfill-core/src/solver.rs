//! M2 solver: bitset-domain DFS with dynamic MRV + score-ordered value
//! selection, wrapped in random restarts, quality tiering, and best-of search.
//!
//! Why restarts: pure DFS gets trapped in a doomed subtree near the root and
//! grinds millions of nodes without backtracking far enough to escape. Each
//! attempt runs under a node budget; on exhaustion we restart from scratch with
//! a fresh RNG, and value ordering is shuffled within equal-score bands so each
//! attempt explores a different region while still preferring high-score words.
//!
//! Quality tiering: an outer loop first tries to fill using only words scoring
//! \>= a high floor (a tier = an index cutoff, since words are score-sorted);
//! the floor is relaxed only if a tier can't solve in its time slice. Across all
//! restarts the best fill (highest mean answer score) is retained.

use crate::bitset::Bitset;
use crate::grid::{Dir, Puzzle};
use crate::util::Rng;
use crate::wordlist::Wordlist;
use std::time::Instant;

/// A theme / pre-placed answer: lock the entry starting at (row, col) in `dir`
/// to `answer`. The answer need NOT be in the wordlist — locked entries are
/// excluded from the search and only propagate their letters as constraints.
#[derive(Clone, Debug)]
pub struct Lock {
    pub row: usize,
    pub col: usize,
    pub dir: Dir,
    pub answer: String,
}

#[derive(Clone)]
pub struct SolveConfig {
    pub time_limit_s: f64,
    /// Quality floors tried high→low. A floor of 50 forbids "questionable"
    /// (<50) entries; relaxing to 40 permits glue.
    pub tiers: Vec<u8>,
    pub seed: u64,
    pub initial_budget: u64,
    pub max_budget: u64,
    /// Max candidates materialized per node (cap for band-shuffle cost).
    pub cand_cap: usize,
    /// Return the first solution found (skip best-of optimization).
    pub stop_on_first: bool,
    /// A fill whose every searched entry scores >= this is "clean"; the best
    /// clean fill (by mean) is tracked and returned alongside the overall
    /// best so callers can prefer it when it passes their gates. 0 disables.
    pub clean_floor: u8,
    /// Post-solve region repair: if the search phases end with no clean fill
    /// (or only a worse-than-best one) and the best fill has 1..=this many
    /// searched entries below `clean_floor`, rip those entries plus their
    /// crossings out, lock the rest of the grid, and re-solve just that
    /// region at the clean floor on a small budget. Beyond ~4 the region
    /// approaches a whole-grid re-solve (each weak word drags 3-5 crossings
    /// with it), which the clean hunt already attempted. 0 disables.
    pub repair_max_weak: usize,
    /// Entries scoring below this are WEAK — real words, above the clean
    /// floor, but the gluey tail an editor counts (NYT tolerates a few per
    /// grid). Distinct from `clean_floor` (hard admissibility) and iffy
    /// (<50): the weak bar measures the tail that mean-score hides. Fills
    /// track `weak_count`, and the clean track prefers FEWER weak entries
    /// before higher mean. 0 disables (count is always 0). Callers surface
    /// this as a tunable; `DEFAULT_WEAK_BAR` is the pipeline default.
    pub weak_bar: u8,
}

/// Default weak bar: 70 sits above the capped usage floors (<=55) and the
/// upstream list's gluey 60s, below its ordinary-word 80-90 bulk. Measured on
/// publication keepers 2026-08: the perceived-glue tail (EEE/ENE/STS class)
/// scored 60-65, ordinary fill 80+.
pub const DEFAULT_WEAK_BAR: u8 = 70;

/// Fraction of `time_limit_s` granted to the post-solve repair pass. Region
/// re-solves are tiny (a handful of entries, most letters pinned), so they
/// either succeed almost immediately or are infeasible.
const REPAIR_TIME_FRAC: f64 = 0.15;

impl Default for SolveConfig {
    fn default() -> Self {
        SolveConfig {
            time_limit_s: 30.0,
            // 70 caps the ladder so the search actively hunts fills with NO
            // weak (sub-70) entries before settling for merely-clean ones.
            tiers: vec![40, 50, 55, 60, 70],
            seed: 0,
            initial_budget: 2_000,
            max_budget: 400_000,
            cand_cap: 400,
            stop_on_first: false,
            clean_floor: 55,
            repair_max_weak: 4,
            weak_bar: DEFAULT_WEAK_BAR,
        }
    }
}

/// A complete fill with its quality stats. `letters` is per-cell (0..25);
/// `fill` lists (entry_id, answer, score) for searched entries followed by
/// locked theme answers. Stats cover the SEARCHED entries only.
#[derive(Clone)]
pub struct SolvedFill {
    pub letters: Vec<Option<u8>>,
    pub mean_score: f64,
    pub min_score: u8,
    pub iffy_count: usize,
    /// Searched entries scoring below `SolveConfig::weak_bar` — the gluey
    /// tail an editor would count. 0 when the bar is disabled.
    pub weak_count: usize,
    pub fill: Vec<(usize, String, u8)>,
}

pub struct SolveResult {
    pub letters: Option<Vec<Option<u8>>>,
    pub nodes: u64,
    pub restarts: u64,
    pub elapsed_s: f64,
    pub reason: &'static str, // "solved" | "unsolved"
    pub mean_score: Option<f64>,
    pub min_score: Option<u8>,
    pub iffy_count: Option<usize>,
    /// Sub-`weak_bar` entry count of the primary fill (see `SolvedFill`).
    pub weak_count: Option<usize>,
    pub fill: Option<Vec<(usize, String, u8)>>,
    /// Best fill with no searched entry below `cfg.clean_floor`, when one
    /// was found — "best" = fewest weak (sub-`weak_bar`) entries, then
    /// highest mean. May describe the same fill as the primary fields.
    /// Callers should prefer it whenever it passes their keep gates on its
    /// own — same yield, strictly fewer weak entries.
    pub clean: Option<SolvedFill>,
}

struct TrailFrame {
    entry: usize,
    word: usize,
    len: usize,
    nbrs: Vec<(usize, Bitset, u32)>,
    set_cells: Vec<usize>,
}

#[derive(PartialEq, Eq, Clone, Copy)]
enum Outcome {
    Solved,
    Exhausted,
    Budget,
    TimeLimit,
}

pub struct Solver<'a> {
    p: &'a Puzzle,
    wl: &'a Wordlist,
    base_domains: Vec<Bitset>, // domains after prefilled, reused each attempt
    domains: Vec<Bitset>,
    dom_count: Vec<u32>,
    cutoff_by_entry: Vec<usize>,
    /// per entry: (pos_in_entry, neighbor_entry, neighbor_pos) for each crossing
    crossings: Vec<Vec<(usize, usize, usize)>>,
    assigned: Vec<Option<usize>>,
    cell_letter: Vec<Option<u8>>,
    used: Vec<Bitset>,
    n_assigned: usize,
    // Theme / pre-placed support:
    is_locked: Vec<bool>,
    base_cell_letter: Vec<Option<u8>>, // template prefill + locked answers
    locked_used: Vec<(usize, usize)>,  // (len, word_idx) of locks present in wordlist
    locked_out: Vec<(usize, String, u8)>, // (entry_id, answer, score) for output
    n_searchable: usize,               // non-locked entries to fill
    /// (len, word_idx) the search must never assign — set via `ban_answer`
    /// (e.g. a root-duplicate member; re-solving without it rescues the grid).
    banned: Vec<(usize, usize)>,
    nodes: u64,
    attempt_budget: u64,
    deadline: Instant,
    floor: u8,
    cand_cap: usize,
    rng: Rng,
}

impl<'a> Solver<'a> {
    pub fn new(p: &'a Puzzle, wl: &'a Wordlist) -> Self {
        Self::with_locks(p, wl, &[]).expect("no locks cannot fail")
    }

    /// Construct a solver with theme / pre-placed answers locked into place.
    /// Returns an error if a lock doesn't resolve to an entry, the answer length
    /// mismatches, or two locks disagree on a shared cell.
    pub fn with_locks(p: &'a Puzzle, wl: &'a Wordlist, locks: &[Lock]) -> Result<Self, String> {
        let n = p.entries.len();
        let domains: Vec<Bitset> = p
            .entries
            .iter()
            .map(|e| Bitset::ones(wl.len_data(e.len).n.max(1)))
            .collect();
        let used: Vec<Bitset> = (0..=crate::wordlist::MAX_LEN)
            .map(|l| Bitset::zeros(wl.by_len[l].n.max(1)))
            .collect();
        // crossings: for each entry, the perpendicular neighbor at each cell.
        let mut crossings: Vec<Vec<(usize, usize, usize)>> = vec![Vec::new(); n];
        for (ei, cx) in crossings.iter_mut().enumerate() {
            for (pos, &cid) in p.entries[ei].cells.iter().enumerate() {
                for &(other, opos) in &p.cell_entries[cid] {
                    if other != ei {
                        cx.push((pos, other, opos));
                    }
                }
            }
        }

        // Resolve locks: build the combined prefill (template + locked answers),
        // mark locked entries, and record their wordlist indices (for dup
        // avoidance) and scores (for output).
        let mut is_locked = vec![false; n];
        let mut base_cell = p.prefilled.clone();
        let mut locked_used = Vec::new();
        let mut locked_out = Vec::new();
        for lk in locks {
            let ei = p
                .find_entry(lk.row, lk.col, lk.dir)
                .ok_or_else(|| format!("no entry at r{}c{} {:?}", lk.row, lk.col, lk.dir))?;
            let answer = lk.answer.to_ascii_uppercase();
            let mut letters: Vec<u8> = Vec::with_capacity(answer.len());
            for ch in answer.chars() {
                if !ch.is_ascii_uppercase() {
                    return Err(format!("answer '{}' has non-letter '{}'", answer, ch));
                }
                letters.push(ch as u8 - b'A');
            }
            let e = &p.entries[ei];
            if letters.len() != e.len {
                return Err(format!(
                    "answer '{}' (len {}) doesn't fit entry at r{}c{} {:?} (len {})",
                    answer,
                    letters.len(),
                    lk.row,
                    lk.col,
                    lk.dir,
                    e.len
                ));
            }
            is_locked[ei] = true;
            for (pos, &cid) in e.cells.iter().enumerate() {
                match base_cell[cid] {
                    Some(x) if x != letters[pos] => {
                        return Err(format!("lock conflict at cell r{}c{}", lk.row, lk.col));
                    }
                    _ => base_cell[cid] = Some(letters[pos]),
                }
            }
            let score = match wl.len_data(e.len).index_of(&letters) {
                Some(w) => {
                    locked_used.push((e.len, w));
                    wl.len_data(e.len).scores[w]
                }
                None => 90, // theme answer not in wordlist: treat as intentional/clean
            };
            locked_out.push((ei, answer, score));
        }
        let n_searchable = is_locked.iter().filter(|&&b| !b).count();

        let mut s = Solver {
            p,
            wl,
            base_domains: Vec::new(),
            domains,
            dom_count: vec![0; n],
            cutoff_by_entry: vec![0; n],
            crossings,
            assigned: vec![None; n],
            cell_letter: vec![None; p.n_cells],
            used,
            n_assigned: 0,
            is_locked,
            base_cell_letter: base_cell,
            locked_used,
            locked_out,
            n_searchable,
            banned: Vec::new(),
            nodes: 0,
            attempt_budget: u64::MAX,
            deadline: Instant::now(),
            floor: 0,
            cand_cap: 256,
            rng: Rng::new(0),
        };
        s.compute_base();
        Ok(s)
    }

    /// Exclude an answer from all subsequent solves on this solver (e.g. one
    /// member of a root-duplicate pair found post-solve — re-solving without
    /// it usually rescues the grid). Returns false if the answer isn't a
    /// searchable wordlist entry (unknown word, bad letters, or locked).
    pub fn ban_answer(&mut self, answer: &str) -> bool {
        let mut letters: Vec<u8> = Vec::with_capacity(answer.len());
        for ch in answer.chars() {
            let up = ch.to_ascii_uppercase();
            if !up.is_ascii_uppercase() {
                return false;
            }
            letters.push(up as u8 - b'A');
        }
        let len = letters.len();
        if !(crate::wordlist::MIN_LEN..=crate::wordlist::MAX_LEN).contains(&len) {
            return false;
        }
        let Some(w) = self.wl.len_data(len).index_of(&letters) else {
            return false;
        };
        if self.locked_used.contains(&(len, w)) {
            return false; // locked answers are placed, not searched — unbannable
        }
        if !self.banned.contains(&(len, w)) {
            self.banned.push((len, w));
        }
        true
    }

    /// Constrain every entry's domain by the combined prefill (template + locks)
    /// and snapshot as the per-attempt base.
    fn compute_base(&mut self) {
        for ei in 0..self.p.entries.len() {
            let e = &self.p.entries[ei];
            let ld = self.wl.len_data(e.len);
            for (pos, &cid) in e.cells.iter().enumerate() {
                if let Some(c) = self.base_cell_letter[cid] {
                    let mask = ld.compat(pos, c).clone();
                    self.domains[ei].and_assign(&mask);
                }
            }
        }
        self.base_domains = self.domains.clone();
    }

    pub fn solve(&mut self, cfg: &SolveConfig) -> SolveResult {
        self.cand_cap = cfg.cand_cap;
        self.rng = Rng::new(cfg.seed);
        let t0 = Instant::now();
        let global_deadline = t0 + std::time::Duration::from_secs_f64(cfg.time_limit_s);

        let mut best: Option<SolvedFill> = None;
        let mut clean: Option<SolvedFill> = None;
        let mut total_nodes = 0u64;
        let mut restarts = 0u64;

        // Tiers are quality floors. We work feasibility-first: the LOWEST floor
        // is most permissive and most likely to solve, so we secure a baseline
        // there before spending any time on stricter (higher-quality) floors —
        // this avoids the trap of burning the budget on an infeasible high
        // floor (almost every grid needs some sub-50 glue).
        let mut tiers = if cfg.tiers.is_empty() {
            vec![40u8]
        } else {
            cfg.tiers.clone()
        };
        tiers.sort_unstable();
        tiers.dedup();
        let lowest = tiers[0];

        // Phase 1 — feasibility: restart at the lowest floor until first solve.
        let mut budget = cfg.initial_budget;
        while Instant::now() < global_deadline && best.is_none() {
            restarts += 1;
            let seed = self.rng.next_u64();
            let outcome = self.attempt(lowest, budget, seed, global_deadline);
            total_nodes += self.nodes;
            if outcome == Outcome::Solved {
                self.consider(&mut best, &mut clean, cfg);
            }
            budget = (budget * 2).min(cfg.max_budget);
        }

        if best.is_some() && !cfg.stop_on_first {
            // Phase 2 — clean hunt: the floors at/above the clean bar get the
            // FIRST slice (half) of the remaining time, highest floor first,
            // each with its own doubling budgets. Clean fills are found fast
            // when they exist at all (measured on 15x15: the floor-60 solve
            // rate barely moves between 2s and 1s of dedicated search), so a
            // bounded up-front slice captures most of them without starving
            // the mean polish below. First clean solve ends the hunt — the
            // polish round-robin keeps improving every floor afterwards.
            let hunt_floors: Vec<u8> = tiers
                .iter()
                .rev()
                .copied()
                .filter(|&f| cfg.clean_floor > 0 && f >= cfg.clean_floor && f > lowest)
                .collect();
            let hunt_start = Instant::now();
            if !hunt_floors.is_empty() && hunt_start < global_deadline && clean.is_none() {
                let slice = (global_deadline - hunt_start) / (2 * hunt_floors.len() as u32);
                'hunt: for (i, &floor) in hunt_floors.iter().enumerate() {
                    let tier_deadline = (hunt_start + slice * (i as u32 + 1)).min(global_deadline);
                    budget = cfg.initial_budget;
                    while Instant::now() < tier_deadline {
                        restarts += 1;
                        let seed = self.rng.next_u64();
                        let outcome = self.attempt(floor, budget, seed, tier_deadline);
                        total_nodes += self.nodes;
                        if outcome == Outcome::Solved {
                            self.consider(&mut best, &mut clean, cfg);
                            break 'hunt;
                        }
                        budget = (budget * 2).min(cfg.max_budget);
                    }
                }
            }

            // Phase 3 — polish: spend the remaining time round-robining over
            // the tiers (highest/cleanest first), keeping the best fill by
            // mean score — and the best clean fill alongside. Higher floors
            // yield cleaner fills when feasible; the lowest floor keeps
            // contributing diverse alternatives.
            let mut polish_floors: Vec<u8> = tiers.clone();
            polish_floors.sort_unstable_by(|a, b| b.cmp(a)); // desc
            let mut idx = 0usize;
            budget = cfg.initial_budget;
            while Instant::now() < global_deadline {
                let floor = polish_floors[idx % polish_floors.len()];
                idx += 1;
                restarts += 1;
                let seed = self.rng.next_u64();
                let outcome = self.attempt(floor, budget, seed, global_deadline);
                total_nodes += self.nodes;
                if outcome == Outcome::Solved {
                    self.consider(&mut best, &mut clean, cfg);
                }
                budget = (budget * 2).min(cfg.max_budget);
                // reset budget growth each full cycle so every floor gets small
                // (fast) attempts too
                if idx.is_multiple_of(polish_floors.len()) {
                    budget = cfg.initial_budget;
                }
            }
        }

        // Phase 4 — repair: the clean hunt re-searches the WHOLE grid from
        // scratch, which on large grids often times out even when the best
        // fill is one bad corner away from clean. Rip the weak entries plus
        // their crossings out of the best fill, lock the rest, and re-solve
        // just that region at the clean floor — a tiny search that converts
        // almost-clean fills into clean ones. Skipped under stop_on_first
        // (those callers want the first answer, not polish).
        if !cfg.stop_on_first && cfg.repair_max_weak > 0 && cfg.clean_floor > 0 {
            if let Some(b) = &best {
                let worth = clean.as_ref().is_none_or(|c| c.mean_score < b.mean_score);
                if worth {
                    if let Some(rep) = self.repair(b, cfg) {
                        if clean.as_ref().is_none_or(|c| Self::better_clean(&rep, c)) {
                            clean = Some(rep);
                        }
                    }
                }
            }
        }

        let elapsed = t0.elapsed().as_secs_f64();
        match best {
            Some(b) => {
                let SolvedFill {
                    letters,
                    mean_score,
                    min_score,
                    iffy_count,
                    weak_count,
                    fill,
                } = b;
                SolveResult {
                    letters: Some(letters),
                    nodes: total_nodes,
                    restarts,
                    elapsed_s: elapsed,
                    reason: "solved",
                    mean_score: Some(mean_score),
                    min_score: Some(min_score),
                    iffy_count: Some(iffy_count),
                    weak_count: Some(weak_count),
                    fill: Some(fill),
                    clean,
                }
            }
            None => SolveResult {
                letters: None,
                nodes: total_nodes,
                restarts,
                elapsed_s: elapsed,
                reason: "unsolved",
                mean_score: None,
                min_score: None,
                iffy_count: None,
                weak_count: None,
                fill: None,
                clean: None,
            },
        }
    }

    /// Clean-track ordering: fewest weak (sub-`weak_bar`) entries first, mean
    /// as the tie-break. With the bar disabled both counts are 0 and this
    /// degenerates to pure mean.
    fn better_clean(a: &SolvedFill, b: &SolvedFill) -> bool {
        a.weak_count < b.weak_count || (a.weak_count == b.weak_count && a.mean_score > b.mean_score)
    }

    /// Fold the just-solved fill into the running bests: overall (highest
    /// mean) and clean (fewest weak, then highest mean, among fills whose
    /// min >= `clean_floor`).
    fn consider(
        &self,
        best: &mut Option<SolvedFill>,
        clean: &mut Option<SolvedFill>,
        cfg: &SolveConfig,
    ) {
        let s = self.snapshot_solution(cfg.weak_bar);
        if cfg.clean_floor > 0
            && s.min_score >= cfg.clean_floor
            && clean.as_ref().is_none_or(|c| Self::better_clean(&s, c))
        {
            *clean = Some(s.clone());
        }
        if best.as_ref().is_none_or(|b| s.mean_score > b.mean_score) {
            *best = Some(s);
        }
    }

    fn snapshot_solution(&self, weak_bar: u8) -> SolvedFill {
        // Quality stats (mean/min/iffy) are computed over the SEARCHED entries
        // only — locked theme answers are a given, not a measure of fill skill.
        let mut total = 0u32;
        let mut min_s = 100u8;
        let mut iffy = 0usize;
        let mut weak = 0usize;
        let mut fill = Vec::with_capacity(self.p.entries.len());
        for ei in 0..self.p.entries.len() {
            if self.is_locked[ei] {
                continue;
            }
            let w = self.assigned[ei].unwrap();
            let ld = self.wl.len_data(self.p.entries[ei].len);
            let sc = ld.scores[w];
            total += sc as u32;
            min_s = min_s.min(sc);
            if sc < 50 {
                iffy += 1;
            }
            if weak_bar > 0 && sc < weak_bar {
                weak += 1;
            }
            fill.push((ei, ld.word_string(w), sc));
        }
        // Append locked theme answers to the fill list (for clue output).
        for (ei, answer, score) in &self.locked_out {
            fill.push((*ei, answer.clone(), *score));
        }
        let n = self.n_searchable.max(1);
        SolvedFill {
            mean_score: total as f64 / n as f64,
            letters: self.cell_letter.clone(),
            fill,
            min_score: if self.n_searchable == 0 { 100 } else { min_s },
            iffy_count: iffy,
            weak_count: weak,
        }
    }

    /// Rip the weak entries (searched, score < `clean_floor`) plus their
    /// crossing entries out of `b`, lock every other entry to its current
    /// answer, and re-solve just that region with clean-floor words on a
    /// small budget (an inner solver; its nodes aren't added to the outer
    /// totals). On success the merged fill has min >= `clean_floor` by
    /// construction: every kept searched entry was already at/above the
    /// floor, and region words are drawn from at/above it. Locked theme
    /// answers are never ripped; outer bans carry over so a banned duplicate
    /// can't sneak back in. Returns None when there is nothing to repair,
    /// the weak set exceeds `repair_max_weak`, or the region doesn't
    /// re-solve in its slice.
    fn repair(&self, b: &SolvedFill, cfg: &SolveConfig) -> Option<SolvedFill> {
        let n = self.p.entries.len();
        let mut weak: Vec<usize> = Vec::new();
        for (ei, _ans, sc) in &b.fill {
            if !self.is_locked[*ei] && *sc < cfg.clean_floor {
                weak.push(*ei);
            }
        }
        if weak.is_empty() || weak.len() > cfg.repair_max_weak {
            return None;
        }

        // Region: the weak entries and every searched entry crossing one.
        let mut in_region = vec![false; n];
        for &ei in &weak {
            in_region[ei] = true;
            for &(_, other, _) in &self.crossings[ei] {
                if !self.is_locked[other] {
                    in_region[other] = true;
                }
            }
        }

        // Everything outside the region — kept searched entries and original
        // theme locks alike — becomes a lock for the inner solver.
        let mut answer_of: Vec<Option<&String>> = vec![None; n];
        for (ei, ans, _sc) in &b.fill {
            answer_of[*ei] = Some(ans);
        }
        let mut locks: Vec<Lock> = Vec::with_capacity(n);
        for (ei, region) in in_region.iter().enumerate() {
            if *region {
                continue;
            }
            let e = &self.p.entries[ei];
            locks.push(Lock {
                row: e.row,
                col: e.col,
                dir: e.dir,
                answer: answer_of[ei]?.clone(),
            });
        }

        let mut inner = Solver::with_locks(self.p, self.wl, &locks).ok()?;
        inner.banned = self.banned.clone();
        let r = inner.solve(&SolveConfig {
            time_limit_s: (cfg.time_limit_s * REPAIR_TIME_FRAC).max(0.05),
            tiers: vec![cfg.clean_floor],
            seed: cfg.seed.wrapping_add(0x9E37_79B9),
            initial_budget: cfg.initial_budget,
            max_budget: cfg.max_budget,
            cand_cap: cfg.cand_cap,
            stop_on_first: true, // any region solve is clean by construction
            clean_floor: cfg.clean_floor,
            repair_max_weak: 0, // no recursive repair
            weak_bar: cfg.weak_bar,
        });
        let letters = r.letters?;
        let inner_fill = r.fill?;

        // Merge back into the OUTER solver's stats convention: quality over
        // outer-searched entries only, locked theme answers appended. Kept
        // entries come back through the inner solver's lock reporting with
        // their real wordlist scores, so the merge reads uniformly.
        let mut by_entry: Vec<Option<(String, u8)>> = vec![None; n];
        for (ei, ans, sc) in inner_fill {
            by_entry[ei] = Some((ans, sc));
        }
        let mut total = 0u32;
        let mut min_s = 100u8;
        let mut iffy = 0usize;
        let mut weak_n = 0usize;
        let mut fill = Vec::with_capacity(n);
        for (ei, slot) in by_entry.iter_mut().enumerate() {
            if self.is_locked[ei] {
                continue;
            }
            let (ans, sc) = slot.take()?;
            total += sc as u32;
            min_s = min_s.min(sc);
            if sc < 50 {
                iffy += 1;
            }
            if cfg.weak_bar > 0 && sc < cfg.weak_bar {
                weak_n += 1;
            }
            fill.push((ei, ans, sc));
        }
        for (ei, answer, score) in &self.locked_out {
            fill.push((*ei, answer.clone(), *score));
        }
        debug_assert!(
            self.n_searchable == 0 || min_s >= cfg.clean_floor,
            "repair must produce a clean fill (min {} < floor {})",
            min_s,
            cfg.clean_floor
        );
        let ns = self.n_searchable.max(1);
        Some(SolvedFill {
            letters,
            mean_score: total as f64 / ns as f64,
            min_score: if self.n_searchable == 0 { 100 } else { min_s },
            iffy_count: iffy,
            weak_count: weak_n,
            fill,
        })
    }

    /// One bounded attempt at a given quality floor. Resets state first.
    fn attempt(&mut self, floor: u8, budget: u64, seed: u64, deadline: Instant) -> Outcome {
        self.floor = floor;
        self.attempt_budget = budget;
        self.deadline = deadline;
        self.nodes = 0;
        self.rng = Rng::new(seed);

        // reset working state from base
        for ei in 0..self.p.entries.len() {
            self.domains[ei].copy_from(&self.base_domains[ei]);
            self.assigned[ei] = None;
            let len = self.p.entries[ei].len;
            let cutoff = self.wl.len_data(len).tier_cutoff(floor);
            self.cutoff_by_entry[ei] = cutoff;
            self.dom_count[ei] = self.domains[ei].count_ones_below(cutoff);
        }
        for l in 0..self.used.len() {
            self.used[l] = Bitset::zeros(self.wl.by_len[l].n.max(1));
        }
        // Locked answers present in the wordlist are marked used so the search
        // can't reuse them elsewhere (no duplicate answers); banned answers
        // are marked the same way so they can never be assigned.
        for &(len, w) in self.locked_used.iter().chain(self.banned.iter()) {
            self.used[len].set(w);
        }
        for cid in 0..self.p.n_cells {
            self.cell_letter[cid] = self.base_cell_letter[cid];
        }
        self.n_assigned = 0;

        self.recurse()
    }

    fn recurse(&mut self) -> Outcome {
        if self.n_assigned == self.n_searchable {
            return Outcome::Solved;
        }
        self.nodes += 1;
        if self.nodes >= self.attempt_budget {
            return Outcome::Budget;
        }
        if self.nodes & 1023 == 0 && Instant::now() >= self.deadline {
            return Outcome::TimeLimit;
        }

        let ei = match self.select_entry() {
            Some(e) => e,
            None => return Outcome::Exhausted,
        };

        let candidates = self.ordered_candidates(ei);
        if candidates.is_empty() {
            return Outcome::Exhausted;
        }

        for w in candidates {
            let frame = self.assign(ei, w);
            // Forward check on the POST-propagation counts (frame.nbrs holds the
            // saved pre-assignment values for undo — checking those never fires).
            let dead = frame
                .nbrs
                .iter()
                .any(|(nbr, _, _)| self.dom_count[*nbr] == 0);
            if !dead {
                let r = self.recurse();
                if r == Outcome::Solved {
                    return Outcome::Solved;
                }
                if r == Outcome::Budget || r == Outcome::TimeLimit {
                    self.undo(frame);
                    return r;
                }
            }
            self.undo(frame);
        }
        Outcome::Exhausted
    }

    fn select_entry(&self) -> Option<usize> {
        let mut best = None;
        let mut best_cnt = u32::MAX;
        let mut best_len = 0usize;
        for ei in 0..self.p.entries.len() {
            if self.assigned[ei].is_some() || self.is_locked[ei] {
                continue;
            }
            let cnt = self.dom_count[ei];
            let len = self.p.entries[ei].len;
            if cnt < best_cnt || (cnt == best_cnt && len > best_len) {
                best_cnt = cnt;
                best_len = len;
                best = Some(ei);
            }
        }
        best
    }

    /// In-tier candidates ordered by score band (desc), then least-constraining
    /// value (high neighbor freedom), with light random jitter for restart
    /// diversity. The band term dominates so quality ordering is preserved.
    fn ordered_candidates(&mut self, ei: usize) -> Vec<usize> {
        let len = self.p.entries[ei].len;
        let cutoff = self.cutoff_by_entry[ei];

        // Active crossings to unassigned neighbors: (pos_in_ei, nbr_len, nbr_pos)
        let mut cross: Vec<(usize, usize, usize)> = Vec::new();
        for &(pos, nbr, npos) in &self.crossings[ei] {
            if self.assigned[nbr].is_none() && !self.is_locked[nbr] {
                cross.push((pos, self.p.entries[nbr].len, npos));
            }
        }

        // (word, score, lcv)
        let mut cands: Vec<(usize, u8, u32)> = Vec::new();
        {
            let ld = self.wl.len_data(len);
            let used = &self.used[len];
            for w in self.domains[ei].iter_ones() {
                if w >= cutoff {
                    break;
                }
                if used.get(w) {
                    continue;
                }
                let letters = &ld.letters[w];
                let mut lcv: u32 = 0;
                for &(pos, nlen, npos) in &cross {
                    lcv = lcv.saturating_add(self.wl.len_data(nlen).pos_count(npos, letters[pos]));
                }
                cands.push((w, ld.scores[w], lcv));
                if cands.len() >= self.cand_cap {
                    break;
                }
            }
        }

        // key = band * BIG + jittered lcv. band dominates → score order kept.
        let mut keyed: Vec<(usize, i64)> = cands
            .iter()
            .map(|&(w, sc, lcv)| {
                let band = (sc / 5) as i64;
                let j = 850 + self.rng.below(301) as i64; // 0.85..1.15
                let key = band * 1_000_000_000 + (lcv as i64 * j / 1000);
                (w, key)
            })
            .collect();
        keyed.sort_by_key(|a| std::cmp::Reverse(a.1));
        keyed.into_iter().map(|(w, _)| w).collect()
    }

    fn assign(&mut self, ei: usize, w: usize) -> TrailFrame {
        let len = self.p.entries[ei].len;
        let letters: Vec<u8> = self.wl.len_data(len).letters[w].to_vec();

        self.assigned[ei] = Some(w);
        self.used[len].set(w);
        self.n_assigned += 1;

        let mut frame = TrailFrame {
            entry: ei,
            word: w,
            len,
            nbrs: Vec::new(),
            set_cells: Vec::new(),
        };
        let cells = self.p.entries[ei].cells.clone();
        for (pos, &cid) in cells.iter().enumerate() {
            let ch = letters[pos];
            if self.cell_letter[cid].is_none() {
                self.cell_letter[cid] = Some(ch);
                frame.set_cells.push(cid);
            }
            for &(nbr, npos) in &self.p.cell_entries[cid] {
                if nbr == ei || self.assigned[nbr].is_some() || self.is_locked[nbr] {
                    continue;
                }
                let nlen = self.p.entries[nbr].len;
                let mask = self.wl.len_data(nlen).compat(npos, ch).clone();
                let saved = self.domains[nbr].clone();
                let saved_cnt = self.dom_count[nbr];
                self.domains[nbr].and_assign(&mask);
                self.dom_count[nbr] = self.domains[nbr].count_ones_below(self.cutoff_by_entry[nbr]);
                frame.nbrs.push((nbr, saved, saved_cnt));
            }
        }
        frame
    }

    fn undo(&mut self, frame: TrailFrame) {
        for (nbr, saved, saved_cnt) in frame.nbrs.into_iter() {
            self.domains[nbr].copy_from(&saved);
            self.dom_count[nbr] = saved_cnt;
        }
        for cid in frame.set_cells {
            self.cell_letter[cid] = None;
        }
        self.used[frame.len].clear(frame.word);
        self.assigned[frame.entry] = None;
        self.n_assigned -= 1;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::wordlist::Wordlist;

    fn dict() -> Wordlist {
        let src = "\
CAB;80
ORE;70
TEN;75
COT;60
ARE;65
BEN;55
CAT;50
CAR;50
RAT;40
DOG;40
";
        Wordlist::from_str(src, 40)
    }

    fn fast_cfg() -> SolveConfig {
        SolveConfig {
            time_limit_s: 2.0,
            stop_on_first: true,
            ..Default::default()
        }
    }

    #[test]
    fn solves_3x3_open() {
        let p = Puzzle::from_template("...\n...\n...\n");
        let wl = dict();
        let mut s = Solver::new(&p, &wl);
        let r = s.solve(&fast_cfg());
        assert_eq!(r.reason, "solved", "should fill a 3x3 double word square");
        let letters = r.letters.unwrap();
        let words: std::collections::HashSet<String> =
            s.p.entries
                .iter()
                .map(|e| {
                    e.cells
                        .iter()
                        .map(|&c| (b'A' + letters[c].unwrap()) as char)
                        .collect::<String>()
                })
                .collect();
        assert_eq!(words.len(), 6, "all six answers must be distinct");
    }

    #[test]
    fn respects_prefilled() {
        let p = Puzzle::from_template("CAB\n...\n...\n");
        let wl = dict();
        let mut s = Solver::new(&p, &wl);
        let r = s.solve(&fast_cfg());
        assert_eq!(r.reason, "solved");
        let letters = r.letters.unwrap();
        assert_eq!(letters[0], Some(2)); // C
        assert_eq!(letters[1], Some(0)); // A
        assert_eq!(letters[2], Some(1)); // B
    }

    #[test]
    fn locks_theme_answer_not_in_wordlist() {
        // ZZZ is not in the dict; lock it as the top across entry and fill the
        // 3x3 around it. The down entries must start with Z,Z,Z respectively.
        let p = Puzzle::from_template("...\n...\n...\n");
        let wl = dict();
        let locks = vec![Lock {
            row: 0,
            col: 0,
            dir: Dir::Across,
            answer: "ZZZ".into(),
        }];
        let mut s = Solver::with_locks(&p, &wl, &locks).unwrap();
        // The down words would need to start with Z — dict has none, so this
        // particular lock is unsatisfiable; assert we don't panic and report it.
        let r = s.solve(&fast_cfg());
        assert_eq!(r.reason, "unsolved");

        // Now a satisfiable lock: top row CAB (in dict), fill around it.
        let locks = vec![Lock {
            row: 0,
            col: 0,
            dir: Dir::Across,
            answer: "CAB".into(),
        }];
        let mut s = Solver::with_locks(&p, &wl, &locks).unwrap();
        let r = s.solve(&fast_cfg());
        assert_eq!(r.reason, "solved");
        let letters = r.letters.unwrap();
        assert_eq!(
            (letters[0], letters[1], letters[2]),
            (Some(2), Some(0), Some(1))
        );
        // The locked answer appears in the fill output.
        let fill = r.fill.unwrap();
        assert!(fill.iter().any(|(_, w, _)| w == "CAB"));
    }

    /// Two disjoint fills of the open 3x3 exist in this dict: a "dirty" one
    /// with the top mean (76.7) that leans on WEN;50, and a clean one
    /// (min 55, mean 67.5): CAB/ORE/TEN x COT/ARE/BEN vs PAW/IRE/TEN x
    /// PIT/ARE/WEN. Mean polish must keep the dirty fill as the primary
    /// result while the clean hunt surfaces the all->=55 fill alongside.
    #[test]
    fn tracks_best_clean_fill_alongside_mean_best() {
        let src =
            "CAB;80\nORE;70\nTEN;75\nCOT;60\nARE;65\nBEN;55\nPAW;90\nIRE;90\nPIT;90\nWEN;50\n";
        let wl = Wordlist::from_str(src, 40);
        let p = Puzzle::from_template("...\n...\n...\n");
        let mut s = Solver::new(&p, &wl);
        let r = s.solve(&SolveConfig {
            time_limit_s: 1.0,
            ..Default::default()
        });
        assert_eq!(r.reason, "solved");
        assert!(
            (r.mean_score.unwrap() - 460.0 / 6.0).abs() < 0.1,
            "primary best is the higher-mean dirty fill, got {}",
            r.mean_score.unwrap()
        );
        assert_eq!(r.min_score.unwrap(), 50, "dirty fill bottoms out at WEN;50");
        let clean = r.clean.expect("clean hunt must find the all->=55 fill");
        assert_eq!(clean.min_score, 55);
        assert!((clean.mean_score - 67.5).abs() < 0.01);
        assert_eq!(clean.iffy_count, 0);
    }

    /// With a clean floor no fill can reach (both families rely on sub-70
    /// words), `clean` stays empty while the primary fill still solves.
    #[test]
    fn clean_absent_when_floor_unreachable() {
        let src =
            "CAB;80\nORE;70\nTEN;75\nCOT;60\nARE;65\nBEN;55\nPAW;90\nIRE;90\nPIT;90\nWEN;50\n";
        let wl = Wordlist::from_str(src, 40);
        let p = Puzzle::from_template("...\n...\n...\n");
        let mut s = Solver::new(&p, &wl);
        let r = s.solve(&SolveConfig {
            time_limit_s: 0.5,
            clean_floor: 70,
            ..Default::default()
        });
        assert_eq!(r.reason, "solved");
        assert!(r.clean.is_none(), "no fill has min >= 70");
    }

    /// Banning WEN kills the dirty family outright, so the only fills left
    /// are the clean ones — and locked/unknown answers refuse to ban.
    #[test]
    fn ban_answer_excludes_word_and_guards_locks() {
        let src =
            "CAB;80\nORE;70\nTEN;75\nCOT;60\nARE;65\nBEN;55\nPAW;90\nIRE;90\nPIT;90\nWEN;50\n";
        let wl = Wordlist::from_str(src, 40);
        let p = Puzzle::from_template("...\n...\n...\n");
        let mut s = Solver::new(&p, &wl);
        assert!(!s.ban_answer("QQQ"), "unknown word can't be banned");
        assert!(s.ban_answer("wen"), "known word bans (case-insensitive)");
        let r = s.solve(&SolveConfig {
            time_limit_s: 0.5,
            ..Default::default()
        });
        assert_eq!(r.reason, "solved");
        let fill = r.fill.unwrap();
        assert!(
            fill.iter().all(|(_, w, _)| w != "WEN"),
            "banned word must not appear"
        );
        assert!(
            (r.mean_score.unwrap() - 67.5).abs() < 0.01,
            "only the clean family remains, got mean {}",
            r.mean_score.unwrap()
        );

        let locks = vec![Lock {
            row: 0,
            col: 0,
            dir: Dir::Across,
            answer: "CAB".into(),
        }];
        let mut s = Solver::with_locks(&p, &wl, &locks).unwrap();
        assert!(!s.ban_answer("CAB"), "locked answers are unbannable");
    }

    /// Repair dict: the dirty 3x3 (CAB/ORE/TEN x COT/ARE/BEN) leans on
    /// ORE;50. Ripping ORE plus its crossings (all three downs) with CAB/TEN
    /// kept leaves exactly one clean region solve: UXU;60 with CUT/AXE/BUN —
    /// the only >=55 middle row compatible with the pinned C?T/A?E/B?N.
    fn repair_dict(down_alt_score: u8) -> Wordlist {
        let src = format!(
            "CAB;80\nTEN;75\nORE;50\nUXU;60\nCOT;90\nARE;90\nBEN;90\nCUT;{s}\nAXE;{s}\nBUN;{s}\n",
            s = down_alt_score
        );
        Wordlist::from_str(&src, 40)
    }

    /// The dirty best fill of the open 3x3 over `repair_dict`, hand-built so
    /// the repair mechanism can be exercised deterministically.
    fn dirty_fill(p: &Puzzle) -> SolvedFill {
        let e = |r, c, d| p.find_entry(r, c, d).unwrap();
        let fill = vec![
            (e(0, 0, Dir::Across), "CAB".to_string(), 80u8),
            (e(1, 0, Dir::Across), "ORE".to_string(), 50),
            (e(2, 0, Dir::Across), "TEN".to_string(), 75),
            (e(0, 0, Dir::Down), "COT".to_string(), 90),
            (e(0, 1, Dir::Down), "ARE".to_string(), 90),
            (e(0, 2, Dir::Down), "BEN".to_string(), 90),
        ];
        SolvedFill {
            letters: Vec::new(), // repair reads answers from `fill`, not letters
            mean_score: 475.0 / 6.0,
            min_score: 50,
            iffy_count: 0,
            weak_count: 1, // ORE;50 under the default bar
            fill,
        }
    }

    /// Direct mechanism test: ripping the weak entry + crossings and
    /// re-solving at the clean floor produces the unique merged clean fill.
    #[test]
    fn repair_converts_weak_fill_to_clean() {
        let wl = repair_dict(90);
        let p = Puzzle::from_template("...\n...\n...\n");
        let s = Solver::new(&p, &wl);
        let b = dirty_fill(&p);
        let cfg = SolveConfig {
            time_limit_s: 2.0,
            ..Default::default()
        };
        let rep = s
            .repair(&b, &cfg)
            .expect("region must re-solve at floor 55");
        assert_eq!(rep.min_score, 60, "weakest repaired entry is UXU;60");
        assert!(
            (rep.mean_score - 485.0 / 6.0).abs() < 0.01,
            "merged mean over all six entries, got {}",
            rep.mean_score
        );
        assert_eq!(rep.iffy_count, 0);
        let words: Vec<&str> = rep.fill.iter().map(|(_, w, _)| w.as_str()).collect();
        for w in ["CAB", "TEN", "UXU", "CUT", "AXE", "BUN"] {
            assert!(words.contains(&w), "repaired fill must contain {w}");
        }
        assert!(!words.contains(&"ORE"), "the weak word must be gone");
        // middle row letters rewritten to U X U
        assert_eq!(rep.letters[3], Some(20));
        assert_eq!(rep.letters[4], Some(23));
        assert_eq!(rep.letters[5], Some(20));

        // Guard: repair_max_weak = 0 disables.
        let off = SolveConfig {
            repair_max_weak: 0,
            ..cfg
        };
        assert!(s.repair(&b, &off).is_none());
    }

    /// Theme locks are never ripped, keep their verbatim answer, and stay
    /// excluded from the merged quality stats.
    #[test]
    fn repair_preserves_theme_locks_and_stats() {
        let wl = repair_dict(90);
        let p = Puzzle::from_template("...\n...\n...\n");
        let locks = vec![Lock {
            row: 0,
            col: 0,
            dir: Dir::Across,
            answer: "CAB".into(),
        }];
        let s = Solver::with_locks(&p, &wl, &locks).unwrap();
        let e = |r, c, d| p.find_entry(r, c, d).unwrap();
        let b = SolvedFill {
            letters: Vec::new(),
            mean_score: 395.0 / 5.0,
            min_score: 50,
            iffy_count: 0,
            weak_count: 1, // ORE;50 under the default bar
            fill: vec![
                (e(1, 0, Dir::Across), "ORE".to_string(), 50),
                (e(2, 0, Dir::Across), "TEN".to_string(), 75),
                (e(0, 0, Dir::Down), "COT".to_string(), 90),
                (e(0, 1, Dir::Down), "ARE".to_string(), 90),
                (e(0, 2, Dir::Down), "BEN".to_string(), 90),
                (e(0, 0, Dir::Across), "CAB".to_string(), 80), // locked, appended
            ],
        };
        let cfg = SolveConfig {
            time_limit_s: 2.0,
            ..Default::default()
        };
        let rep = s.repair(&b, &cfg).expect("region must re-solve");
        assert_eq!(rep.min_score, 60);
        assert!(
            (rep.mean_score - 405.0 / 5.0).abs() < 0.01,
            "stats cover the five searched entries only, got {}",
            rep.mean_score
        );
        assert!(
            rep.fill.iter().any(|(_, w, _)| w == "CAB"),
            "locked theme answer stays in the fill output"
        );
    }

    /// End-to-end: when the clean hunt can't run (no >=55 tier configured)
    /// the repair phase still surfaces the clean alternative, while the
    /// higher-mean dirty fill stays the primary result.
    #[test]
    fn solve_repair_fills_clean_slot() {
        let wl = repair_dict(60); // low-scored alt downs: dirty keeps the mean lead
        let p = Puzzle::from_template("...\n...\n...\n");
        let mut s = Solver::new(&p, &wl);
        let r = s.solve(&SolveConfig {
            time_limit_s: 0.5,
            tiers: vec![40],
            ..Default::default()
        });
        assert_eq!(r.reason, "solved");
        assert!(
            (r.mean_score.unwrap() - 475.0 / 6.0).abs() < 0.1,
            "primary best stays the dirty fill, got {}",
            r.mean_score.unwrap()
        );
        assert_eq!(r.min_score.unwrap(), 50);
        let clean = r.clean.expect("repair must surface the clean alternative");
        assert_eq!(clean.min_score, 60);
        assert!(
            (clean.mean_score - 395.0 / 6.0).abs() < 0.05,
            "clean fill is the UXU variant, got {}",
            clean.mean_score
        );
    }

    #[test]
    fn lock_errors_on_bad_spec() {
        let p = Puzzle::from_template("...\n...\n...\n");
        let wl = dict();
        // wrong length
        let bad = vec![Lock {
            row: 0,
            col: 0,
            dir: Dir::Across,
            answer: "TOOLONG".into(),
        }];
        assert!(Solver::with_locks(&p, &wl, &bad).is_err());
        // no entry there
        let bad = vec![Lock {
            row: 9,
            col: 9,
            dir: Dir::Across,
            answer: "CAB".into(),
        }];
        assert!(Solver::with_locks(&p, &wl, &bad).is_err());
    }
}
