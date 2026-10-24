//! Measure how often generation produces a usable puzzle, and at what quality.
//!
//! The plan asks two questions of every family at this stage: how often generation
//! produces a usable puzzle, and how long verification takes. This answers the first for
//! Kakuro.
//!
//! It measures **both** methods against each other, which is the point of it now. ADR
//! 0019's method draws a random block pattern and repairs it; ADR 0021's builds the board
//! so that no run is one square long, and settles uniqueness with the digits. The columns
//! that matter are `free/1000`, which is the share of white squares handed to the player
//! rather than worked out, and `ends/1000`, which is the share of digits that are ones or
//! nines. The first is what the built method fixed. The second is what it cost.
//!
//! The older method is ruinously slow above 7 by 7 — ADR 0019 measured about 6.5 seconds
//! per kept puzzle at 9 by 9 — so it is only measured where that is affordable.
//!
//! Run with `cargo run --release --example sweep -p gs-kakuro`.

use std::time::Instant;

use gs_kakuro::construct::swap_rectangles;
use gs_kakuro::generate::{batch, given_away_per_mille, Shape};
use gs_kakuro::solve::Budget;

fn main() {
    let seeds = 40_u64;
    println!(
        "{:>7} {:>8} {:>6} {:>7} {:>8} {:>7} {:>7} {:>10} {:>10} {:>6}",
        "board",
        "method",
        "kept",
        "/1000",
        "seconds",
        "whites",
        "rounds",
        "free/1000",
        "ends/1000",
        "rects"
    );
    for (w, h) in [(4, 4), (5, 5), (6, 6), (7, 7), (8, 8), (9, 9)] {
        let affordable = w <= 7;
        let methods = [
            ("random", Shape::random_pattern(w, h), affordable),
            ("built", Shape::middling(w, h), true),
        ];
        for (name, shape, run_it) in methods {
            if !run_it {
                continue;
            }
            let started = Instant::now();
            let (kept, report) = batch(0..seeds, shape, Budget::default());
            let elapsed = started.elapsed();
            assert!(report.is_consistent(), "{report:?}");

            let n = u32::try_from(kept.len()).unwrap_or(u32::MAX).max(1);
            let average = |of: &dyn Fn(&gs_kakuro::generate::Candidate) -> u32| -> u32 {
                kept.iter().map(of).sum::<u32>() / n
            };
            println!(
                "{:>7} {:>8} {:>6} {:>7} {:>8.2} {:>7} {:>7} {:>10} {:>10} {:>6}",
                format!("{w}x{h}"),
                name,
                report.kept,
                report.kept_per_mille(),
                elapsed.as_secs_f64(),
                average(&|c| u32::try_from(c.puzzle.layout.white_count()).unwrap_or(0)),
                average(&|c| c.repairs),
                average(&|c| given_away_per_mille(&c.puzzle.layout)),
                average(&|c| c.features.digits_at_the_ends_per_mille),
                average(&|c| swap_rectangles(&c.puzzle.layout)),
            );
        }
    }
}
