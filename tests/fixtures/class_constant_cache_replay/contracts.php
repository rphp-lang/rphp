<?php
class ReplayOwner {
    public const VALUE = 21;
    public const ARRAY_VALUE = ['v' => [2, 3]];
    private const SECRET = 37;
    protected const PROTECTED_VALUE = 41;
    #[Deprecated('use VALUE')]
    public const OLD_VALUE = 43;
    public static function protectedValue() { return self::PROTECTED_VALUE; }
}
class ReplayChild extends ReplayOwner { public const VALUE = 23; }
class ReplayOther {}
function replayValues() {
    $copy = ReplayOwner::ARRAY_VALUE;
    $copy['v'][0]++;
    echo ReplayOwner::VALUE, ':', $copy['v'][0], ':', ReplayOwner::ARRAY_VALUE['v'][0], '|';
}
function lateAliasValue() { return ReplayAlias::VALUE; }
function lateMissingValue() { return ReplayMissing::VALUE; }
function lateScopeValue($class) { return $class::VALUE; }
function oldValue() { return ReplayOwner::OLD_VALUE; }
spl_autoload_register(function($class) {
    echo 'autoload:', $class, '|';
    if ($class === 'ReplayAlias') { class_alias(ReplayOwner::class, $class); }
});
for ($i = 0; $i < 3; $i++) {
    replayValues();
    echo lateAliasValue(), ':', ReplayChild::protectedValue(), ':', lateScopeValue(ReplayChild::class), '|';
}
for ($i = 0; $i < 2; $i++) {
    try { echo lateMissingValue(), '|'; }
    catch (Error $error) { echo $error->getMessage(), '|'; }
    if ($i === 0) { eval('class ReplayMissing { const VALUE = 47; }'); }
}
$probe = static function() { return ReplayOwner::SECRET; };
$allowed = Closure::bind($probe, null, ReplayOwner::class);
$denied = Closure::bind($probe, null, ReplayOther::class);
for ($i = 0; $i < 2; $i++) {
    echo $allowed(), '|';
    try { echo $denied(); } catch (Error $error) { echo $error->getMessage(), '|'; }
}
$warnings = 0;
set_error_handler(function($level, $message) use (&$warnings) {
    $warnings++;
    echo 'deprecated:', $level, ':', ReplayOwner::VALUE, '|';
    return true;
});
for ($i = 0; $i < 3; $i++) { echo oldValue(), '|'; }
restore_error_handler();
echo 'warnings:', $warnings, "\n";
