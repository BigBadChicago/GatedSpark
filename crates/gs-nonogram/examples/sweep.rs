//! Measure what the generator actually produces across a range of settings.
//!
//! Run it:
//!
//! ```sh
//! cargo run --release --example sweep
//! ```
//!
//! Why this exists as a committed tool rather than a throwaway script. The three
//! numbers in `Shape::blobby` are a starting point, and the difficulty levels this
//! family will eventually define have to come from measurements of real output. The
//! rule inherited from the last project is that no number in a document is allowed
//! unless something regenerates it, so the thing that regenerates these numbers
//! belongs in the repository next to the code it measures.
//!
//! It is not a test and asserts nothing. It prints a table; a person reads it and
//! decides. `tests/generation.rs` holds the claims that must always hold.
//!
//! One column is here because of a mistake. An earlier version printed the forced-move
//! ratio and nothing else about work done, and a first reading of its output concluded
//! that the ratio spreads out on sparser boards. It does not: it is exactly 1000 for
//! every puzzle that needs no guessing, because the ratio is measured at the first
//! fixpoint and a line-solvable puzzle is finished there. All of the apparent spread
//! came from the handful of puzzles that needed a guess. The column that actually
//! separates one line-solvable puzzle from another is `propagation rounds`, so this
//! prints both and labels which population each describes.
//!
//! Release mode matters: the line solver probes every undecided cell both ways, so a
//! debug build of the larger sizes is slow enough to look broken.

use std::io::Write;

use gs_core::state::Dims;
use gs_nonogram::generate::{batch, Shape};
use gs_nonogram::solve::Budget;

/// Seeds per setting. Enough to see a trend, small enough to finish.
const SEEDS: u64 = 200;

fn main() {
    let mut out = std::io::stdout().lock();
    writeln!(
        out,
        "{SEEDS} seeds per row. 'guessing' counts kept puzzles that propagation alone \
         could not finish."
    )
    .ok();
    writeln!(
        out,
        "'rounds' is propagation rounds among the kept puzzles that need NO guessing: \
         min/median/max. That is the column that grades them."
    )
    .ok();
    writeln!(
        out,
        "'forced' is the forced-move ratio over ALL kept puzzles. It is 1000 for every \
         line-solvable one, so any spread shown is the guessing ones alone."
    )
    .ok();
    writeln!(
        out,
        "\n size  fill smooth |   kept | guessing | deepest | rounds (no-guess only) | forced"
    )
    .ok();

    for (w, h) in [(5u16, 5u16), (8, 8), (10, 10), (12, 12)] {
        for fill in [300u16, 400, 500, 600, 700] {
            for smoothing_rounds in [0u8, 1, 2, 3] {
                let shape = Shape {
                    dims: Dims {
                        width: w,
                        height: h,
                    },
                    fill_per_mille: fill,
                    smoothing_rounds,
                };
                let (kept, report) = batch(0..SEEDS, shape, Budget::default());

                let guessing = kept.iter().filter(|c| !c.features.line_solvable()).count();
                let deepest = kept
                    .iter()
                    .map(|c| c.features.max_search_depth)
                    .max()
                    .unwrap_or(0);
                let forced: Vec<u32> = kept
                    .iter()
                    .map(|c| c.features.forced_ratio_per_mille())
                    .collect();
                let low = forced.iter().copied().min().unwrap_or(0);
                let high = forced.iter().copied().max().unwrap_or(0);

                let mut rounds: Vec<u32> = kept
                    .iter()
                    .filter(|c| c.features.line_solvable())
                    .map(|c| c.features.propagation_rounds)
                    .collect();
                rounds.sort_unstable();
                let rounds_summary = if rounds.is_empty() {
                    "          none".to_string()
                } else {
                    let n = rounds.len();
                    format!(
                        "{:>2}/{:>2}/{:<2} ({} distinct)",
                        rounds[0],
                        rounds[n / 2],
                        rounds[n - 1],
                        {
                            let mut d = rounds.clone();
                            d.dedup();
                            d.len()
                        }
                    )
                };

                // Flushed per line on purpose. A long run should show its work as it
                // goes rather than appear to hang and then dump everything at once.
                writeln!(
                    out,
                    "{w:>2}x{h:<2} {fill:>5} {smoothing_rounds:>6} | {:>3}/{SEEDS} | \
                     {guessing:>8} | {deepest:>7} | {rounds_summary:>22} | {low:>4}..{high}",
                    report.kept
                )
                .ok();
                out.flush().ok();
            }
        }
    }
}
