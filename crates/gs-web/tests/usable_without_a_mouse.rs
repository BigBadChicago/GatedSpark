//! Accessibility, held by a test rather than by good intentions.
//!
//! The small-team plan puts accessibility on the list of things that must not be cut,
//! and then it was cut: labels and keyboard handlers were written once and nothing
//! noticed when they stopped matching the page. That is the failure this file exists to
//! make loud. A page that quietly stops working for somebody using a keyboard, or
//! listening rather than looking, looks perfect in every screenshot and passes every
//! other test in this repository.
//!
//! **What this test can and cannot prove.** It reads `www/index.html` as text and
//! checks that the pieces which make the page usable are still there. It cannot prove
//! they still *work*: only a real browser can do that, and
//! `scripts/drive-page.mjs` does it by playing a whole puzzle with the keyboard alone.
//! This test is the net underneath, and it is here rather than there because it needs
//! no browser and so runs on every push. Deleting a line it checks for fails here in
//! seconds; breaking the behaviour behind that line fails in the browser check.
//!
//! When one of these fails, the fix is never to change the test to match the page. The
//! test says what the page owes a player who cannot use a mouse or cannot see it.

use std::path::Path;

fn page() -> String {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("www/index.html");
    std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("reading {}: {e}", path.display()))
}

/// The text of one function in the page's script, so a check can say "inside `refresh`"
/// rather than "somewhere in the file". Brace counting is enough here because the page
/// is one script we control; it is not a JavaScript parser and is not trying to be.
fn body_of(source: &str, signature: &str) -> String {
    let start = source
        .find(signature)
        .unwrap_or_else(|| panic!("the page no longer has `{signature}`"));
    let after = &source[start + signature.len()..];
    let open = after
        .find('{')
        .unwrap_or_else(|| panic!("`{signature}` has no body"));
    let mut depth = 0usize;
    for (i, c) in after[open..].char_indices() {
        match c {
            '{' => depth += 1,
            '}' => {
                depth -= 1;
                if depth == 0 {
                    return after[open..open + i + 1].to_string();
                }
            }
            _ => {}
        }
    }
    panic!("`{signature}` has no closing brace");
}

/// Every square must be reachable by keyboard and must answer Enter and Space.
///
/// A board that can only be tapped is a board that cannot be played at all without a
/// pointing device. There is no partial version of this: one missing half makes the
/// other half useless.
#[test]
fn the_picture_board_can_be_reached_and_pressed_with_a_keyboard() {
    let page = page();
    assert!(
        page.contains("td.tabIndex = 0;"),
        "the squares are no longer in the keyboard's path, so the picture game cannot \
         be played without a mouse"
    );
    let keys = body_of(&page, r#"el("board").addEventListener("keydown","#);
    assert!(
        keys.contains("Enter") && keys.contains('"') && keys.contains("tap("),
        "the squares no longer answer a key press: {keys}"
    );
}

/// The same for the loop game's lines.
#[test]
fn the_loop_board_can_be_reached_and_pressed_with_a_keyboard() {
    let page = page();
    assert!(
        page.contains("tabindex: 0"),
        "the lines are no longer in the keyboard's path, so the loop game cannot be \
         played without a mouse"
    );
    let keys = body_of(&page, r#"el("loop").addEventListener("keydown","#);
    assert!(
        keys.contains("Enter") && keys.contains("tapLine("),
        "the lines no longer answer a key press: {keys}"
    );
}

/// Every state a square or a line can be in must have a word for it.
///
/// The board shows a square's state by how it is drawn. Somebody listening to the page
/// gets nothing at all from that, so the state has to be in the square's name too. The
/// risk this guards is a fourth state gaining a look and not gaining a word: it would
/// be visible to everybody who can see the board and invisible to everybody else.
#[test]
fn every_state_of_a_square_and_a_line_has_a_word_for_it() {
    let page = page();

    let squares = body_of(&page, "const SQUARE_SAID =");
    for state in ["Square.Unknown", "Square.Filled", "Square.Empty"] {
        assert!(
            squares.contains(state),
            "a square can be {state} and the page has no word for it: {squares}"
        );
    }

    let lines = body_of(&page, "const LINE_SAID =");
    for state in ["Drawn.Unknown", "Drawn.Yes", "Drawn.No"] {
        assert!(
            lines.contains(state),
            "a line can be {state} and the page has no word for it: {lines}"
        );
    }

    // The cycle a tap walks through and the words must cover the same states. If a tap
    // can reach a state the word table does not know, that state is silent.
    let tapped = body_of(&page, "const NEXT =");
    for state in ["Square.Unknown", "Square.Filled", "Square.Empty"] {
        assert!(
            tapped.contains(state) == squares.contains(state),
            "tapping can reach {state} but nothing says it out loud, or the other way \
             round"
        );
    }
}

/// The name of a square must be rebuilt every time the board is, not only when tapped.
///
/// The core changes squares the player never touched: undo, redo, and opening a game
/// saved on this device all rewrite the whole board. A name updated only where the tap
/// landed drifts away from the board it describes, and the player listening is then
/// told something false, which is worse than being told nothing.
#[test]
fn the_name_of_a_square_is_rebuilt_whenever_the_board_is() {
    let page = page();

    let refresh = body_of(&page, "function refresh()");
    assert!(
        refresh.contains("squareName("),
        "`refresh` sets how a square looks without setting what it is called, so the \
         two can disagree after an undo: {refresh}"
    );

    let loop_refresh = body_of(&page, "function refreshLoop()");
    assert!(
        loop_refresh.contains("lineName("),
        "`refreshLoop` sets how a line looks without setting what it is called: \
         {loop_refresh}"
    );
}

/// Anything the page says to the player must announce itself.
///
/// These lines change without anything else on the page changing. To a screen reader a
/// silent change is no change at all, so without this the solved message, the refusal
/// and the running commentary are seen and not heard.
#[test]
fn everything_the_page_says_is_announced_rather_than_only_shown() {
    let page = page();
    // `refused` is the one that takes the board away, so it interrupts; the rest wait
    // for a pause.
    for id in ["solved", "note", "shape", "saved-line"] {
        let tag = line_with(&page, &format!("id=\"{id}\""));
        assert!(
            tag.contains("aria-live=\"polite\""),
            "`{id}` speaks to the player and is never read aloud: {tag}"
        );
    }
    let refused = line_with(&page, "id=\"refused\"");
    assert!(
        refused.contains("role=\"alert\""),
        "a refusal takes the board away and must interrupt rather than wait: {refused}"
    );
}

/// Both pickers must say what they are.
///
/// An unlabelled menu is read out as "menu", which tells somebody choosing between two
/// of them nothing whatsoever.
#[test]
fn both_pickers_say_what_they_choose() {
    let page = page();
    for id in ["game-pick", "day-pick"] {
        assert!(
            page.contains(&format!("for=\"{id}\"")),
            "`{id}` has no label, so it is read out as just \"menu\""
        );
    }
}

/// The clue numbers must say which row or column they belong to.
#[test]
fn the_clue_numbers_belong_to_a_stated_row_or_column() {
    let page = page();
    assert!(
        page.contains("th.scope = \"col\""),
        "the column clues no longer say which column they are for"
    );
    assert!(
        page.contains("scope: \"row\""),
        "the row clues no longer say which row they are for"
    );
}

/// The whole line of the file containing `needle`, for a readable failure.
fn line_with(page: &str, needle: &str) -> String {
    page.lines()
        .find(|line| line.contains(needle))
        .unwrap_or_else(|| panic!("the page no longer contains `{needle}`"))
        .trim()
        .to_string()
}
