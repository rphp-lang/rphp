use std::process::Command;

fn run(source: &str) -> std::process::Output {
    Command::new(env!("CARGO_BIN_EXE_rphp"))
        .args([
            "-n",
            "-d",
            "memory_limit=2M",
            "-d",
            "display_errors=0",
            "-d",
            "log_errors=0",
            "-r",
            source,
        ])
        .output()
        .expect("run bounded allocation probe")
}

#[test]
fn object_exhaustion_is_not_catchable_and_shutdown_keeps_existing_objects() {
    let result = run(r#"
        $items = [];
        register_shutdown_function(function() use (&$items) {
            $last = error_get_last();
            echo $last['type'] === E_ERROR ? 'fatal;' : 'wrong;';
            echo str_starts_with($last['message'], 'Allowed memory size of 2097152 bytes exhausted') ? 'budget;' : 'wrong;';
            echo count($items) > 0 && end($items) instanceof stdClass ? 'intact' : 'broken';
        });
        try { for ($i = 0; $i < 100000; ++$i) { $items[] = new stdClass; } }
        catch (Throwable $e) { echo 'caught'; }
        echo 'escaped';
    "#);
    assert_eq!(result.status.code(), Some(255));
    assert_eq!(result.stdout, b"fatal;budget;intact");
    assert_eq!(result.stderr, b"");
}

#[test]
fn packed_growth_failure_preserves_reference_alias_and_append_cursor() {
    let result = run(r#"
        $items = [];
        $alias =& $items;
        register_shutdown_function(function() use (&$items, &$alias) {
            $count = count($items);
            echo $count > 0 && $count === count($alias) && end($alias) === $count - 1 ? 'intact;' : 'broken;';
            ini_set('memory_limit', '-1');
            $items[] = 991;
            echo array_key_last($alias) === $count && $alias[$count] === 991 ? 'cursor' : 'broken';
        });
        for ($i = 0; $i < 1000000; ++$i) { $items[] = $i; }
        echo 'escaped';
    "#);
    assert_eq!(result.status.code(), Some(255));
    assert_eq!(result.stdout, b"intact;cursor");
    assert_eq!(result.stderr, b"");
}

#[test]
fn reclaimed_objects_do_not_consume_a_cumulative_allocation_quota() {
    let result = run(r#"
        for ($i = 0; $i < 30000; ++$i) { $value = new stdClass; }
        unset($value);
        echo 'released';
    "#);
    assert!(result.status.success(), "{:?}", result);
    assert_eq!(result.stdout, b"released");
    assert_eq!(result.stderr, b"");
}

#[test]
fn unlimited_mode_and_shared_array_aliases_remain_valid() {
    let result = run(r#"
        ini_set('memory_limit', '-1');
        $values = [];
        for ($i = 0; $i < 180000; ++$i) { $values[] = $i; }
        $copy = $values;
        echo count($values) === count($copy) ? 'shared' : 'broken';
    "#);
    assert!(result.status.success(), "{:?}", result);
    assert_eq!(result.stdout, b"shared");
    assert_eq!(result.stderr, b"");
}

#[test]
fn string_builder_checks_the_budget_before_allocating_and_publishing() {
    let result = run(r#"
        $text = 'original';
        register_shutdown_function(function() use (&$text) {
            echo $text === 'original' ? 'intact' : 'changed';
        });
        $text = str_repeat('q', 8 * 1024 * 1024);
        echo 'escaped';
    "#);
    assert_eq!(result.status.code(), Some(255));
    assert_eq!(result.stdout, b"intact");
    assert_eq!(result.stderr, b"");
}

#[test]
fn compound_string_failure_preserves_cow_alias_and_original_prefix() {
    let result = run(r#"
        $text = str_repeat('s', 400000);
        $alias = $text;
        register_shutdown_function(function() use (&$text, &$alias) {
            echo strlen($alias) === 400000 && $alias[0] === 's' ? 'alias;' : 'broken;';
            echo strlen($text) >= 400000 && $text[0] === 's' ? 'prefix' : 'broken';
        });
        for ($i = 0; $i < 30; ++$i) { $text .= str_repeat('t', 200000); }
        echo 'escaped';
    "#);
    assert_eq!(result.status.code(), Some(255));
    assert_eq!(result.stdout, b"alias;prefix");
    assert_eq!(result.stderr, b"");
}

#[test]
fn released_strings_and_peak_reset_do_not_retain_their_payload_charge() {
    let result = run(r#"
        for ($i = 0; $i < 30; ++$i) { $text = str_repeat('x', 350000); }
        $before = memory_get_usage();
        unset($text);
        $after = memory_get_usage();
        echo $after < $before ? 'released;' : 'retained;';
        memory_reset_peak_usage();
        echo memory_get_peak_usage() < $before ? 'reset' : 'stale';
    "#);
    assert!(result.status.success(), "{:?}", result);
    assert_eq!(result.stdout, b"released;reset");
    assert_eq!(result.stderr, b"");
}

#[test]
fn repeated_property_replacement_and_unset_reuse_the_storage_budget() {
    let result = run(r#"
        $object = new stdClass;
        for ($i = 0; $i < 40000; ++$i) {
            $object->payload = $i;
            unset($object->payload);
        }
        echo count((array)$object) === 0 ? 'released' : 'retained';
    "#);
    assert!(result.status.success(), "{:?}", result);
    assert_eq!(result.stdout, b"released");
    assert_eq!(result.stderr, b"");
}

#[test]
fn fatal_output_finalization_discards_data_but_preserves_callback_order() {
    let result = run(r#"
        ob_start(function($bytes, $phase) {
            global $values;
            $length = count($values);
            for ($n = 0; $n < $length; ++$n) { $values[] = 17; }
            fwrite(STDOUT, $phase === 11 && count($values) === $length * 2 ? 'clean-final' : 'broken');
            return 'must be discarded';
        });
        echo 'buffered data';
        $values = [];
        for ($i = 0; $i < 1000000; ++$i) { $values[] = 3; }
    "#);
    assert_eq!(result.status.code(), Some(255));
    assert_eq!(result.stdout, b"clean-final");
    assert_eq!(result.stderr, b"");
}

#[test]
fn expanding_string_builder_rejects_its_upper_bound_before_publication() {
    let result = run(r#"
        $result = 'unchanged';
        register_shutdown_function(function() use (&$result) { echo $result; });
        $result = wordwrap(str_repeat('a', 8192), 1, str_repeat('-', 16384));
        echo 'escaped';
    "#);
    assert_eq!(result.status.code(), Some(255));
    assert_eq!(result.stdout, b"unchanged");
    assert_eq!(result.stderr, b"");
}

#[test]
fn captured_closures_are_budgeted_without_breaking_existing_captures() {
    let result = run(r#"
        $callbacks = [];
        register_shutdown_function(function() use (&$callbacks) {
            echo count($callbacks) > 0 && $callbacks[0]() === 37 ? 'retained' : 'broken';
        });
        for ($i = 0; $i < 50000; ++$i) {
            $number = $i + 37;
            $callbacks[] = static function() use ($number) { return $number; };
        }
        echo 'escaped';
    "#);
    assert_eq!(result.status.code(), Some(255));
    assert_eq!(result.stdout, b"retained");
    assert_eq!(result.stderr, b"");
}

#[test]
fn output_buffers_obey_the_budget_and_discard_partial_bytes_on_fatal() {
    let result = run(r#"
        register_shutdown_function(function() { echo 'shutdown'; });
        ob_start();
        for ($i = 0; $i < 10000; ++$i) { echo str_repeat('q', 1024); }
        echo 'escaped';
    "#);
    assert_eq!(result.status.code(), Some(255));
    assert_eq!(result.stdout, b"shutdown");
    assert_eq!(result.stderr, b"");
}

#[test]
fn reclaimed_closures_and_cleaned_buffers_release_their_storage_charge() {
    let result = run(r#"
        for ($i = 0; $i < 30000; ++$i) { $callback = static fn() => 39; }
        unset($callback);
        ob_start();
        for ($i = 0; $i < 30; ++$i) {
            echo str_repeat('z', 200000);
            ob_clean();
        }
        ob_end_clean();
        echo 'released';
    "#);
    assert!(result.status.success(), "{:?}", result);
    assert_eq!(result.stdout, b"released");
    assert_eq!(result.stderr, b"");
}
