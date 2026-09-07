mod common;

#[cfg(feature = "stream-registry")]
use common::run_php;

// These are independently written protocol specimens, captured byte-for-byte
// against the reference CLI. They deliberately cover observable ordering, not
// just transformed data or successful function registration.
macro_rules! filter_contract {
    ($name:ident, $fixture:literal) => {
        #[test]
        #[cfg(feature = "stream-registry")]
        fn $name() {
            assert_eq!(
                run_php(include_str!(concat!(
                    "fixtures/user_stream_filters/",
                    $fixture,
                    ".php"
                ))),
                include_str!(concat!("fixtures/user_stream_filters/", $fixture, ".out"))
            );
        }
    };
}

filter_contract!(
    snapshot_arguments_do_not_recover_mutated_caller_aliases,
    "argument_snapshot"
);

#[cfg(all(
    feature = "file-contents",
    feature = "stream-line",
    feature = "stream-contents"
))]
filter_contract!(native_prebuffer_attachment_is_transactional, "prebuffer");

#[cfg(all(feature = "stream-contents", feature = "stream-registry"))]
filter_contract!(
    open_argument_strings_survive_reentrant_source_writes,
    "open_argument_snapshot"
);

#[cfg(all(feature = "file-contents", feature = "stream-truncate"))]
filter_contract!(
    only_physical_io_invalidates_the_stat_cache,
    "prebuffer_stat_cache"
);

#[cfg(all(feature = "stream-contents", feature = "stream-registry"))]
filter_contract!(
    prebuffer_chain_exceptions_and_native_writes,
    "prebuffer_boundaries"
);

#[cfg(all(feature = "file-contents", feature = "stream-contents"))]
filter_contract!(
    uri_consumers_share_filter_creation_io_and_close,
    "uri_consumers"
);
#[cfg(feature = "file-contents")]
filter_contract!(
    uri_factory_failures_preserve_warning_and_exception_order,
    "uri_failures"
);
#[cfg(all(feature = "file-contents", feature = "include-path", unix))]
filter_contract!(
    read_errors_keep_include_require_and_callback_priority,
    "read_failures"
);

#[test]
#[cfg(feature = "stream-registry")]
fn filter_wrapper_does_not_bypass_default_url_include_policy() {
    assert_eq!(
        run_php(
            r#"<?php
$warnings = 0;
set_error_handler(function ($level, $message) use (&$warnings) { ++$warnings; });
class NeverOpenedFilter extends php_user_filter {
    function onCreate(): bool { echo "unexpected factory\n"; return true; }
}
stream_filter_register('guarded.input', NeverOpenedFilter::class);
$ran = false;
$result = include 'php://filter/read=guarded.input/resource=data://text/plain,<?php $ran=true;';
echo $warnings, ':', (int)$result, ':', (int)$ran, "\n";
"#
        ),
        "2:0:0\n"
    );
}

filter_contract!(
    resource_callbacks_follow_alias_container_and_exception_boundaries,
    "resource_release"
);
filter_contract!(
    callback_resources_in_shallow_objects_keep_last_alias_order,
    "shallow_resource"
);

filter_contract!(
    creation_flush_seek_and_remove_keep_callback_order,
    "lifecycle"
);
filter_contract!(
    prepend_orders_binary_filters_without_changing_consumed_count,
    "binary_chain"
);
filter_contract!(
    cloned_bucket_handles_move_instead_of_duplicating_data,
    "bucket_moves"
);
filter_contract!(read_eof_flushes_tail_once_before_close, "read_eof");
filter_contract!(
    creation_failures_keep_stream_and_diagnostic_order,
    "creation_failure"
);
filter_contract!(
    stream_projection_obeys_declared_type_and_private_storage,
    "property_projection"
);
filter_contract!(
    internal_projection_keeps_hooks_readonly_and_reference_constraints,
    "property_write_contract"
);
filter_contract!(
    legacy_memory_modes_deliver_read_callbacks_without_file_mode_rules,
    "memory_modes"
);
filter_contract!(
    close_exception_retires_stream_and_every_filter_without_later_callbacks,
    "close_errors"
);
#[cfg(all(
    feature = "stream-line",
    feature = "stream-copy",
    feature = "stream-contents",
    feature = "formatted-io"
))]
filter_contract!(
    filtered_lines_csv_copy_and_formatted_io_share_cursor_and_callbacks,
    "read_consumers"
);

#[test]
#[cfg(feature = "stream-registry")]
fn filter_diagnostics_keep_nested_source_and_restore_after_handler_exceptions() {
    assert_eq!(
        common::run_php_with_source_context(
            include_str!("fixtures/user_stream_filters/diagnostic_origin.php"),
            "filter-origins.php",
            "",
        ),
        include_str!("fixtures/user_stream_filters/diagnostic_origin.out"),
    );
}

#[test]
#[cfg(all(
    feature = "stream-registry",
    feature = "csv-write",
    feature = "stream-contents"
))]
fn filtered_csv_writer_preserves_encoder_bytes_and_write_count() {
    assert_eq!(
        run_php(
            r#"<?php
class CsvOutputFilter extends php_user_filter {
    function filter($in, $out, &$used, $closing): int {
        while ($b = stream_bucket_make_writeable($in)) {
            $used += $b->datalen;
            $b->data = strtoupper($b->data);
            stream_bucket_append($out, $b);
        }
        return PSFS_PASS_ON;
    }
}
stream_filter_register('csv.output', CsvOutputFilter::class);
$s = fopen('php://memory', 'w+');
stream_filter_append($s, 'csv.output', STREAM_FILTER_WRITE);
var_dump(fputcsv($s, ['a,b', 'x"y', "z\nq"], ',', '"', ''));
rewind($s);
echo bin2hex(stream_get_contents($s)), "\n";
fclose($s);
"#
        ),
        "int(19)\n22412c42222c2258222259222c225a0a51220a\n"
    );
}
filter_contract!(
    feed_and_fatal_status_keep_distinct_output_and_removal_rules,
    "feed_and_error"
);
filter_contract!(
    unprocessed_brigades_warn_without_replacing_pending_exceptions,
    "brigade_diagnostics"
);
filter_contract!(
    builtin_byte_uppercase_and_user_filters_share_order_aliases_and_identity,
    "builtin_composition"
);
filter_contract!(
    bucket_data_reference_preserves_bytes_and_original_length,
    "bucket_reference"
);
filter_contract!(
    closing_inside_filter_is_rejected_without_invalidating_stream,
    "close_reentry"
);
filter_contract!(
    last_alias_and_container_release_close_before_next_statement,
    "last_alias"
);
filter_contract!(
    sparse_temporary_ranges_keep_native_aliases_and_php_close_order,
    "temporary_ranges"
);
filter_contract!(
    assignment_into_scalar_and_reference_keeps_php_resource_ownership,
    "primitive_assignment"
);
filter_contract!(
    argument_fetch_keeps_warning_snapshot_reference_and_closed_alias_contracts,
    "argument_fetch"
);

#[test]
#[cfg(feature = "stream-registry")]
fn stream_registry_identity_does_not_publish_or_reuse_php_constants() {
    assert_eq!(
        run_php(
            r#"<?php
$stream = fopen('php://memory', 'w+');
$key = "\0rphp-resource-scope";
var_dump(defined($key));
var_dump(define($key, 17));
var_dump(constant($key));
var_dump(fwrite($stream, 'ok'));
rewind($stream);
var_dump(fread($stream, 2));
fclose($stream);
"#
        ),
        "bool(false)\nbool(true)\nint(17)\nint(2)\nstring(2) \"ok\"\n"
    );
}

#[test]
#[cfg(feature = "stream-registry")]
fn registration_keeps_exact_case_and_defers_class_resolution() {
    assert_eq!(
        run_php(
            r#"<?php
foreach ([['', 'Absent'], ['x', ''], ['Tag.One', 'Absent'], ['Tag.One', 'Absent'], ['tag.one', 'Absent'], ['Tag.*', 'Absent']] as $pair) {
    try { var_dump(stream_filter_register(...$pair)); }
    catch (Throwable $e) { echo $e::class, ':', $e->getMessage(), "\n"; }
}
foreach (stream_get_filters() as $name) if (str_starts_with(strtolower($name), 'tag.')) echo $name, "\n";
"#
        ),
        concat!(
            "ValueError:stream_filter_register(): Argument #1 ($filter_name) must be a non-empty string\n",
            "ValueError:stream_filter_register(): Argument #2 ($class) must be a non-empty string\n",
            "bool(true)\nbool(false)\nbool(true)\nbool(true)\nTag.One\ntag.one\nTag.*\n"
        )
    );
}

#[test]
#[cfg(feature = "stream-registry")]
fn filter_status_and_operation_flags_are_independent() {
    assert_eq!(
        run_php(
            r#"<?php
foreach (['STREAM_FILTER_READ','STREAM_FILTER_WRITE','STREAM_FILTER_ALL','PSFS_ERR_FATAL','PSFS_FEED_ME','PSFS_PASS_ON','PSFS_FLAG_NORMAL','PSFS_FLAG_FLUSH_INC','PSFS_FLAG_FLUSH_CLOSE'] as $name) echo $name, '=', constant($name), "\n";
"#
        ),
        concat!(
            "STREAM_FILTER_READ=1\nSTREAM_FILTER_WRITE=2\nSTREAM_FILTER_ALL=3\n",
            "PSFS_ERR_FATAL=0\nPSFS_FEED_ME=1\nPSFS_PASS_ON=2\n",
            "PSFS_FLAG_NORMAL=0\nPSFS_FLAG_FLUSH_INC=1\nPSFS_FLAG_FLUSH_CLOSE=2\n"
        )
    );
}

#[test]
#[cfg(feature = "stream-registry")]
fn registry_and_bucket_functions_publish_real_call_shapes() {
    assert_eq!(
        run_php(
            r#"<?php
foreach (['stream_filter_register','stream_bucket_new','stream_bucket_make_writeable','stream_bucket_append','stream_bucket_prepend'] as $name) {
    $f = new ReflectionFunction($name); echo $f->getName(), ':', $f->getNumberOfRequiredParameters(), ':', $f->getReturnType(), ':';
    foreach ($f->getParameters() as $p) echo $p->getName(), '/', $p->getType(), '/', (int)$p->isPassedByReference(), ';';
    echo "\n";
}
"#
        ),
        concat!(
            "stream_filter_register:2:bool:filter_name/string/0;class/string/0;\n",
            "stream_bucket_new:2:StreamBucket:stream//0;buffer/string/0;\n",
            "stream_bucket_make_writeable:1:?StreamBucket:brigade//0;\n",
            "stream_bucket_append:2:void:brigade//0;bucket/StreamBucket/0;\n",
            "stream_bucket_prepend:2:void:brigade//0;bucket/StreamBucket/0;\n"
        )
    );
}

#[test]
#[cfg(feature = "stream-registry")]
fn filter_and_bucket_defaults_preserve_uninitialized_typed_slots() {
    assert_eq!(
        run_php(
            r#"<?php
foreach (['php_user_filter','StreamBucket'] as $name) { $r = new ReflectionClass($name); echo $name, ':', (int)$r->isFinal(), ':', json_encode($r->getDefaultProperties()), "\n"; }
"#
        ),
        "php_user_filter:0:{\"filtername\":\"\",\"params\":\"\",\"stream\":null}\nStreamBucket:1:{\"bucket\":null}\n"
    );
}
