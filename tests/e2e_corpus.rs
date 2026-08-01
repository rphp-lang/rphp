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
