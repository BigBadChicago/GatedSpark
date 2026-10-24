//! Measure how often generation produces a usable puzzle, and at what quality.
//!
//! The plan asks two questions of every family at this stage: how often generation
//! produces a usable puzzle, and how long verification takes. This answers the first
//! for Kakuro, and records the quality number the method is weakest on.
//!
//! Run with `cargo run --release --example sweep -p gs-kakuro`.

use std::time::Instant;

use gs_kakuro::generate::{batch, given_away_per_mille, Shape};
use gs_kakuro::solve::Budget;

fn main() {
    let seeds = 40_u64;
    println!(
        "{:>7} {:>6} {:>7} {:>8} {:>7} {:>8} {:>10}",
        "board", "kept", "/1000", "seconds", "whites", "repairs", "free/1000"
    );
    for (w, h) in [(4, 4), (5, 5), (6, 6), (7, 7), (8, 8), (9, 9)] {
        let shape = Shape::middling(w, h);
        let started = Instant::now();
        let (kept, report) = batch(0..seeds, shape, Budget::default());
        let elapsed = started.elapsed();
        assert!(report.is_consistent(), "{report:?}");

        let n = u32::try_from(kept.len()).unwrap_or(u32::MAX).max(1);
        let whites = kept
            .iter()
            .map(|c| u32::try_from(c.puzzle.layout.white_count()).unwrap_or(0))
            .sum::<u32>()
            / n;
        let repairs = kept.iter().map(|c| c.repairs).sum::<u32>() / n;
        let free = kept
            .iter()
            .map(|c| given_away_per_mille(&c.puzzle.layout))
            .sum::<u32>()
            / n;
        println!(
            "{:>7} {:>6} {:>7} {:>8.2} {:>7} {:>8} {:>10}",
            format!("{w}x{h}"),
            report.kept,
            report.kept_per_mille(),
            elapsed.as_secs_f64(),
            whites,
            repairs,
            free
        );
    }
}
