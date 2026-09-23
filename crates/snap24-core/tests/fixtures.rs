//! Differential test: the Rust solver must agree with the Python oracle on every
//! golden fixture. Regenerate `fixtures.json` with the command in
//! `tests/reference/oracle.py --help`; a mismatch here means the two
//! implementations disagree and one of them is wrong.

use snap24_core::{is_solvable, reachable, solve, Rational};

fn strings(value: &serde_json::Value) -> Vec<String> {
    value
        .as_array()
        .expect("json array")
        .iter()
        .map(|v| v.as_str().expect("json string").to_string())
        .collect()
}

#[test]
fn differential_against_oracle_fixtures() {
    let data: serde_json::Value = serde_json::from_str(include_str!("fixtures.json"))
        .expect("fixtures.json is valid json");
    assert_eq!(
        data["canonical_form"], "v1",
        "fixtures were generated with a different canonical form"
    );

    let hands = data["hands"].as_array().expect("hands array");
    assert!(!hands.is_empty(), "no fixtures to check");

    for hand in hands {
        let cards: Vec<i64> = hand["cards"]
            .as_array()
            .expect("cards array")
            .iter()
            .map(|v| v.as_i64().expect("card is i64"))
            .collect();
        let expected_reachable = strings(&hand["reachable"]);
        let got_reachable: Vec<String> = reachable(&cards).iter().map(|r| r.to_string()).collect();
        assert_eq!(
            got_reachable, expected_reachable,
            "reachable mismatch for {cards:?}"
        );

        for case in hand["cases"].as_array().expect("cases array") {
            let target: Rational = case["target"].as_str().expect("target string").parse().unwrap();
            let expected = strings(&case["solutions"]);
            let got = solve(&cards, target);
            assert_eq!(got, expected, "solution mismatch for {cards:?} -> {target}");
            assert_eq!(
                is_solvable(&cards, target),
                !expected.is_empty(),
                "is_solvable disagrees with fixtures for {cards:?} -> {target}"
            );
        }
    }
}

/// Ticket 03 performance bar: a full 5-card `solve` well under 10 ms in release.
/// Debug builds are far slower, so the assertion only applies in release.
#[test]
fn solve_five_cards_is_fast() {
    let cards = [1, 1, 1, 1, 8];
    let mut best = std::time::Duration::MAX;
    for _ in 0..50 {
        let start = std::time::Instant::now();
        let got = solve(&cards, 24);
        assert_eq!(got.len(), 10);
        best = best.min(start.elapsed());
    }
    if !cfg!(debug_assertions) {
        assert!(
            best < std::time::Duration::from_millis(10),
            "best 5-card solve took {best:?}, expected < 10ms"
        );
    }
}
