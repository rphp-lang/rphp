<?php
chdir(getenv('RPHP_DIRECTORY_SPEC_DIR'));
class NativeDirectoryNameForSpec {
    public function __toString(): string { echo "stringify\n"; return '.'; }
}
try { dir(new NativeDirectoryNameForSpec, false); } catch (Throwable $e) { echo $e->getMessage(), "\n"; }
set_error_handler(function($code, $message) { echo "diagnostic:", $message, "\n"; throw new Exception('stop'); });
try { dir(null, 99); } catch (Throwable $e) { echo $e->getMessage(), "\n"; }
restore_error_handler();
$d = dir(new NativeDirectoryNameForSpec, null);
echo $d->path, "\n";
$d->close();
