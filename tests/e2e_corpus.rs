mod common;

use common::run_php;

#[test]
fn order_pipeline_matches_reference_php_result() {
    let source = include_str!("../benches/corpus_order_pipeline.php")
        .replace("runQuotePipeline(500000)", "runQuotePipeline(5000)");
    let output = run_php(&source);
    let (result, _) = output
        .split_once('|')
        .expect("corpus benchmark must separate result from elapsed time");

    // Reference result from PHP 8.4.12 with `php -n`.
    assert_eq!(result, "98957780,13275352,112233132,2100");
}

#[test]
fn typed_order_pipeline_matches_reference_php_result() {
    let source = include_str!("../benches/corpus_typed_order_pipeline.php").replace(
        "runTypedQuotePipeline(500000)",
        "runTypedQuotePipeline(5000)",
    );
    let output = run_php(&source);
    let (result, _) = output
        .split_once('|')
        .expect("typed corpus benchmark must separate result from elapsed time");

    assert_eq!(result, "98957780,13275352,112233132,2100");
}

#[test]
fn stateful_ledger_pipeline_matches_reference_php_result() {
    let source = include_str!("../benches/corpus_ledger_pipeline.php")
        .replace("runLedgerPipeline(500000)", "runLedgerPipeline(5000)");
    let output = run_php(&source);
    let (result, _) = output
        .split_once('|')
        .expect("ledger corpus benchmark must separate result from elapsed time");

    assert_eq!(result, "5000,78312500,2752500,1752");
}

#[test]
fn typed_stateful_ledger_pipeline_matches_reference_php_result() {
    let source = include_str!("../benches/corpus_typed_ledger_pipeline.php").replace(
        "runTypedLedgerPipeline(500000)",
        "runTypedLedgerPipeline(5000)",
    );
    let output = run_php(&source);
    let (result, _) = output
        .split_once('|')
        .expect("typed ledger corpus benchmark must separate result from elapsed time");

    assert_eq!(result, "5000,78312500,2752500,1752");
}
