mod common;

use common::run_php;
use std::fs::{self, File};
use std::io::Write;
use std::sync::atomic::{AtomicU64, Ordering};
use zip::write::SimpleFileOptions;

static NEXT_TEMP: AtomicU64 = AtomicU64::new(1);

struct TempTree(std::path::PathBuf);

impl TempTree {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!(
            "rphp-zip-composer-{}-{}",
            std::process::id(),
            NEXT_TEMP.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir_all(&path).unwrap();
        Self(path)
    }
}

impl Drop for TempTree {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

#[test]
fn zip_archive_extracts_composer_dist_archives_in_process() {
    let tree = TempTree::new();
    let archive_path = tree.0.join("package.zip");
    let file = File::create(&archive_path).unwrap();
    let mut archive = zip::ZipWriter::new(file);
    let options = SimpleFileOptions::default()
        .compression_method(zip::CompressionMethod::DEFLATE)
        .unix_permissions(0o644);
    archive
        .start_file("vendor-package/composer.json", options)
        .unwrap();
    archive.write_all(br#"{"name":"rphp/package"}"#).unwrap();
    archive
        .start_file("vendor-package/src/Contract.php", options)
        .unwrap();
    archive.write_all(b"<?php return 'installed';\n").unwrap();
    archive.finish().unwrap();

    let destination = tree.0.join("extract");
    let source = format!(
        r#"<?php
echo (int) extension_loaded('zip'), ':',
    (new ReflectionClass(ZipArchive::class))->getExtensionName(), ':',
    (int) is_subclass_of(ZipArchive::class, Countable::class), "\n";
$zip = new ZipArchive();
var_dump($zip->open('{}'));
echo $zip->numFiles, ':', $zip->count(), ':', $zip->status, ':',
    (int) str_ends_with($zip->filename, 'package.zip'), "\n";
$stat = $zip->statIndex(0);
echo $stat['name'], ':', $stat['size'], ':', (int) ($stat['comp_size'] > 0), ':',
    $zip->locateName('vendor-package/src/Contract.php'), ':',
    $zip->getNameIndex(1), "\n";
echo $zip->getFromIndex(0), "\n";
var_dump($zip->extractTo('{}'));
echo file_get_contents('{}/vendor-package/src/Contract.php');
var_dump($zip->close());
"#,
        archive_path.display(),
        destination.display(),
        destination.display(),
    );
    assert_eq!(
        run_php(&source),
        concat!(
            "1:zip:1\n",
            "bool(true)\n",
            "2:2:0:1\n",
            "vendor-package/composer.json:23:1:1:vendor-package/src/Contract.php\n",
            "{\"name\":\"rphp/package\"}\n",
            "bool(true)\n",
            "<?php return 'installed';\n",
            "bool(true)\n",
        )
    );
}

#[test]
fn zip_archive_invalid_file_returns_php_error_code() {
    let tree = TempTree::new();
    let invalid = tree.0.join("not-a-zip.bin");
    fs::write(&invalid, b"not a zip").unwrap();
    assert_eq!(
        run_php(&format!(
            "<?php $zip = new ZipArchive(); var_dump($zip->open('{}') === ZipArchive::ER_NOZIP, $zip->status === ZipArchive::ER_NOZIP);",
            invalid.display()
        )),
        "bool(true)\nbool(true)\n"
    );
}

#[test]
fn zip_archive_composer_methods_match_php_85_contracts() {
    assert_eq!(
        run_php(
            r#"<?php
foreach (['open', 'statIndex', 'locateName', 'getNameIndex', 'getFromIndex', 'extractTo'] as $name) {
    $method = new ReflectionMethod(ZipArchive::class, $name);
    echo $name, ':', $method->getNumberOfRequiredParameters(), '/', $method->getNumberOfParameters(), ':', $method->getReturnType(), ':';
    foreach ($method->getParameters() as $parameter) {
        echo $parameter->getName(), '=', $parameter->isDefaultValueAvailable() ? var_export($parameter->getDefaultValue(), true) : '-', ',';
    }
    echo "\n";
}
"#,
        ),
        concat!(
            "open:1/2:int|bool:filename=-,flags=0,\n",
            "statIndex:1/2:array|false:index=-,flags=0,\n",
            "locateName:1/2:int|false:name=-,flags=0,\n",
            "getNameIndex:1/2:string|false:index=-,flags=0,\n",
            "getFromIndex:1/3:string|false:index=-,len=0,flags=0,\n",
            "extractTo:1/2:bool:pathto=-,files=NULL,\n",
        )
    );
}

#[test]
fn composer_process_cwd_guard_observes_the_zend_thread_safe_constant() {
    assert_eq!(
        run_php(
            "<?php $cwd = null; if ($cwd === null && defined('ZEND_THREAD_SAFE')) { $cwd = getcwd(); } var_dump(ZEND_THREAD_SAFE, is_string($cwd), is_dir($cwd));"
        ),
        "bool(false)\nbool(true)\nbool(true)\n"
    );
}
