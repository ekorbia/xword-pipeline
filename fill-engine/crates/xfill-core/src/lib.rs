//! xfill-core: quality-aware crossword fill engine.
//!
//! - [`bitset`]: fixed-size bitset (constraint-propagation primitive)
//! - [`wordlist`]: scored wordlist + compatibility masks
//! - [`grid`]: 2D template → geometry-agnostic [`grid::Puzzle`]
//! - [`solver`]: M1 DFS solver (MRV + score order)

pub mod bitset;
pub mod dup;
pub mod gen;
pub mod grid;
pub mod library;
pub mod solver;
pub mod util;
pub mod wordlist;

pub use grid::Puzzle;
pub use solver::{Lock, SolveConfig, SolveResult, Solver};
pub use wordlist::Wordlist;
