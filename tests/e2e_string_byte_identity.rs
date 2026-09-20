mod common;

use common::run_php;

#[test]
fn byte_projections_casts_hashes_and_stream_writes_keep_utf8_identity() {
    assert_eq!(
        run_php(
            r##"<?php
$s = "<info>🪪</info> x";
$t = substr($s, 0, 10);
echo strlen($t), ',', (int) ($t === "<info>🪪"), ',', (int) str_contains($t, "🪪"), ',', iconv_strlen($t), ',', iconv_strlen((string) $t), ',', bin2hex((string) $t), ',', (int) (md5($t) === md5("<info>🪪")), '|';
echo md5("🪪é"), ',', bin2hex(strrev("🪪é")), ',', bin2hex(strrev(strrev("🪪é"))), ',', json_encode(unpack('C*', "🪪")), ',', json_encode(unpack('C*', substr("é", 0, 2))), '|';
$h = fopen('php://memory', 'w+');
fprintf($h, "%s|%5s|%'*8s|", "🪪é", "é", "🪪");
vfprintf($h, "%s-%d", ["é", 7]);
fputs($h, "🪪");
rewind($h);
echo bin2hex(stream_get_contents($h)), '|';
file_put_contents('php://output', "🪪é|");
$o = fopen('php://output', 'w'); fwrite($o, "é🪪"); fclose($o);
echo '|', bin2hex(substr("🪪", 1, 2)), ',', strlen(substr("🪪", 1, 2) . "é"), "\n";
"##,
        ),
        r##"10,1,1,7,7,3c696e666f3ef09faaaa,1|e2eba763b57225866996330615c4e56f,a9c3aaaa9ff0,f09faaaac3a9,{"1":240,"2":159,"3":170,"4":170},{"1":195,"2":169}|f09faaaac3a97c202020c3a97c2a2a2a2af09faaaa7cc3a92d37f09faaaa|🪪é|é🪪|9faa,4
"##
    );
}

#[test]
fn non_unicode_patterns_match_bytes_while_unicode_patterns_match_characters() {
    assert_eq!(
        run_php(
            r##"<?php
$s = "a🪪b";
echo preg_match('/[\x80-\xFF]/', $s), preg_match('/^.{6}$/', $s), preg_match('/^.{3}$/u', $s), preg_match_all('/./s', $s), preg_match_all('/./su', $s), '|';
echo preg_replace('/[\x80-\xFF]/', '?', $s), ',', preg_replace('/[\xF0-\xF7][\x80-\xBF]{3}/', '<4>', $s), ',', preg_replace('/🪪/', 'é', $s), ',', preg_replace('/(é)/', '[$1]', 'café'), '|';
preg_match('/b/', "🪪 b", $m, PREG_OFFSET_CAPTURE); echo json_encode($m), ',';
preg_match('/(?<x>[\x80-\xFF]+)/', $s, $m, PREG_OFFSET_CAPTURE); echo strlen($m['x'][0]), ':', $m['x'][1], ',';
preg_match_all('/[\x80-\xBF]/', "é🪪", $all, PREG_OFFSET_CAPTURE); echo json_encode(array_column($all[0], 1)), '|';
echo json_encode(preg_split('/\R/', "a\r\n🪪\nb\rc")), ',', json_encode(preg_split('/x/', "é x b", -1, PREG_SPLIT_OFFSET_CAPTURE)), '|';
echo preg_replace_callback('/[\x80-\xFF]{4}/', fn ($m) => strtoupper(bin2hex($m[0])) . 'é', $s), ',', preg_replace_callback('/./u', fn ($m) => strlen($m[0]), "aé🪪"), '|';
echo json_encode(preg_grep('/[\x80-\xFF]/', ['a', 'é', 'b🪪'])), ',', json_encode(preg_replace(['/a/', '/[\xC3]/'], ['é', '_'], "café a")), ',', preg_match('/^\X$/u', "e\u{301}"), preg_match('/^\p{L}+$/u', "café"), preg_match('/^a/A', "ba"), "\n";
"##,
        ),
        r##"11163|a????b,a<4>b,aéb,caf[é]|[["b",5]],4:1,[1,3,4,5]|["a","\ud83e\udeaa","b","c"],[["\u00e9 ",0],[" b",4]]|aF09FAAAAéb,124|{"1":"\u00e9","2":"b\ud83e\udeaa"},,110
"##
    );
}

#[test]
fn runtime_information_functions_report_php_shapes() {
    assert_eq!(
        run_php(
            r##"<?php
echo json_encode([fnmatch('*.php', 'a.php'), fnmatch('a?c', 'abc'), fnmatch('[a-c]*', 'dog'), fnmatch('*.PHP', 'x.php', FNM_CASEFOLD), fnmatch('*a', '.a', FNM_PERIOD), fnmatch('a*', 'a/b', FNM_PATHNAME), fnmatch('a\*', 'a*', FNM_NOESCAPE)]), '|';
echo gettype(memory_get_usage()), ',', gettype(memory_get_peak_usage(true)), ',', (int) (getmypid() > 0), ',', gettype(gethostname()), ',', strlen(uniqid()), ',', strlen(uniqid('', true)), ',', mt_getrandmax(), ',', (int) is_countable([1]), (int) is_countable(new ArrayObject()), (int) is_countable('x'), '|';
echo json_encode([php_ini_loaded_file(), php_ini_scanned_files(), get_cfg_var('nope')]), ',', count(sys_getloadavg()), ',', (int) in_array('sha256', hash_algos(), true), ',', hash('sha256', 'abc'), ',', hash('crc32b', 'abc'), ',', hash('md5', ''), "\n";
"##,
        ),
        r##"[true,true,false,true,false,false,false]|integer,integer,1,string,13,23,2147483647,110|[false,false,false],3,1,ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad,352441c2,d41d8cd98f00b204e9800998ecf8427e
"##
    );
}
