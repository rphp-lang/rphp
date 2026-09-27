mod common;

use common::run_php;
use openssl::asn1::{Asn1Integer, Asn1Time};
use openssl::bn::BigNum;
use openssl::hash::MessageDigest;
use openssl::pkey::PKey;
use openssl::rsa::Rsa;
use openssl::sign::Signer;
use openssl::x509::extension::SubjectAlternativeName;
use openssl::x509::{X509, X509NameBuilder};

fn hex(bytes: &[u8]) -> String {
    const DIGITS: &[u8; 16] = b"0123456789abcdef";
    let mut result = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        result.push(DIGITS[(byte >> 4) as usize] as char);
        result.push(DIGITS[(byte & 15) as usize] as char);
    }
    result
}

fn certificate_and_signature() -> (Vec<u8>, Vec<u8>) {
    let key = PKey::from_rsa(Rsa::generate(2048).unwrap()).unwrap();
    let mut name = X509NameBuilder::new().unwrap();
    name.append_entry_by_text("C", "CZ").unwrap();
    name.append_entry_by_text("O", "RPHP").unwrap();
    name.append_entry_by_text("CN", "packages.example.test")
        .unwrap();
    let name = name.build();
    let mut certificate = X509::builder().unwrap();
    certificate.set_version(2).unwrap();
    let serial = BigNum::from_u32(42).unwrap();
    let serial = Asn1Integer::from_bn(&serial).unwrap();
    certificate.set_serial_number(&serial).unwrap();
    certificate.set_subject_name(&name).unwrap();
    certificate.set_issuer_name(&name).unwrap();
    certificate.set_pubkey(&key).unwrap();
    certificate
        .set_not_before(&Asn1Time::days_from_now(0).unwrap())
        .unwrap();
    certificate
        .set_not_after(&Asn1Time::days_from_now(2).unwrap())
        .unwrap();
    let context = certificate.x509v3_context(None, None);
    let alt_names = SubjectAlternativeName::new()
        .dns("packages.example.test")
        .dns("cdn.example.test")
        .build(&context)
        .unwrap();
    certificate.append_extension(alt_names).unwrap();
    certificate.sign(&key, MessageDigest::sha384()).unwrap();
    let certificate = certificate.build().to_pem().unwrap();
    let mut signer = Signer::new(MessageDigest::sha384(), &key).unwrap();
    signer.update(b"composer-contract").unwrap();
    (certificate, signer.sign_to_vec().unwrap())
}

#[test]
fn openssl_composer_surface_is_backed_by_the_linked_library() {
    let version = openssl::version::version();
    let number = openssl::version::number();
    assert_eq!(
        run_php(
            r#"<?php
echo (int) extension_loaded('openssl'), (int) extension_loaded('OPENSSL'),
    (int) in_array('openssl', get_loaded_extensions(), true), "\n";
$functions = get_extension_funcs('openssl'); sort($functions);
echo implode(',', $functions), "\n";
foreach (['openssl_get_cert_locations','openssl_x509_parse','openssl_get_md_methods','openssl_pkey_get_public','openssl_pkey_get_details','openssl_verify','openssl_free_key'] as $name) {
    $function = new ReflectionFunction($name);
    echo $name, ':', $function->getExtensionName(), ':',
        $function->getNumberOfRequiredParameters(), '/', $function->getNumberOfParameters(), ':',
        $function->getReturnType(), "\n";
}
echo (new ReflectionClass(OpenSSLAsymmetricKey::class))->getExtensionName(), "\n";
echo OPENSSL_VERSION_TEXT, "\n", OPENSSL_VERSION_NUMBER, ':', OPENSSL_ALGO_SHA384, "\n";
$locations = openssl_get_cert_locations();
echo implode(',', array_keys($locations)), "\n";
echo (int) in_array('sha384', openssl_get_md_methods(), true), "\n";
"#,
        ),
        format!(
            concat!(
                "111\n",
                "openssl_free_key,openssl_get_cert_locations,openssl_get_md_methods,openssl_get_publickey,openssl_pkey_get_details,openssl_pkey_get_public,openssl_verify,openssl_x509_parse\n",
                "openssl_get_cert_locations:openssl:0/0:array\n",
                "openssl_x509_parse:openssl:1/2:array|false\n",
                "openssl_get_md_methods:openssl:0/1:array\n",
                "openssl_pkey_get_public:openssl:1/1:OpenSSLAsymmetricKey|false\n",
                "openssl_pkey_get_details:openssl:1/1:array|false\n",
                "openssl_verify:openssl:3/5:int|false\n",
                "openssl_free_key:openssl:1/1:void\n",
                "openssl\n",
                "{}\n{}:8\n",
                "default_cert_file,default_cert_file_env,default_cert_dir,default_cert_dir_env,default_private_dir,default_default_cert_area,ini_cafile,ini_capath\n",
                "1\n",
            ),
            version, number
        )
    );
}

#[test]
fn x509_public_key_details_and_signature_verification_cover_composer_security_checks() {
    let (certificate, signature) = certificate_and_signature();
    let source = format!(
        r#"<?php
$certificate = hex2bin('{}');
$signature = hex2bin('{}');
$key = openssl_pkey_get_public($certificate);
var_dump($key instanceof OpenSSLAsymmetricKey);
$parsed = openssl_x509_parse($certificate, false);
echo $parsed['subject']['countryName'], ':', $parsed['subject']['organizationName'], ':',
    $parsed['subject']['commonName'], "\n";
echo $parsed['extensions']['subjectAltName'], "\n";
var_dump($parsed['validFrom_time_t'] < $parsed['validTo_time_t']);
$details = openssl_pkey_get_details($key);
echo (int) ($details['bits'] >= 2048), ':', $details['type'], ':',
    (int) str_starts_with($details['key'], '-----BEGIN PUBLIC KEY-----'), ':',
    implode(',', array_keys($details)), "\n";
var_dump(
    openssl_verify('composer-contract', $signature, $key, OPENSSL_ALGO_SHA384),
    openssl_verify('wrong', $signature, $key, 'sha384')
);
openssl_free_key($key);
echo "freed\n";
"#,
        hex(&certificate),
        hex(&signature),
    );
    assert_eq!(
        run_php(&source),
        concat!(
            "bool(true)\n",
            "CZ:RPHP:packages.example.test\n",
            "DNS:packages.example.test, DNS:cdn.example.test\n",
            "bool(true)\n",
            "1:0:1:bits,key,rsa,type\n",
            "int(1)\n",
            "int(0)\n",
            "freed\n",
        )
    );
}
