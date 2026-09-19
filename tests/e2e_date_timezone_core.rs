mod common;

use common::run_php;

#[test]
fn iana_2026a_projects_offsets_abbreviations_and_dst_boundaries() {
    assert_eq!(
        run_php(
            r#"<?php
$cases = [
    ['America/New_York', 1704067200],
    ['America/New_York', 1710053999],
    ['America/New_York', 1710054000],
    ['America/New_York', 1730613599],
    ['America/New_York', 1730613600],
    ['Europe/Prague', -2486592000],
    ['Asia/Kathmandu', 504901799],
    ['Asia/Kathmandu', 504901800],
    ['Australia/Lord_Howe', 1728140400],
    ['Pacific/Chatham', 1727532900],
    ['Pacific/Apia', 1325239200],
    ['Africa/Casablanca', 1710036000],
    ['Europe/Dublin', 1704067200],
    ['Europe/Dublin', 1719792000],
    ['Africa/Accra', -1017633600],
    ['Africa/Windhoek', 1483228800],
    ['Africa/Windhoek', 1704067200],
    ['Africa/Casablanca', 0],
    ['Africa/Timbuktu', -2208988800],
    ['CET', -2208988800],
    ['HST', -2208988800],
    ['WET', 0],
];
foreach ($cases as [$zone, $timestamp]) {
    date_default_timezone_set($zone);
    echo $zone, '|', $timestamp, '|', date('Y-m-d H:i:s T P Z I', $timestamp), "\n";
}
"#,
        ),
        concat!(
            "America/New_York|1704067200|2023-12-31 19:00:00 EST -05:00 -18000 0\n",
            "America/New_York|1710053999|2024-03-10 01:59:59 EST -05:00 -18000 0\n",
            "America/New_York|1710054000|2024-03-10 03:00:00 EDT -04:00 -14400 1\n",
            "America/New_York|1730613599|2024-11-03 01:59:59 EDT -04:00 -14400 1\n",
            "America/New_York|1730613600|2024-11-03 01:00:00 EST -05:00 -18000 0\n",
            "Europe/Prague|-2486592000|1891-03-16 00:57:44 PMT +00:57 3464 0\n",
            "Asia/Kathmandu|504901799|1985-12-31 23:59:59 +0530 +05:30 19800 0\n",
            "Asia/Kathmandu|504901800|1986-01-01 00:15:00 +0545 +05:45 20700 0\n",
            "Australia/Lord_Howe|1728140400|2024-10-06 01:30:00 +1030 +10:30 37800 0\n",
            "Pacific/Chatham|1727532900|2024-09-29 04:00:00 +1345 +13:45 49500 1\n",
            "Pacific/Apia|1325239200|2011-12-31 00:00:00 +14 +14:00 50400 1\n",
            "Africa/Casablanca|1710036000|2024-03-10 02:00:00 +00 +00:00 0 0\n",
            "Europe/Dublin|1704067200|2024-01-01 00:00:00 GMT +00:00 0 0\n",
            "Europe/Dublin|1719792000|2024-07-01 01:00:00 IST +01:00 3600 1\n",
            "Africa/Accra|-1017633600|1937-10-02 20:20:00 +0020 +00:20 1200 1\n",
            "Africa/Windhoek|1483228800|2017-01-01 02:00:00 CAT +02:00 7200 1\n",
            "Africa/Windhoek|1704067200|2024-01-01 02:00:00 CAT +02:00 7200 0\n",
            "Africa/Casablanca|0|1970-01-01 00:00:00 +00 +00:00 0 0\n",
            "Africa/Timbuktu|-2208988800|1899-12-31 23:28:00 LMT -00:32 -1920 0\n",
            "CET|-2208988800|1900-01-01 01:00:00 CET +01:00 3600 0\n",
            "HST|-2208988800|1899-12-31 14:00:00 HST -10:00 -36000 0\n",
            "WET|0|1970-01-01 00:00:00 WET +00:00 0 0\n",
        )
    );
}

#[test]
fn mktime_uses_php_gap_and_overlap_selection() {
    assert_eq!(
        run_php(
            r#"<?php
date_default_timezone_set('America/New_York');
foreach ([[2024,3,10,2,30,0], [2024,11,3,1,30,0],
          [2024,3,10,3,30,0], [2024,11,3,2,30,0]] as $parts) {
    $timestamp = mktime($parts[3], $parts[4], $parts[5], $parts[1], $parts[2], $parts[0]);
    echo implode(',', $parts), '|', $timestamp, '|', date('c T I', $timestamp), "\n";
}
"#,
        ),
        concat!(
            "2024,3,10,2,30,0|1710055800|2024-03-10T03:30:00-04:00 EDT 1\n",
            "2024,11,3,1,30,0|1730611800|2024-11-03T01:30:00-04:00 EDT 1\n",
            "2024,3,10,3,30,0|1710055800|2024-03-10T03:30:00-04:00 EDT 1\n",
            "2024,11,3,2,30,0|1730619000|2024-11-03T02:30:00-05:00 EST 0\n",
        )
    );
}

#[test]
fn localtime_and_idate_observe_transition_state() {
    assert_eq!(
        run_php(
            r#"<?php
date_default_timezone_set('Pacific/Chatham');
foreach ([1727531999, 1727532000] as $timestamp) {
    $local = localtime($timestamp, true);
    echo $timestamp, '|', $local['tm_hour'], ':', $local['tm_min'], '|',
        $local['tm_isdst'], '|', idate('I', $timestamp), '|', idate('Z', $timestamp), "\n";
}
"#,
        ),
        concat!("1727531999|2:44|0|0|45900\n", "1727532000|3:45|1|1|49500\n",)
    );
}

#[test]
fn timezone_selection_accepts_iana_identifiers_and_preserves_failure_state() {
    assert_eq!(
        run_php(
            r#"<?php
date_default_timezone_set('UTC');
foreach (['Europe/Prague', 'America/Coyhaique', 'Pacific/Kanton', 'No/Such'] as $zone) {
    set_error_handler(static function (int $severity, string $message): bool {
        echo $severity, ':', $message, "\n";
        return true;
    });
    var_dump(date_default_timezone_set($zone));
    restore_error_handler();
    echo date_default_timezone_get(), "\n";
}
"#,
        ),
        concat!(
            "bool(true)\nEurope/Prague\n",
            "bool(true)\nAmerica/Coyhaique\n",
            "bool(true)\nPacific/Kanton\n",
            "8:date_default_timezone_set(): Timezone ID 'No/Such' is invalid\n",
            "bool(false)\nPacific/Kanton\n",
        )
    );
}
