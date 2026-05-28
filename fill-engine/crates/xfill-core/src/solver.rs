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
}

impl Default for SolveConfig {
    fn default() -> Self {
        SolveConfig {
            time_limit_s: 30.0,
            tiers: vec![50, 40],
            seed: 0,
            initial_budget: 2_000,
            max_budget: 400_000,
            cand_cap: 400,
            stop_on_first: false,
        }
    }
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
    pub fill: Option<Vec<(usize, String, u8)>>,
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

struct Best {
    mean: f64,
    letters: Vec<Option<u8>>,
    fill: Vec<(usize, String, u8)>,
    min: u8,
    iffy: usize,
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

        let mut best: Option<Best> = None;
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
        let lowest = tiers[0];

        // Phase 1 — feasibility: restart at the lowest floor until first solve.
        let mut budget = cfg.initial_budget;
        while Instant::now() < global_deadline && best.is_none() {
            restarts += 1;
            let seed = self.rng.next_u64();
            let outcome = self.attempt(lowest, budget, seed, global_deadline);
            total_nodes += self.nodes;
            if outcome == Outcome::Solved {
                best = Some(self.snapshot_solution());
            }
            budget = (budget * 2).min(cfg.max_budget);
        }

        // Phase 2 — polish: with a baseline secured, spend remaining time
        // round-robining over the tiers (highest/cleanest first), keeping the
        // best fill by mean score. Higher floors yield cleaner fills when
        // feasible; the lowest floor keeps contributing diverse alternatives.
        if best.is_some() && !cfg.stop_on_first && !tiers.is_empty() {
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
                    let cand = self.snapshot_solution();
                    if cand.mean > best.as_ref().unwrap().mean {
                        best = Some(cand);
                    }
                }
                budget = (budget * 2).min(cfg.max_budget);
                // reset budget growth each full cycle so every floor gets small
                // (fast) attempts too
                if idx.is_multiple_of(polish_floors.len()) {
                    budget = cfg.initial_budget;
                }
            }
        }

        let elapsed = t0.elapsed().as_secs_f64();
        match best {
            Some(b) => SolveResult {
                letters: Some(b.letters),
                nodes: total_nodes,
                restarts,
                elapsed_s: elapsed,
                reason: "solved",
                mean_score: Some(b.mean),
                min_score: Some(b.min),
                iffy_count: Some(b.iffy),
                fill: Some(b.fill),
            },
            None => SolveResult {
                letters: None,
                nodes: total_nodes,
                restarts,
                elapsed_s: elapsed,
                reason: "unsolved",
                mean_score: None,
                min_score: None,
                iffy_count: None,
                fill: None,
            },
        }
    }

    fn snapshot_solution(&self) -> Best {
        // Quality stats (mean/min/iffy) are computed over the SEARCHED entries
        // only — locked theme answers are a given, not a measure of fill skill.
        let mut total = 0u32;
        let mut min_s = 100u8;
        let mut iffy = 0usize;
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
            fill.push((ei, ld.word_string(w), sc));
        }
        // Append locked theme answers to the fill list (for clue output).
        for (ei, answer, score) in &self.locked_out {
            fill.push((*ei, answer.clone(), *score));
        }
        let n = self.n_searchable.max(1);
        Best {
            mean: total as f64 / n as f64,
            letters: self.cell_letter.clone(),
            fill,
            min: if self.n_searchable == 0 { 100 } else { min_s },
            iffy,
        }
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
        // can't reuse them elsewhere (no duplicate answers).
        for &(len, w) in &self.locked_used {
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
            let dead = frame.nbrs.iter().any(|(_, _, c)| *c == 0);
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
