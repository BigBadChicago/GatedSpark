//! Prints the solver text for a hand-made puzzle, for reading by eye or piping into
//! a solver.
//!
//! A debugging tool, and part of no check. It exists because the one-piece half of
//! the loop rule was got right by printing the text and running `z3` over it before
//! any test existed, and the next person to change that encoding will want to do the
//! same.
//!
//! ```text
//! cargo run -p gs-slitherlink --example dump-query -- tworings sat | z3 -in
//! ```
//!
//! Fixtures: `full`, `sparse`, `none`, `tworings`. Queries: `sat`, `unique`.
use gs_slitherlink::board::Geometry;
use gs_slitherlink::clues::Clues;
use gs_slitherlink::loops::Loop;
use gs_slitherlink::rules::Slitherlink;

fn rect(g: Geometry, x0: u16, y0: u16, x1: u16, y1: u16) -> Loop {
    let mut drawn = vec![false; g.edge_count()];
    let mut mark = |e: Option<gs_slitherlink::board::EdgeId>| {
        if let Some(e) = e {
            drawn[e.0 as usize] = true;
        }
    };
    for x in x0..=x1 {
        mark(g.across(x, y0));
        mark(g.across(x, y1 + 1));
    }
    for y in y0..=y1 {
        mark(g.down(x0, y));
        mark(g.down(x1 + 1, y));
    }
    Loop::of(g, drawn).expect("a rectangle")
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let which = args.get(1).map_or("full", |s| s.as_str());
    let g = Geometry::new(4, 4).expect("a board");
    let answer = rect(g, 1, 1, 2, 2);
    let full = Clues::of(&answer);
    let clues = match which {
        "full" => full,
        "sparse" => {
            let mut c = full;
            for cell in g.cells() {
                if cell.0 % 2 == 0 {
                    c = c.without(cell);
                }
            }
            c
        }
        "none" => Clues::none(g),
        // Numbers worked out from a board of two separate rings. Every number is
        // satisfiable only by that board, which the loop rule forbids, so a solver
        // reading this must say unsat.
        "tworings" => {
            let mut drawn = vec![false; g.edge_count()];
            for (x0, y0, x1, y1) in [(0u16, 0u16, 0u16, 0u16), (2u16, 2u16, 3u16, 3u16)] {
                for x in x0..=x1 {
                    if let Some(e) = g.across(x, y0) {
                        drawn[e.0 as usize] = true;
                    }
                    if let Some(e) = g.across(x, y1 + 1) {
                        drawn[e.0 as usize] = true;
                    }
                }
                for y in y0..=y1 {
                    if let Some(e) = g.down(x0, y) {
                        drawn[e.0 as usize] = true;
                    }
                    if let Some(e) = g.down(x1 + 1, y) {
                        drawn[e.0 as usize] = true;
                    }
                }
            }
            let numbers = g
                .cells()
                .map(|cell| {
                    let sides = g.edges_of_cell(cell).expect("four");
                    Some(
                        u8::try_from(sides.iter().filter(|e| drawn[e.0 as usize]).count())
                            .expect("at most four"),
                    )
                })
                .collect();
            Clues::new(g, numbers).expect("a grid")
        }
        other => panic!("unknown fixture {other}"),
    };
    let p = Slitherlink::new(clues);
    let kind = args.get(2).map_or("sat", |s| s.as_str());
    let text = if kind == "unique" {
        gs_slitherlink::smtlib::uniqueness_query(&p, &answer.as_state()).expect("encodable")
    } else {
        gs_slitherlink::smtlib::solvability_query(&p).expect("encodable")
    };
    print!("{text}");
}
