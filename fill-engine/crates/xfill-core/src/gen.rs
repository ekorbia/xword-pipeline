//! Generate VALID rotationally-symmetric grids: 180° symmetry, every run >= 3,
//! single connected white component, target block count. Random patterns are
//! mostly ugly-to-fill; pairing this with the solver as a screener (fill many,
//! keep the clean ones) is the product path.

use crate::util::Rng;

pub fn generate(n: usize, target_blocks: usize, rng: &mut Rng, max_outer: usize) -> Option<String> {
    for _ in 0..max_outer {
        let mut block = vec![vec![false; n]; n];
        let mut placed = 0usize;
        let center = (n / 2, n / 2);
        let mut attempts = 0;
        while placed < target_blocks && attempts < 3000 {
            attempts += 1;
            let r = rng.below(n);
            let c = rng.below(n);
            let (r2, c2) = (n - 1 - r, n - 1 - c);
            if block[r][c] {
                continue;
            }
            // A self-symmetric center cell only exists on ODD-sized grids;
            // on even sizes every cell has a distinct 180° partner.
            let is_center = n % 2 == 1 && (r, c) == center;
            if is_center && target_blocks - placed < 1 {
                continue;
            }
            if !is_center && target_blocks - placed < 2 {
                continue;
            }
            block[r][c] = true;
            if !is_center {
                block[r2][c2] = true;
            }
            let ok =
                runs_ok_after(&block, r, c, n) && (is_center || runs_ok_after(&block, r2, c2, n));
            if ok {
                placed += if is_center { 1 } else { 2 };
            } else {
                block[r][c] = false;
                if !is_center {
                    block[r2][c2] = false;
                }
            }
        }
        if placed + 1 >= target_blocks && all_runs_ok(&block, n) && connected(&block, n) {
            return Some(render(&block, n));
        }
    }
    None
}

/// Choose across-entry placements for theme answers of the given `lengths`.
/// Returns (row, col, len) per theme, or None if the count isn't supported.
///
/// Themes go on spread-out rows in the TOP half (their 180° mirror rows stay
/// theme-free), which keeps the block pattern symmetric for ANY mix of theme
/// lengths without theme-vs-theme conflicts. Columns are chosen close to
/// centered while keeping the side gaps legal (0 or >= 3 cells).
pub fn place_themes(n: usize, lengths: &[usize]) -> Option<Vec<(usize, usize, usize)>> {
    let k = lengths.len();
    // Rows are chosen to avoid mirror-row pairs (which would force conflicting
    // bounding blocks for unequal lengths). Each theme sits in the top half (or
    // the self-symmetric center row 7); its 180° mirror becomes an equal-length
    // NON-theme slot in the bottom half — so the grid stays symmetric and looks
    // balanced (long entries top and bottom) while theme answers fill cleanly.
    let rows: &[usize] = match k {
        1 => &[7],
        2 => &[3, 5],
        3 => &[2, 4, 6],
        4 => &[1, 3, 5, 7],
        _ => return None,
    };
    // Reject mirror-row pairs unless equal length (they'd force conflicting
    // bounding blocks). We pick row tables that only pair mirror rows when we
    // can also require equal lengths; to stay simple, only pair them when the
    // lengths actually match, else fail and let the caller adjust.
    let mut out = Vec::with_capacity(k);
    for (i, &len) in lengths.iter().enumerate() {
        let r = rows[i];
        if len > n {
            return None;
        }
        let col = find_start(n, len)?;
        out.push((r, col, len));
    }
    // Validate mirror-pair length compatibility: if row r and n-1-r are both
    // theme rows, their lengths must match (centered bounds mirror to equal len).
    for a in 0..k {
        for b in (a + 1)..k {
            if rows[a] + rows[b] == n - 1 && lengths[a] != lengths[b] {
                return None;
            }
        }
    }
    Some(out)
}

/// Closest-to-centered start column for a length-`len` across entry such that
/// the side gaps (between the bounding blocks and the grid edges) are legal.
fn find_start(n: usize, len: usize) -> Option<usize> {
    if len > n {
        return None;
    }
    let centered = (n - len) / 2;
    let mut best: Option<usize> = None;
    for start in 0..=(n - len) {
        let left_ok = start == 0 || start == 1 || start >= 4;
        let rend = start + len;
        let right_ok = rend == n || rend == n - 1 || rend + 4 <= n;
        if left_ok && right_ok {
            let d = (start as isize - centered as isize).unsigned_abs();
            if best.is_none_or(|b| d < (b as isize - centered as isize).unsigned_abs()) {
                best = Some(start);
            }
        }
    }
    best
}

fn themes_intact(block: &[Vec<bool>], placements: &[(usize, usize, usize)], n: usize) -> bool {
    for &(r, c, len) in placements {
        if c > 0 && !block[r][c - 1] {
            return false; // missing left bound
        }
        if c + len < n && !block[r][c + len] {
            return false; // missing right bound
        }
        for j in 0..len {
            if block[r][c + j] {
                return false; // interior block
            }
        }
    }
    true
}

/// Generate a valid symmetric grid containing the given across theme slots.
/// `placements` are (row, col, len); the answers themselves are locked in later.
pub fn generate_themed(
    n: usize,
    placements: &[(usize, usize, usize)],
    target_blocks: usize,
    rng: &mut Rng,
    max_outer: usize,
) -> Option<String> {
    // Cells that must stay white: theme cells and their 180° partners.
    let mut protected = vec![vec![false; n]; n];
    for &(r, c, len) in placements {
        if r >= n || c + len > n {
            return None;
        }
        for j in 0..len {
            protected[r][c + j] = true;
            protected[n - 1 - r][n - 1 - (c + j)] = true;
        }
    }
    // Seed the theme bounding blocks (and their symmetric partners).
    let mut seed = vec![vec![false; n]; n];
    let place = |block: &mut Vec<Vec<bool>>, r: usize, c: usize| -> bool {
        let (r2, c2) = (n - 1 - r, n - 1 - c);
        if protected[r][c] || protected[r2][c2] {
            return false;
        }
        block[r][c] = true;
        block[r2][c2] = true;
        true
    };
    for &(r, c, len) in placements {
        if c > 0 && !place(&mut seed, r, c - 1) {
            return None;
        }
        if c + len < n && !place(&mut seed, r, c + len) {
            return None;
        }
    }
    let seed_blocks: usize = seed.iter().flatten().filter(|&&b| b).count();

    for _ in 0..max_outer {
        let mut block = seed.clone();
        let mut placed = seed_blocks;
        let center = (n / 2, n / 2);
        let mut attempts = 0;
        while placed < target_blocks && attempts < 4000 {
            attempts += 1;
            let r = rng.below(n);
            let c = rng.below(n);
            let (r2, c2) = (n - 1 - r, n - 1 - c);
            if block[r][c] || protected[r][c] || protected[r2][c2] {
                continue;
            }
            // A self-symmetric center cell only exists on ODD-sized grids;
            // on even sizes every cell has a distinct 180° partner.
            let is_center = n % 2 == 1 && (r, c) == center;
            if is_center && target_blocks - placed < 1 {
                continue;
            }
            if !is_center && target_blocks - placed < 2 {
                continue;
            }
            block[r][c] = true;
            if !is_center {
                block[r2][c2] = true;
            }
            let ok =
                runs_ok_after(&block, r, c, n) && (is_center || runs_ok_after(&block, r2, c2, n));
            if ok {
                placed += if is_center { 1 } else { 2 };
            } else {
                block[r][c] = false;
                if !is_center {
                    block[r2][c2] = false;
                }
            }
        }
        if placed + 1 >= target_blocks
            && all_runs_ok(&block, n)
            && connected(&block, n)
            && themes_intact(&block, placements, n)
        {
            return Some(render(&block, n));
        }
    }
    None
}

fn runs_ok_after(block: &[Vec<bool>], r: usize, c: usize, n: usize) -> bool {
    // measure white segments adjacent to (r,c) in its row and column
    let mut left = 0;
    let mut cc = c as isize - 1;
    while cc >= 0 && !block[r][cc as usize] {
        left += 1;
        cc -= 1;
    }
    let mut right = 0;
    let mut cc = c + 1;
    while cc < n && !block[r][cc] {
        right += 1;
        cc += 1;
    }
    let mut up = 0;
    let mut rr = r as isize - 1;
    while rr >= 0 && !block[rr as usize][c] {
        up += 1;
        rr -= 1;
    }
    let mut down = 0;
    let mut rr = r + 1;
    while rr < n && !block[rr][c] {
        down += 1;
        rr += 1;
    }
    for seg in [left, right, up, down] {
        if seg > 0 && seg < 3 {
            return false;
        }
    }
    true
}

fn all_runs_ok(block: &[Vec<bool>], n: usize) -> bool {
    for row in block.iter().take(n) {
        let mut run = 0;
        // Range loop is the cleanest expression: 0..=n includes the sentinel
        // index `c == n` that closes any open run at the end of the row.
        #[allow(clippy::needless_range_loop)]
        for c in 0..=n {
            let b = c == n || row[c];
            if !b {
                run += 1;
            } else {
                if run > 0 && run < 3 {
                    return false;
                }
                run = 0;
            }
        }
    }
    for c in 0..n {
        let mut run = 0;
        for row in block.iter().take(n) {
            let b = row[c];
            if !b {
                run += 1;
            } else {
                if run > 0 && run < 3 {
                    return false;
                }
                run = 0;
            }
        }
        // sentinel pass equivalent to r == n in the original loop
        if run > 0 && run < 3 {
            return false;
        }
    }
    true
}

fn connected(block: &[Vec<bool>], n: usize) -> bool {
    let mut start = None;
    let mut total = 0;
    for (r, row) in block.iter().take(n).enumerate() {
        for (c, &cell) in row.iter().take(n).enumerate() {
            if !cell {
                total += 1;
                if start.is_none() {
                    start = Some((r, c));
                }
            }
        }
    }
    let Some(start) = start else { return false };
    let mut seen = vec![vec![false; n]; n];
    seen[start.0][start.1] = true;
    let mut stack = vec![start];
    let mut count = 0;
    while let Some((r, c)) = stack.pop() {
        count += 1;
        let mut nbrs = Vec::new();
        if r > 0 {
            nbrs.push((r - 1, c));
        }
        if r + 1 < n {
            nbrs.push((r + 1, c));
        }
        if c > 0 {
            nbrs.push((r, c - 1));
        }
        if c + 1 < n {
            nbrs.push((r, c + 1));
        }
        for (nr, nc) in nbrs {
            if !block[nr][nc] && !seen[nr][nc] {
                seen[nr][nc] = true;
                stack.push((nr, nc));
            }
        }
    }
    count == total
}

fn render(block: &[Vec<bool>], n: usize) -> String {
    let mut s = String::with_capacity(n * (n + 1));
    for row in block.iter().take(n) {
        for &cell in row.iter().take(n) {
            s.push(if cell { '#' } else { '.' });
        }
        s.push('\n');
    }
    s
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::grid::Puzzle;

    #[test]
    fn generates_valid_grid() {
        let mut rng = Rng::new(1);
        let t = generate(15, 36, &mut rng, 4000).expect("should generate");
        let p = Puzzle::from_template(&t);
        assert_eq!(p.orphan_cells(), 0, "no orphan cells");
        // block count near target
        let b = p.block_count();
        assert!((34..=38).contains(&b), "block count {b} near 36");
    }

    #[test]
    fn find_start_avoids_short_edge_runs() {
        // len 11 can't be centered (start 2 leaves a 1-cell edge run); a legal
        // start must exist.
        let s = find_start(15, 11).unwrap();
        let left_ok = s == 0 || s == 1 || s >= 4;
        let rend = s + 11;
        assert!(left_ok && (rend == 15 || rend == 14 || rend + 4 <= 15));
        // full-width and clean lengths center fine
        assert_eq!(find_start(15, 15), Some(0));
        assert_eq!(find_start(15, 13), Some(1));
    }

    #[test]
    fn generates_themed_grid_with_intact_slots() {
        use crate::grid::Dir;
        let lengths = [15usize, 13, 7];
        let placements = place_themes(15, &lengths).expect("placeable");
        let mut rng = Rng::new(7);
        let mut got = None;
        for _ in 0..40 {
            if let Some(t) = generate_themed(15, &placements, 40, &mut rng, 300) {
                got = Some(t);
                break;
            }
        }
        let t = got.expect("should generate a themed grid");
        let p = Puzzle::from_template(&t);
        assert_eq!(p.orphan_cells(), 0);
        // every theme placement must be a real across entry of that exact length
        for &(r, c, len) in &placements {
            let ei = p
                .find_entry(r, c, Dir::Across)
                .expect("theme slot is an entry");
            assert_eq!(p.entries[ei].len, len, "theme slot has expected length");
        }
    }
}
