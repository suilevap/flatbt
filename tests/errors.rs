//! Diagnostics go through one process-wide handler, so this binary holds a
//! single test.

use std::sync::Mutex;

use flatbt::{BtState, EntryMode, NodeResult, check, set_error_handler, update};

static REPORTED: Mutex<Vec<String>> = Mutex::new(Vec::new());

fn record(message: &dyn std::fmt::Display) {
    REPORTED.lock().unwrap().push(message.to_string());
}

#[test]
fn diagnostics_reach_the_installed_handler() {
    set_error_handler(record);
    fn pass(_: &()) -> bool {
        true
    }
    let tree = check(pass);
    let other = check(pass);
    let mut state: BtState<_, _> = BtState::new(&other);
    assert_eq!(
        update(&tree, &mut state, &mut (), EntryMode::Evaluate),
        NodeResult::Failure
    );
    assert_eq!(
        *REPORTED.lock().unwrap(),
        ["state belongs to a different root definition"]
    );
}
