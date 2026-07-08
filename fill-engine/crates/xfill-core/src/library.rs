//! Shared grid-library artifact: the vetted JSON the daily-puzzle pipeline (clue
//! writing, scheduling) consumes. Used by both the `library` (themeless) and
//! `theme` (themed) binaries so they emit an identical schema.

use crate::grid::{Dir, Puzzle};
use crate::solver::{SolveResult, SolvedFill};
use std::collections::HashSet;

pub struct LibEntry {
    pub num: u32,
    pub dir: char, // 'A' | 'D'
    pub row: usize,
    pub col: usize,
    pub len: usize,
    pub answer: String,
    pub score: u8,
    pub theme: bool,
}

pub struct LibGrid {
    pub blocks: usize,
    pub mean: f64,
    pub min: u8,
    pub iffy: usize,
    pub themed: bool,
    pub template: Vec<String>,
    pub fill: Vec<String>,
    pub entries: Vec<LibEntry>,
}

/// Build a library record from a solve result's primary (best-by-mean) fill.
/// `theme_ids` are the entry ids that were locked theme answers (empty for
/// themeless). Returns None if the result isn't a solved fill.
pub fn build_lib_grid(p: &Puzzle, r: &SolveResult, theme_ids: &HashSet<usize>) -> Option<LibGrid> {
    let (mean, min, iffy) = match (r.mean_score, r.min_score, r.iffy_count) {
        (Some(m), Some(mn), Some(i)) => (m, mn, i),
        _ => return None,
    };
    let (letters, fill) = match (r.letters.as_deref(), r.fill.as_ref()) {
        (Some(l), Some(f)) => (l, f),
        _ => return None,
    };
    Some(build_from_parts(
        p, letters, mean, min, iffy, fill, theme_ids,
    ))
}

/// Build a library record from a specific fill (e.g. the solver's `clean`
/// alternative when it passes the caller's keep gates).
pub fn build_lib_grid_from(p: &Puzzle, f: &SolvedFill, theme_ids: &HashSet<usize>) -> LibGrid {
    build_from_parts(
        p,
        &f.letters,
        f.mean_score,
        f.min_score,
        f.iffy_count,
        &f.fill,
        theme_ids,
    )
}

fn build_from_parts(
    p: &Puzzle,
    letters: &[Option<u8>],
    mean: f64,
    min: u8,
    iffy: usize,
    fill: &[(usize, String, u8)],
    theme_ids: &HashSet<usize>,
) -> LibGrid {
    let nums = p.number_entries();
    let mut entries: Vec<LibEntry> = fill
        .iter()
        .map(|(ei, ans, sc)| {
            let e = &p.entries[*ei];
            LibEntry {
                num: nums[*ei],
                dir: if e.dir == Dir::Across { 'A' } else { 'D' },
                row: e.row,
                col: e.col,
                len: e.len,
                answer: ans.clone(),
                score: *sc,
                theme: theme_ids.contains(ei),
            }
        })
        .collect();
    entries.sort_by_key(|g| (g.num, g.dir));
    LibGrid {
        blocks: p.block_count(),
        mean,
        min,
        iffy,
        themed: !theme_ids.is_empty(),
        template: p.render(None).lines().map(str::to_string).collect(),
        fill: p
            .render(Some(letters))
            .lines()
            .map(str::to_string)
            .collect(),
        entries,
    }
}

/// Which member(s) of a root-duplicate pair to try banning for a refill
/// retry, most-disposable first: non-theme members only (theme answers are
/// locked and can't be banned), lower score first, shorter on ties. Empty if
/// neither member is a searchable fill entry.
pub fn dup_ban_targets<'a>(g: &'a LibGrid, a: &str, b: &str) -> Vec<&'a str> {
    let mut cands: Vec<&LibEntry> = g
        .entries
        .iter()
        .filter(|e| !e.theme && (e.answer == a || e.answer == b))
        .collect();
    cands.sort_by_key(|e| (e.score, e.answer.len()));
    cands.into_iter().map(|e| e.answer.as_str()).collect()
}

fn esc(s: &str) -> String {
    s.replace('\\', "\\\\").replace('"', "\\\"")
}

fn json_arr(rows: &[String]) -> String {
    rows.iter()
        .map(|r| format!("\"{}\"", esc(r)))
        .collect::<Vec<_>>()
        .join(", ")
}

/// Serialize a library to JSON. `themes` lists the theme answers (empty for a
/// themeless library) and is recorded in the metadata.
pub fn write_json(
    path: &str,
    grids: &[LibGrid],
    wordlist: &str,
    target_blocks: usize,
    themes: &[String],
) -> std::io::Result<()> {
    let mut s = String::new();
    s.push_str("{\n");
    s.push_str(&format!("  \"wordlist\": \"{}\",\n", esc(wordlist)));
    s.push_str(&format!("  \"target_blocks\": {target_blocks},\n"));
    s.push_str(&format!("  \"themed\": {},\n", !themes.is_empty()));
    s.push_str(&format!("  \"themes\": [{}],\n", json_arr(themes)));
    s.push_str(&format!("  \"count\": {},\n", grids.len()));
    s.push_str("  \"grids\": [\n");
    for (gi, g) in grids.iter().enumerate() {
        s.push_str("    {\n");
        s.push_str(&format!("      \"id\": {gi},\n"));
        s.push_str(&format!("      \"blocks\": {},\n", g.blocks));
        s.push_str(&format!("      \"themed\": {},\n", g.themed));
        s.push_str(&format!("      \"mean_score\": {:.2},\n", g.mean));
        s.push_str(&format!("      \"min_score\": {},\n", g.min));
        s.push_str(&format!("      \"iffy\": {},\n", g.iffy));
        s.push_str(&format!(
            "      \"template\": [{}],\n",
            json_arr(&g.template)
        ));
        s.push_str(&format!("      \"fill\": [{}],\n", json_arr(&g.fill)));
        s.push_str("      \"entries\": [\n");
        for (ei, e) in g.entries.iter().enumerate() {
            s.push_str(&format!(
                "        {{\"num\": {}, \"dir\": \"{}\", \"row\": {}, \"col\": {}, \"len\": {}, \"answer\": \"{}\", \"score\": {}, \"theme\": {}}}{}\n",
                e.num, e.dir, e.row, e.col, e.len, esc(&e.answer), e.score, e.theme,
                if ei + 1 < g.entries.len() { "," } else { "" }
            ));
        }
        s.push_str("      ]\n");
        s.push_str(if gi + 1 < grids.len() {
            "    },\n"
        } else {
            "    }\n"
        });
    }
    s.push_str("  ]\n}\n");
    if let Some(parent) = std::path::Path::new(path).parent() {
        if !parent.as_os_str().is_empty() {
            std::fs::create_dir_all(parent)?;
        }
    }
    std::fs::write(path, s)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(answer: &str, score: u8, theme: bool) -> LibEntry {
        LibEntry {
            num: 1,
            dir: 'A',
            row: 0,
            col: 0,
            len: answer.len(),
            answer: answer.to_string(),
            score,
            theme,
        }
    }

    fn grid(entries: Vec<LibEntry>) -> LibGrid {
        LibGrid {
            blocks: 0,
            mean: 0.0,
            min: 0,
            iffy: 0,
            themed: false,
            template: Vec::new(),
            fill: Vec::new(),
            entries,
        }
    }

    #[test]
    fn dup_ban_targets_prefers_disposable_member() {
        let g = grid(vec![
            entry("ARENA", 60, false),
            entry("RENA", 50, false),
            entry("SLAMDUNK", 90, true),
        ]);
        // Lower score first, so the junkier member is banned before the keeper.
        assert_eq!(dup_ban_targets(&g, "ARENA", "RENA"), vec!["RENA", "ARENA"]);
        // A theme member is locked → only the fill member is bannable.
        assert_eq!(dup_ban_targets(&g, "SLAMDUNK", "ARENA"), vec!["ARENA"]);
        // Equal scores tie-break shorter-first.
        let g = grid(vec![entry("ILLS", 50, false), entry("ILL", 50, false)]);
        assert_eq!(dup_ban_targets(&g, "ILL", "ILLS"), vec!["ILL", "ILLS"]);
    }
}
