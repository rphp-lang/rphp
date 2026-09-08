<?php
chdir(getenv('RPHP_LINK_SPEC_DIR'));
file_put_contents('source', 'kept');
class LinkPath {
    public function __construct(public $label, public $text) {}
    public function __toString(): string { echo $this->label, "\n"; return $this->text; }
}
foreach (['link', 'symlink'] as $name) {
    var_dump($name(new LinkPath('target', 'source'), new LinkPath('destination', $name)));
    try { $name(new LinkPath('invalid-target', "bad\0"), new LinkPath('unreached', 'new')); }
    catch (Throwable $e) { echo $e->getMessage(), "\n"; }
    try { $name(new LinkPath('valid-target', 'source'), new LinkPath('invalid-destination', "bad\0")); }
    catch (Throwable $e) { echo $e->getMessage(), "\n"; }
}
echo readlink(new LinkPath('read', 'symlink')), "\n";
var_dump(file_exists('new'));
