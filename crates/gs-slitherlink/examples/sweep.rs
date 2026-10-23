//! Measure what the generator produces, and what it costs.
//!
//! The Nonogram sweep (`gs-nonogram/examples/sweep.rs`) existed because the numbers
//! that decide a difficulty design cannot be guessed, and the first reading of them
//! was wrong. This is the same tool for this family, and its most important column is
//! **questions**: how many uniqueness questions the removal loop asks per accepted
//! puzzle. That number decides how fast a pool can be filled, and it is the one real
//! difference in cost between this family and Nonogram.
//!
//! ```text
//! cargo run --release -p gs-slitherlink --example sweep
//! ```

use std::time::Instant;

use gs_slitherlink::generate::{batch, Shape, REMOVAL_BUDGET};

fn main() {
    let seeds = std::env::args()
        .nth(1)
        .and_then(|a| a.parse::<u64>().ok())
        .unwrap_or(40);
    println!(
        "{:>7} {:>8} {:>6} {:>6} {:>9} {:>8} {:>7} {:>7} {:>7}",
        "board", "enclosed", "kept", "/1000", "questions", "clues", "removed", "guesses", "ms"
    );
    for (w, h) in [(4u16, 4u16), (5, 5), (7, 7), (10, 10), (12, 12)] {
        for enclosed in [200u32, 400, 600] {
            let shape = Shape {
                width: w,
                height: h,
                enclosed_per_mille: enclosed,
            };
            let started = Instant::now();
            let (kept, report) = batch(0..seeds, shape, REMOVAL_BUDGET);
            let elapsed = started.elapsed().as_millis();
            let n = u32::try_from(kept.len().max(1)).unwrap_or(1);
            let clues: u32 = kept.iter().map(|c| c.puzzle.clues.given_count()).sum();
            let removed: u32 = kept.iter().map(|c| c.clues_removed).sum();
            let guesses: u32 = kept.iter().map(|c| c.features.decision_points).sum();
            let questions = report.questions_asked / n;
            println!(
                "{:>7} {:>8} {:>6} {:>6} {:>9} {:>8} {:>7} {:>7} {:>7}",
                format!("{w}x{h}"),
                enclosed,
                report.kept,
                report.kept_per_mille(),
                questions,
                clues / n,
                removed / n,
                guesses / n,
                elapsed
            );
            assert!(report.is_consistent(), "{report:?}");
        }
    }
}
