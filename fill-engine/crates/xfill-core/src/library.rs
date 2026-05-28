//! Shared grid-library artifact: the vetted JSON the daily-puzzle pipeline (clue
//! writing, scheduling) consumes. Used by both the `library` (themeless) and
//! `theme` (themed) binaries so they emit an identical schema.

use crate::grid::{Dir, Puzzle};
use crate::solver::SolveResult;
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

/// Build a library record from a solved puzzle. `theme_ids` are the entry ids
/// that were locked theme answers (empty for themeless). Returns None if the
/// result isn't a solved fill.
pub fn build_lib_grid(p: &Puzzle, r: &SolveResult, theme_ids: &HashSet<usize>) -> Option<LibGrid> {
    let (mean, min, iffy, fill) = match (r.mean_score, r.min_score, r.iffy_count, r.fill.as_ref()) {
        (Some(m), Some(mn), Some(i), Some(f)) => (m, mn, i, f),
        _ => return None,
    };
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
    Some(LibGrid {
        blocks: p.block_count(),
        mean,
        min,
        iffy,
        themed: !theme_ids.is_empty(),
        template: p.render(None).lines().map(str::to_string).collect(),
        fill: p
            .render(r.letters.as_deref())
            .lines()
            .map(str::to_string)
            .collect(),
        entries,
    })
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
