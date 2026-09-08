<?php
declare(strict_types=1);
chdir(getenv('RPHP_LINK_SPEC_DIR'));
foreach (['link', 'symlink'] as $name) {
    foreach ([[1, 'out'], ['source', 2]] as $args) {
        try { $name(...$args); }
        catch (Throwable $e) { echo get_class($e), ':', $e->getMessage(), "\n"; }
    }
}
try { readlink(1); } catch (Throwable $e) { echo get_class($e), ':', $e->getMessage(), "\n"; }
