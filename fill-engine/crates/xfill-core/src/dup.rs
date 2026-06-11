//! Root-duplicate detection for filled grids: two answers sharing a word root
//! (TEN / TENTH, EVEN / UNEVENLY, BASEBALL / SEVEBALLESTEROS sharing BALL)
//! read as duplicates to an editor even though they aren't exact-equal, and
//! they're historically the most common high-severity QA finding. The solver's
//! `used` bitsets only prevent exact-word reuse, so this check runs on
//! COMPLETED fills in the screeners: grids with a root-dup simply aren't kept.
//!
//! The cost asymmetry justifies being aggressive: a false positive rejects one
//! candidate grid out of a surplus (the screeners early-stop with grids to
//! spare); a miss becomes a grid-layer QA finding that can only be fixed by
//! regenerating + re-cluing the whole puzzle.
//!
//! Three rules, each tuned for crossword answers (not a full morphology
//! engine):
//!   1. STEM EQUALITY — inflections: plurals (S/ES/IES), tenses (ED/ING with
//!      undoubling), comparatives (ER/EST), ordinals (TH), adverbs (LY), and
//!      prefixes (UN/RE/DIS/MIS/NON/OVER/OUT/PRE when >= 4 letters remain,
//!      which protects UNION->ION, UNIT->IT). Catches TEN/TENTH, CAT/CATS,
//!      RUN/RUNNING, EVEN/UNEVENLY.
//!   2. CONTAINMENT — one answer's stem (>= 4 letters) appears inside another
//!      answer. Catches HOME/HOMERUN, EVEN/UNEVENLY. The >= 4 floor keeps
//!      TEN/TENNIS-style coincidences unflagged.
//!   3. SHARED EMBEDDED WORD — two answers both contain the same dictionary
//!      word (4-7 letters, score >= 70: an editor only notices PROMINENT
//!      words) at a word EDGE in at least one of them, e.g. BALL (score 90)
//!      in BASEBALL (suffix) and SEVEBALLESTEROS (interior). Derivational
//!      fragments (ABLE, LESS, NESS, ...) are exempt — shared suffixes like
//!      PORTABLE/READABLE are editorially fine.
//!
//! Still not caught (rare; QA remains the backstop): irregular forms
//! (RAN/RUN) and short roots behind prefixes (RERUN/RUN — remainder < 4).
//!
//! TS port of the stemmer: clue-writer/src/dupCheck.ts — keep in sync.

use crate::wordlist::Wordlist;
use std::collections::HashSet;

/// Stem an uppercase A-Z answer by iteratively stripping one inflectional
/// suffix or prefix per pass, never shrinking below 3 letters (4 for
/// prefixes).
pub fn stem(word: &str) -> String {
    let mut w = word.to_string();
    loop {
        let before = w.len();
        w = strip_one(&w);
        if w.len() == before {
            return w;
        }
    }
}

fn strip_one(w: &str) -> String {
    let n = w.len();
    let keep = |k: usize| n >= k + 3; // remaining stem must be >= 3 letters

    // IES -> Y (STORIES -> STORY)
    if keep(2) && w.ends_with("IES") {
        return format!("{}Y", &w[..n - 3]);
    }
    // Undoubling suffixes: strip, then collapse a doubled final consonant
    // (RUNNING -> RUNN -> RUN, STOPPED -> STOPP -> STOP, BIGGEST -> BIG).
    for suf in ["ING", "EST", "ED", "ER"] {
        if keep(suf.len()) && w.ends_with(suf) {
            let mut s = w[..n - suf.len()].to_string();
            let b = s.as_bytes();
            let last = b[b.len() - 1];
            if b.len() >= 4 && last == b[b.len() - 2] && !b"AEIOU".contains(&last) {
                s.pop();
            }
            return s;
        }
    }
    // Plain suffixes.
    for suf in ["ES", "TH", "LY"] {
        if keep(suf.len()) && w.ends_with(suf) {
            return w[..n - suf.len()].to_string();
        }
    }
    // S, but not SS (MASS stays MASS).
    if keep(1) && w.ends_with('S') && !w.ends_with("SS") {
        return w[..n - 1].to_string();
    }
    // Prefixes, only when a substantial root (>= 4 letters) remains:
    // UNEVEN -> EVEN, REOPEN -> OPEN; but UNION stays (ION is 3), UNIT stays.
    for pre in ["OVER", "OUT", "PRE", "NON", "DIS", "MIS", "UN", "RE"] {
        if n >= pre.len() + 4 && w.starts_with(pre) {
            return w[pre.len()..].to_string();
        }
    }
    w.to_string()
}

/// Derivational fragments that are real words but read as shared SUFFIX
/// morphology, not shared roots — two answers both ending in -ABLE or -NESS
/// are editorially fine.
const FRAGMENT_EXEMPT: &[&str] = &["ABLE", "LESS", "NESS", "LIKE", "SHIP", "WISE", "FULL"];

/// Stem-equality + containment check (no wordlist needed). `items` is
/// (answer, is_theme); theme-vs-theme pairs are EXEMPT — theme sets often
/// share a word deliberately (HOMERUN / HOMEPAGE / HOMEROOM).
pub fn find_root_dup(items: &[(String, bool)]) -> Option<(String, String)> {
    let stems: Vec<String> = items.iter().map(|(a, _)| stem(a)).collect();
    for i in 0..items.len() {
        for j in (i + 1)..items.len() {
            if items[i].1 && items[j].1 {
                continue;
            }
            if stems[i] == stems[j] {
                return Some((items[i].0.clone(), items[j].0.clone()));
            }
            // Containment: one answer's stem inside the other answer.
            if stems[i].len() >= 4 && items[j].0.contains(stems[i].as_str()) {
                return Some((items[i].0.clone(), items[j].0.clone()));
            }
            if stems[j].len() >= 4 && items[i].0.contains(stems[j].as_str()) {
                return Some((items[i].0.clone(), items[j].0.clone()));
            }
        }
    }
    None
}

/// Full duplicate checker: stem equality + containment + shared embedded
/// dictionary words. Build once per run (it snapshots the relevant slice of
/// the wordlist), then call from worker threads.
pub struct DupChecker {
    /// Real words of length 4-7 scoring >= 70 — candidates for the
    /// shared-embedded-word rule. The high floor keeps the rule to words an
    /// editor would actually notice; including the obscure 50-69 band was
    /// measured to reject most of a 15x15 library for shared fragments no
    /// human would flag.
    embedded: HashSet<String>,
}

impl DupChecker {
    pub fn new(wl: &Wordlist) -> Self {
        let mut embedded = HashSet::new();
        for len in 4..=7usize {
            let ld = wl.len_data(len);
            let cutoff = ld.tier_cutoff(70);
            for i in 0..cutoff {
                embedded.insert(ld.word_string(i));
            }
        }
        for f in FRAGMENT_EXEMPT {
            embedded.remove(*f);
        }
        DupChecker { embedded }
    }

    /// Every embedded dictionary word (4-7 letters) inside `answer`, tagged
    /// with whether it sits at an edge (prefix or suffix position).
    fn embedded_words(&self, answer: &str) -> Vec<(String, bool)> {
        let n = answer.len();
        let mut out = Vec::new();
        for wlen in 4..=7usize.min(n) {
            for start in 0..=(n - wlen) {
                let sub = &answer[start..start + wlen];
                if self.embedded.contains(sub) {
                    let edge = start == 0 || start + wlen == n;
                    out.push((sub.to_string(), edge));
                }
            }
        }
        out
    }

    /// First duplicate pair under any rule, or None.
    pub fn find_dup(&self, items: &[(String, bool)]) -> Option<(String, String)> {
        if let Some(pair) = find_root_dup(items) {
            return Some(pair);
        }
        // Shared embedded word, at an edge in at least one of the pair.
        let embeds: Vec<Vec<(String, bool)>> =
            items.iter().map(|(a, _)| self.embedded_words(a)).collect();
        for i in 0..items.len() {
            for j in (i + 1)..items.len() {
                if items[i].1 && items[j].1 {
                    continue;
                }
                for (wi, edge_i) in &embeds[i] {
                    for (wj, edge_j) in &embeds[j] {
                        if wi == wj && (*edge_i || *edge_j) {
                            return Some((items[i].0.clone(), items[j].0.clone()));
                        }
                    }
                }
            }
        }
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fill(a: &str) -> (String, bool) {
        (a.to_string(), false)
    }
    fn theme(a: &str) -> (String, bool) {
        (a.to_string(), true)
    }

    fn checker(words: &[&str]) -> DupChecker {
        let mut embedded: HashSet<String> = words.iter().map(|w| w.to_string()).collect();
        for f in FRAGMENT_EXEMPT {
            embedded.remove(*f);
        }
        DupChecker { embedded }
    }

    #[test]
    fn stems_inflections_and_prefixes() {
        assert_eq!(stem("TENTH"), "TEN");
        assert_eq!(stem("CATS"), "CAT");
        assert_eq!(stem("STORIES"), "STORY");
        assert_eq!(stem("RUNNING"), "RUN");
        assert_eq!(stem("STOPPED"), "STOP");
        assert_eq!(stem("FASTEST"), "FAST");
        assert_eq!(stem("MASS"), "MASS"); // SS guard
        assert_eq!(stem("OPERA"), "OPERA");
        // Prefixes with the >= 4 remainder guard.
        assert_eq!(stem("UNEVENLY"), "EVEN"); // LY then UN
        assert_eq!(stem("REOPENED"), "OPEN"); // ED then RE
        assert_eq!(stem("UNION"), "UNION"); // ION is 3 — not stripped
        assert_eq!(stem("UNIT"), "UNIT");
        assert_eq!(stem("OVERT"), "OVERT");
    }

    #[test]
    fn flags_stem_dups() {
        assert!(find_root_dup(&[fill("TEN"), fill("AREA"), fill("TENTH")]).is_some());
        assert!(find_root_dup(&[fill("EVEN"), fill("UNEVENLY")]).is_some()); // reported case
        assert!(find_root_dup(&[fill("RUN"), fill("RUNNING")]).is_some());
    }

    #[test]
    fn flags_containment() {
        assert!(find_root_dup(&[fill("HOME"), fill("HOMERUN")]).is_some());
        // >= 4 floor: TEN inside TENNIS is a coincidence, not a root.
        assert!(find_root_dup(&[fill("TEN"), fill("TENNIS")]).is_none());
    }

    #[test]
    fn allows_unrelated_words() {
        assert!(find_root_dup(&[fill("ERA"), fill("OPERA"), fill("AREA")]).is_none());
        assert!(find_root_dup(&[fill("UNION"), fill("ION")]).is_none());
    }

    #[test]
    fn shared_embedded_word_flagged() {
        // The reported case: BALL edge-suffix in BASEBALL, interior in
        // SEVEBALLESTEROS.
        let c = checker(&["BALL", "BASE", "EROS", "LEST"]);
        assert!(c
            .find_dup(&[theme("BASEBALL"), fill("SEVEBALLESTEROS")])
            .is_some());
        // No shared embedded word -> clean.
        assert!(c.find_dup(&[fill("BASEBALL"), fill("MUSTARD")]).is_none());
    }

    #[test]
    fn derivational_fragments_exempt() {
        // PORTABLE / READABLE share ABLE (edge in both) — editorially fine.
        let c = checker(&["ABLE", "PORT", "READ"]);
        assert!(c.find_dup(&[fill("PORTABLE"), fill("READABLE")]).is_none());
    }

    #[test]
    fn theme_pairs_exempt_but_theme_vs_fill_checked() {
        assert!(find_root_dup(&[theme("HOMER"), theme("HOMERS")]).is_none());
        assert!(find_root_dup(&[theme("TENTH"), fill("TEN")]).is_some());
        let c = checker(&["BALL"]);
        // Two theme answers sharing BALL: allowed (that can BE the theme).
        assert!(c
            .find_dup(&[theme("BASEBALL"), theme("BALLPARK")])
            .is_none());
    }
}
