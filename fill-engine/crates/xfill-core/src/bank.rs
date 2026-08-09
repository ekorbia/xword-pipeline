//! Template bank: full-size templates that have PROVEN they clean-fill.
//!
//! The 2026-08 template-reliability study found per-pattern clean-fill rates
//! are bimodal — most random 15×15 shapes essentially never fill clean, a
//! small minority almost always do — and that reusing the proven minority
//! lifts publication keeper rates ~6.4× held-out. The bank is that minority,
//! persisted: the `tpl-bank` bin probes fresh shapes and appends the ones
//! that clean-fill; generation then draws a caller-chosen fraction of its
//! candidates from the bank (fills still vary by seed — reusing a grid
//! PATTERN is normal constructor practice) and the rest fresh, which keeps
//! exploring and feeds future bank builds.
//!
//! File format (`data/tpl-bank-<size>.txt`, kept LOCAL/gitignored — a
//! private, regenerable asset): one template per line, rows joined by `/`,
//! `.` open / `#` block; anything after the first `;` is metadata (`words=`,
//! `cleans=`). Any line that isn't an exact `size`×`size` grid — the file
//! header, hand notes, other sizes — is skipped on load, so the file can be
//! hand-pruned freely. There is deliberately NO `#`-comment rule: a
//! template's first row can legitimately begin with a block cell (this bug
//! hid 271 of the first 1,092 banked shapes), and prose never survives the
//! structural validation anyway.

use crate::util::Rng;

/// Parse a bank file into template strings (rows joined by `\n`, the form
/// `Puzzle::from_template` accepts). Only exact `size`×`size` grids of
/// `.`/`#` survive; everything else (the header, metadata tails, other
/// sizes) is skipped. NOTE: no comment prefix — a template's first row may
/// begin with `#` (a block cell).
pub fn parse_bank(text: &str, size: usize) -> Vec<String> {
    let mut out = Vec::new();
    for line in text.lines() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        let tpl_part = line.split(';').next().unwrap_or("");
        let rows: Vec<&str> = tpl_part.split('/').collect();
        if rows.len() != size {
            continue;
        }
        if rows
            .iter()
            .any(|r| r.len() != size || r.bytes().any(|b| b != b'.' && b != b'#'))
        {
            continue;
        }
        out.push(rows.join("\n"));
    }
    out
}

/// Encode a template (rows joined by `\n`) as a bank line with metadata.
pub fn encode_line(tpl: &str, words: usize, cleans: usize) -> String {
    format!(
        "{};words={words};cleans={cleans}",
        tpl.trim().replace('\n', "/")
    )
}

/// Sample up to `n` distinct templates from the bank, uniformly without
/// replacement (partial Fisher–Yates over an index vec).
pub fn sample(bank: &[String], n: usize, rng: &mut Rng) -> Vec<String> {
    let n = n.min(bank.len());
    let mut idx: Vec<usize> = (0..bank.len()).collect();
    for i in 0..n {
        let j = i + (rng.next_u64() as usize) % (idx.len() - i);
        idx.swap(i, j);
    }
    idx[..n].iter().map(|&i| bank[i].clone()).collect()
}

/// Pipeline-default banked fraction of candidates: most of a full-size run
/// (proven shapes are the scarce resource there), none elsewhere or when the
/// caller wants pure exploration. Callers surface this as a tunable.
pub fn default_bank_fraction(size: usize) -> f64 {
    if size >= 13 {
        0.7
    } else {
        0.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_skips_junk_and_wrong_sizes() {
        let text = "# header prose never parses as a grid\n\
                    .../#../...;words=6;cleans=2\n\
                    ..../####/..../....\n\
                    ...x/..../..../....;bad\n\
                    .../.#./...\n";
        let b3 = parse_bank(text, 3);
        assert_eq!(b3.len(), 2, "two valid 3x3 lines (metadata optional)");
        assert_eq!(b3[0], "...\n#..\n...");
        assert_eq!(b3[1], "...\n.#.\n...");
        assert_eq!(parse_bank(text, 4).len(), 1, "row-count must match size");
        assert!(parse_bank("....\n", 4).is_empty(), "not a full grid line");
    }

    #[test]
    fn parse_keeps_templates_whose_first_cell_is_a_block() {
        // Regression: a leading '#' is a BLOCK CELL, not a comment marker —
        // the old comment rule silently hid a quarter of the real bank.
        let text = "#../.#./..#;words=6;cleans=1\n";
        let b = parse_bank(text, 3);
        assert_eq!(b, vec!["#..\n.#.\n..#".to_string()]);
    }

    #[test]
    fn encode_roundtrips_through_parse() {
        let tpl = "..#\n...\n#..";
        let line = encode_line(tpl, 6, 2);
        assert_eq!(line, "..#/.../#..;words=6;cleans=2");
        let parsed = parse_bank(&line, 3);
        assert_eq!(parsed, vec![tpl.to_string()]);
    }

    #[test]
    fn sample_is_distinct_and_bounded() {
        let bank: Vec<String> = (0..10).map(|i| format!("tpl{i}")).collect();
        let mut rng = Rng::new(7);
        let s = sample(&bank, 6, &mut rng);
        assert_eq!(s.len(), 6);
        let mut d = s.clone();
        d.sort();
        d.dedup();
        assert_eq!(d.len(), 6, "no repeats — sampling is without replacement");
        assert_eq!(sample(&bank, 99, &mut rng).len(), 10, "capped at bank size");
        assert!(sample(&[], 5, &mut rng).is_empty());
    }

    #[test]
    fn default_fraction_full_size_only() {
        assert_eq!(default_bank_fraction(15), 0.7);
        assert_eq!(default_bank_fraction(13), 0.7);
        assert_eq!(default_bank_fraction(9), 0.0);
        assert_eq!(default_bank_fraction(5), 0.0);
    }
}
