<?php
set_error_handler(function ($level, $message) { echo 'warning:', $message, "\n"; });
class UriRefusal extends php_user_filter {
    public function onCreate(): bool { echo "refused\n"; return false; }
    public function onClose(): void { echo "refusal-close\n"; }
}
class UriExplosion extends php_user_filter {
    public function onCreate(): bool { throw new Exception('creation-stop'); }
    public function onClose(): void { echo "orphan-close\n"; }
}
stream_filter_register('uri.refusal', UriRefusal::class);
stream_filter_register('uri.explosion', UriExplosion::class);
foreach (['absent', 'uri.refusal', 'uri.explosion'] as $name) {
    try {
        echo 'read:', file_get_contents('php://filter/read=' . $name . '/resource=data://text/plain,body'), "\n";
    } catch (Throwable $e) {
        echo get_class($e), ':', $e->getMessage(), "\n";
    }
}
foreach (['php://filter/', 'php://filter/resource=', 'php://filter/read=string.toupper/RESOURCE=php://memory'] as $uri) {
    try { fopen($uri, 'r'); }
    catch (Throwable $e) { echo get_class($e), ':', $e->getMessage(), "\n"; }
}
echo 'ordinary:', file_get_contents('data://text/plain,unchanged'), "\n";
echo "done\n";
