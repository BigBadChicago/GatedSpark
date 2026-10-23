//! The arithmetic between cells, dots and edges.
//!
//! Three index spaces over one board is the main thing that can be quietly wrong
//! here, and a wrong lookup does not crash: it makes a clue count the wrong cell's
//! sides, which reads as a hard puzzle rather than as a fault. So these tests check
//! the lookups against each other rather than against numbers copied out of the
//! implementation.

use gs_slitherlink::board::{Along, EdgeId, Geometry, SizeError};

fn board(w: u16, h: u16) -> Geometry {
    Geometry::new(w, h).expect("a sensible board")
}

#[test]
fn counts_match_the_shape() {
    let g = board(3, 2);
    assert_eq!(g.cell_count(), 6);
    assert_eq!(g.dot_count(), 4 * 3);
    // 3 across on each of 3 rows of dots, 4 down on each of 2 rows of cells.
    assert_eq!(g.across_count(), 9);
    assert_eq!(g.down_count(), 8);
    assert_eq!(g.edge_count(), 17);
}

#[test]
fn every_edge_number_is_used_exactly_once() {
    let g = board(4, 3);
    let mut seen = vec![0u32; g.edge_count()];
    for y in 0..g.dots_down() {
        for x in 0..g.width() {
            let edge = g.across(x, y).expect("an edge across");
            seen[edge.0 as usize] += 1;
        }
    }
    for y in 0..g.height() {
        for x in 0..g.dots_across() {
            let edge = g.down(x, y).expect("an edge down");
            seen[edge.0 as usize] += 1;
        }
    }
    assert!(
        seen.iter().all(|c| *c == 1),
        "every edge number is produced exactly once: {seen:?}"
    );
}

#[test]
fn edge_at_is_the_inverse_of_across_and_down() {
    let g = board(5, 4);
    for y in 0..g.dots_down() {
        for x in 0..g.width() {
            let edge = g.across(x, y).expect("across");
            assert_eq!(g.edge_at(edge), Some((Along::Across, x, y)));
        }
    }
    for y in 0..g.height() {
        for x in 0..g.dots_across() {
            let edge = g.down(x, y).expect("down");
            assert_eq!(g.edge_at(edge), Some((Along::Down, x, y)));
        }
    }
}

#[test]
fn an_edge_number_off_the_board_is_nothing() {
    let g = board(2, 2);
    let past_the_end = EdgeId(u32::try_from(g.edge_count()).expect("small"));
    assert_eq!(g.edge_at(past_the_end), None);
}

#[test]
fn a_cell_has_four_different_sides_wherever_it_sits() {
    let g = board(3, 3);
    for cell in g.cells() {
        let sides = g.edges_of_cell(cell).expect("four sides");
        let mut sorted = sides.to_vec();
        sorted.sort_unstable();
        sorted.dedup();
        assert_eq!(sorted.len(), 4, "cell {cell:?} has four distinct sides");
    }
}

#[test]
fn a_cells_sides_meet_its_own_four_corners() {
    // The lookup worth checking across modules: the sides of a cell must join the
    // dots at that cell's corners, and nothing else. An off-by-one here is exactly
    // the fault that makes a clue count a neighbour's sides.
    let g = board(4, 4);
    for cell in g.cells() {
        let (cx, cy) = g.cell_at(cell).expect("a place");
        let mut corners: Vec<_> = [(cx, cy), (cx + 1, cy), (cx, cy + 1), (cx + 1, cy + 1)]
            .iter()
            .map(|(x, y)| g.dot(*x, *y).expect("a corner"))
            .collect();
        corners.sort_unstable();
        for side in g.edges_of_cell(cell).expect("four sides") {
            let (a, b) = g.ends_of(side).expect("two ends");
            assert!(
                corners.contains(&a) && corners.contains(&b),
                "side {side:?} of cell {cell:?} joins {a:?} and {b:?}, not its own corners"
            );
        }
    }
}

#[test]
fn a_dot_and_its_edges_agree_with_each_other() {
    let g = board(3, 4);
    for dot in g.dots() {
        for edge in g.edges_of_dot(dot) {
            let (a, b) = g.ends_of(edge).expect("two ends");
            assert!(
                a == dot || b == dot,
                "edge {edge:?} is listed at dot {dot:?} but joins {a:?} and {b:?}"
            );
        }
    }
    // Every edge is listed at both of its ends, which is what the flood fill in
    // `loops` relies on to walk from one dot to the next.
    for edge in g.edges() {
        let (a, b) = g.ends_of(edge).expect("two ends");
        assert!(g.edges_of_dot(a).contains(&edge));
        assert!(g.edges_of_dot(b).contains(&edge));
    }
}

#[test]
fn corner_dots_have_two_edges_and_middle_dots_have_four() {
    let g = board(3, 3);
    assert_eq!(g.edges_of_dot(g.dot(0, 0).expect("corner")).len(), 2);
    assert_eq!(g.edges_of_dot(g.dot(3, 3).expect("corner")).len(), 2);
    assert_eq!(g.edges_of_dot(g.dot(1, 0).expect("rim")).len(), 3);
    assert_eq!(g.edges_of_dot(g.dot(2, 2).expect("middle")).len(), 4);
}

#[test]
fn a_board_with_no_cells_is_refused() {
    assert_eq!(
        Geometry::new(0, 5),
        Err(SizeError::TooSmall {
            width: 0,
            height: 5
        })
    );
    assert_eq!(
        Geometry::new(5, 0),
        Err(SizeError::TooSmall {
            width: 5,
            height: 0
        })
    );
}

#[test]
fn a_board_too_big_to_index_is_refused_not_wrapped() {
    // The refusal matters more than the limit. A wrapped edge number would put two
    // different edges at one address, and every check on the board would pass.
    let huge = Geometry::new(1000, 1000);
    assert!(matches!(huge, Err(SizeError::TooBig { .. })), "{huge:?}");
    // And the largest board that does fit really does fit.
    let big = Geometry::new(180, 180).expect("180 square fits");
    assert!(u16::try_from(big.edge_count()).is_ok());
}

#[test]
fn the_state_is_edge_shaped() {
    let g = board(10, 10);
    let dims = g.state_dims();
    assert_eq!(usize::from(dims.width), g.edge_count());
    assert_eq!(dims.height, 1);
    assert_eq!(dims.cell_count(), 220);
}
