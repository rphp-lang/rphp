<?php
// Diagnostic driver only: load caller-supplied dependencies and inputs.
if ($argc < 3) { throw new RuntimeException('usage: parser.php ARCHIVE SOURCE...'); }
require 'phar://' . $argv[1] . '/vendor/autoload.php';
$parser = (new PhpParser\ParserFactory())->createForNewestSupportedVersion();
$sources = [];
foreach (array_slice($argv, 2) as $path) {
    $source = file_get_contents($path);
    if ($source === false) { throw new RuntimeException('source read failed'); }
    $sources[] = $source;
}
$controlPath = getenv('RPHP_PERF_CONTROL');
$ackPath = getenv('RPHP_PERF_ACK');
$control = false;
$ack = false;
if ($controlPath !== false || $ackPath !== false) {
    $control = fopen($controlPath, 'w');
    $ack = fopen($ackPath, 'r');
    if ($control === false || $ack === false) { throw new RuntimeException('perf control open failed'); }
    fwrite($control, "enable\n");
    fflush($control);
    if (trim(fgets($ack), "\0\r\n") !== 'ack') { throw new RuntimeException('perf enable acknowledgement missing'); }
}
$start = microtime(true);
$trees = [];
foreach ($sources as $source) { $trees[] = $parser->parse($source); }
$seconds = microtime(true) - $start;
if ($control !== false) {
    fwrite($control, "disable\n");
    fflush($control);
    if (trim(fgets($ack), "\0\r\n") !== 'ack') { throw new RuntimeException('perf disable acknowledgement missing'); }
    fclose($control);
    fclose($ack);
}
fprintf(STDERR, "RPHP_BENCH_ANALYSIS=%.9f\n", $seconds);
// ASTs stay alive across measurement; digest and teardown are outside it.
echo count($trees), ':', hash('sha256', serialize($trees)), "\n";
