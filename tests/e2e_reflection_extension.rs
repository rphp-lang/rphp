mod common;

use common::run_php;

#[test]
fn reflection_extension_projects_only_the_registered_runtime_surface() {
    assert_eq!(
        run_php(
            r#"<?php
$extension = new ReflectionExtension('OpEnSsL');
echo $extension->getName(), ':', $extension->getVersion(), ':',
    (int) $extension->isPersistent(), ':', (int) $extension->isTemporary(), "\n";
echo phpversion('openssl'), ':', var_export(phpversion('missing'), true), "\n";
$functions = array_keys($extension->getFunctions()); sort($functions);
echo implode(',', $functions), "\n";
$constants = array_keys($extension->getConstants()); sort($constants);
echo implode(',', $constants), "\n";
$classes = $extension->getClasses();
echo implode(',', array_keys($classes)), ':', $classes['OpenSSLAsymmetricKey']->getName(), "\n";
var_dump($extension->getDependencies());
$pcre = new ReflectionExtension('pcre');
echo implode(',', array_keys($pcre->getINIEntries())), "\n";
ob_start(); $pcre->info(); $info = ob_get_clean();
echo (int) str_contains($info, 'PCRE Library Version => 10.42 2022-12-11'), "\n";
try { new ReflectionExtension('missing'); }
catch (ReflectionException $error) { echo $error->getMessage(), "\n"; }
try { clone $extension; }
catch (Error $error) { echo $error->getMessage(), "\n"; }
"#,
        ),
        concat!(
            "openssl:8.5.0:1:0\n",
            "8.5.0:false\n",
            "openssl_free_key,openssl_get_cert_locations,openssl_get_md_methods,openssl_get_publickey,openssl_pkey_get_details,openssl_pkey_get_public,openssl_verify,openssl_x509_parse\n",
            "OPENSSL_ALGO_MD5,OPENSSL_ALGO_SHA1,OPENSSL_ALGO_SHA224,OPENSSL_ALGO_SHA256,OPENSSL_ALGO_SHA384,OPENSSL_ALGO_SHA512,OPENSSL_KEYTYPE_DH,OPENSSL_KEYTYPE_DSA,OPENSSL_KEYTYPE_EC,OPENSSL_KEYTYPE_RSA,OPENSSL_PKCS1_PADDING,OPENSSL_PKCS1_PSS_PADDING,OPENSSL_VERSION_NUMBER,OPENSSL_VERSION_TEXT\n",
            "OpenSSLAsymmetricKey:OpenSSLAsymmetricKey\n",
            "array(0) {\n}\n",
            "pcre.backtrack_limit,pcre.recursion_limit\n",
            "1\n",
            "Extension \"missing\" does not exist\n",
            "Trying to clone an uncloneable object of class ReflectionExtension\n",
        )
    );
}

#[test]
fn reflection_extension_exposes_php_85_method_and_property_contracts() {
    assert_eq!(
        run_php(
            r#"<?php
$class = new ReflectionClass(ReflectionExtension::class);
echo implode(',', $class->getInterfaceNames()), "\n";
$property = $class->getProperty('name');
echo $property->getType(), ':', (int) $property->isPublic(), ':',
    (int) $property->isReadOnly(), ':', (int) $property->hasDefaultValue(), "\n";
foreach (['__clone','__construct','__toString','getName','getVersion','getFunctions','getConstants','getINIEntries','getClasses','getClassNames','getDependencies','info','isPersistent','isTemporary'] as $name) {
    $method = $class->getMethod($name);
    echo $name, ':', $method->getNumberOfRequiredParameters(), '/',
        $method->getNumberOfParameters(), ':',
        $method->hasReturnType() ? $method->getReturnType() : '-', "\n";
}
"#,
        ),
        concat!(
            "Stringable,Reflector\n",
            "string:1:0:0\n",
            "__clone:0/0:void\n",
            "__construct:1/1:-\n",
            "__toString:0/0:string\n",
            "getName:0/0:-\n",
            "getVersion:0/0:-\n",
            "getFunctions:0/0:-\n",
            "getConstants:0/0:-\n",
            "getINIEntries:0/0:-\n",
            "getClasses:0/0:-\n",
            "getClassNames:0/0:-\n",
            "getDependencies:0/0:-\n",
            "info:0/0:-\n",
            "isPersistent:0/0:-\n",
            "isTemporary:0/0:-\n",
        )
    );
}
