//! Composer-facing OpenSSL contracts backed by the process OpenSSL library.
//!
//! PHP's extension is a binding to OpenSSL as well. Keeping certificates and
//! public keys in opaque native object state preserves that boundary without
//! exposing native pointers or delegating PHP execution to another runtime.

use std::rc::Rc;

use ::openssl::hash::MessageDigest;
use ::openssl::pkey::{Id, PKey, Public};
use ::openssl::rsa::Padding;
use ::openssl::sign::Verifier;
use ::openssl::x509::{X509, X509NameRef};

use super::*;
use crate::compiler::compile::ClassDef;
use crate::value::{NativeObjectState, ObjectLayout};
use crate::vm::function::InternalFunctionHandler;

const KEY_CLASS: &str = "OpenSSLAsymmetricKey";

#[derive(Clone, Default)]
struct PublicKeyState {
    pem: Vec<u8>,
}

impl NativeObjectState for PublicKeyState {
    fn clone_state(&self) -> Box<dyn NativeObjectState> {
        Box::new(self.clone())
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }

    fn as_any_mut(&mut self) -> &mut dyn std::any::Any {
        self
    }
}

fn register_key_class(eg: &mut ExecutorGlobals) {
    eg.register_class(ClassDef {
        attributes: Vec::new(),
        name: KEY_CLASS.to_string(),
        source_file: None,
        declaration_line: 0,
        end_line: 0,
        doc_comment: None,
        parent: None,
        implements: Vec::new(),
        is_interface: false,
        is_abstract: false,
        is_final: true,
        is_trait: false,
        is_enum: false,
        is_readonly: false,
        allow_dynamic_properties: false,
        uses: Vec::new(),
        trait_aliases: Vec::new(),
        trait_precedences: Vec::new(),
        properties: Vec::new(),
        static_properties: Vec::new(),
        constants: Vec::new(),
        property_layout: Rc::new(ObjectLayout::empty()),
        property_defaults: Rc::from([]),
        readonly_props: Vec::new(),
        methods: Vec::new(),
        abstract_methods: Vec::new(),
        enum_backing_error: None,
        deferred_instance_defaults: None,
        class_id: 0,
    })
    .expect("OpenSSLAsymmetricKey class name is unique");
}

struct FunctionContract {
    name: &'static str,
    handler: InternalFunctionHandler,
    required: u32,
    parameters: &'static [&'static str],
    hints: Vec<ParamTypeHint>,
    result: ParamTypeHint,
    defaults: Vec<Option<Value>>,
}

#[cold]
pub(super) fn register(eg: &mut ExecutorGlobals, functions: &mut Vec<Box<InternalFunction>>) {
    register_key_class(eg);
    let false_type = || ParamTypeHint::ClassName("false".into());
    let key_type = || ParamTypeHint::ClassName(KEY_CLASS.into());
    let contracts = [
        FunctionContract {
            name: "openssl_get_cert_locations",
            handler: get_cert_locations,
            required: 0,
            parameters: &[],
            hints: vec![],
            result: ParamTypeHint::Array,
            defaults: vec![],
        },
        FunctionContract {
            name: "openssl_x509_parse",
            handler: x509_parse,
            required: 1,
            parameters: &["certificate", "short_names"],
            hints: vec![
                ParamTypeHint::Union(vec![
                    ParamTypeHint::ClassName("OpenSSLCertificate".into()),
                    ParamTypeHint::String,
                ]),
                ParamTypeHint::Bool,
            ],
            result: ParamTypeHint::Union(vec![ParamTypeHint::Array, false_type()]),
            defaults: vec![None, Some(Value::bool(true))],
        },
        FunctionContract {
            name: "openssl_get_md_methods",
            handler: get_md_methods,
            required: 0,
            parameters: &["aliases"],
            hints: vec![ParamTypeHint::Bool],
            result: ParamTypeHint::Array,
            defaults: vec![Some(Value::bool(false))],
        },
        FunctionContract {
            name: "openssl_pkey_get_public",
            handler: pkey_get_public,
            required: 1,
            parameters: &["public_key"],
            hints: vec![ParamTypeHint::None],
            result: ParamTypeHint::Union(vec![key_type(), false_type()]),
            defaults: vec![None],
        },
        FunctionContract {
            name: "openssl_get_publickey",
            handler: pkey_get_public,
            required: 1,
            parameters: &["public_key"],
            hints: vec![ParamTypeHint::None],
            result: ParamTypeHint::Union(vec![key_type(), false_type()]),
            defaults: vec![None],
        },
        FunctionContract {
            name: "openssl_pkey_get_details",
            handler: pkey_get_details,
            required: 1,
            parameters: &["key"],
            hints: vec![key_type()],
            result: ParamTypeHint::Union(vec![ParamTypeHint::Array, false_type()]),
            defaults: vec![None],
        },
        FunctionContract {
            name: "openssl_verify",
            handler: verify,
            required: 3,
            parameters: &["data", "signature", "public_key", "algorithm", "padding"],
            hints: vec![
                ParamTypeHint::String,
                ParamTypeHint::String,
                ParamTypeHint::None,
                ParamTypeHint::Union(vec![ParamTypeHint::String, ParamTypeHint::Int]),
                ParamTypeHint::Int,
            ],
            result: ParamTypeHint::Union(vec![ParamTypeHint::Int, false_type()]),
            defaults: vec![None, None, None, Some(Value::long(1)), Some(Value::long(0))],
        },
        FunctionContract {
            name: "openssl_free_key",
            handler: free_key,
            required: 1,
            parameters: &["key"],
            hints: vec![key_type()],
            result: ParamTypeHint::Void,
            defaults: vec![None],
        },
    ];
    for contract in contracts {
        let mut function = Box::new(
            make_internal_function(
                contract.handler,
                contract.parameters.len() as u32,
                contract.required,
                Vec::new(),
            )
            .with_static_parameter_names(contract.parameters),
        );
        function.common.sig.param_type_hints = contract.hints;
        function.common.sig.return_type_hint = contract.result;
        let pointer = &function.common as *const FunctionCommon;
        eg.register_function(contract.name, pointer)
            .expect("OpenSSL function name is unique");
        eg.register_internal_function_reflection_metadata(pointer, contract.defaults, "openssl");
        functions.push(function);
    }
}

fn write(rv: *mut Value, value: Value) -> Result<(), VmError> {
    write_return_value(rv, value);
    Ok(())
}

fn path_or_bytes(bytes: &[u8]) -> Option<Vec<u8>> {
    if let Some(path) = bytes.strip_prefix(b"file://") {
        let path = std::str::from_utf8(path).ok()?;
        std::fs::read(path).ok()
    } else {
        Some(bytes.to_vec())
    }
}

fn certificate(bytes: &[u8]) -> Option<X509> {
    let bytes = path_or_bytes(bytes)?;
    X509::stack_from_pem(&bytes)
        .ok()
        .and_then(|mut certificates| (!certificates.is_empty()).then(|| certificates.remove(0)))
        .or_else(|| X509::from_der(&bytes).ok())
}

fn public_key(bytes: &[u8]) -> Option<PKey<Public>> {
    let bytes = path_or_bytes(bytes)?;
    PKey::public_key_from_pem(&bytes)
        .ok()
        .or_else(|| PKey::public_key_from_der(&bytes).ok())
        .or_else(|| certificate(&bytes)?.public_key().ok())
}

fn key_pem_from_value(value: &Value) -> Option<Vec<u8>> {
    let value = value.dereferenced();
    if let Some(bytes) = value.php_string_bytes() {
        return public_key(&bytes).and_then(|key| key.public_key_to_pem().ok());
    }
    value
        .as_object()?
        .native_object_state::<PublicKeyState>()
        .map(|state| state.pem.clone())
}

fn key_from_value(value: &Value) -> Option<PKey<Public>> {
    PKey::public_key_from_pem(&key_pem_from_value(value)?).ok()
}

fn key_object(eg: &ExecutorGlobals, pem: Vec<u8>) -> Option<Value> {
    let class = eg.find_class(KEY_CLASS)?;
    let value = Value::object(PhpObject::with_layout_from_defaults(
        class.class_id,
        Rc::clone(&class.property_layout),
        class.property_defaults.as_ref(),
    ));
    let mut object = value.as_object_mut()?;
    *object.native_object_state_mut::<PublicKeyState>() = PublicKeyState { pem };
    drop(object);
    Some(value)
}

fn get_cert_locations(
    _ed: *mut ExecuteData,
    rv: *mut Value,
    _eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let area = ::openssl::version::dir()
        .strip_prefix("OPENSSLDIR: \"")
        .and_then(|value| value.strip_suffix('"'))
        .unwrap_or("/usr/lib/ssl");
    let mut result = PhpArray::with_hash_capacity(8);
    result.set_str(
        "default_cert_file",
        Value::string(format!("{area}/cert.pem")),
    );
    result.set_str("default_cert_file_env", Value::string("SSL_CERT_FILE"));
    result.set_str("default_cert_dir", Value::string(format!("{area}/certs")));
    result.set_str("default_cert_dir_env", Value::string("SSL_CERT_DIR"));
    result.set_str(
        "default_private_dir",
        Value::string(format!("{area}/private")),
    );
    result.set_str("default_default_cert_area", Value::string(area));
    result.set_str("ini_cafile", Value::string(""));
    result.set_str("ini_capath", Value::string(""));
    write(rv, Value::array(result))
}

fn name_array(name: &X509NameRef, short: bool) -> PhpArray {
    let mut result = PhpArray::with_hash_capacity(name.entries().count());
    for entry in name.entries() {
        let nid = entry.object().nid();
        let key = if short {
            nid.short_name()
        } else {
            nid.long_name()
        };
        let Ok(key) = key else { continue };
        let Ok(value) = entry.data().to_string() else {
            continue;
        };
        result.set_str(key, Value::string(value));
    }
    result
}

fn display_name(name: &X509NameRef) -> String {
    let mut output = String::new();
    for entry in name.entries() {
        let Ok(key) = entry.object().nid().short_name() else {
            continue;
        };
        let Ok(value) = entry.data().to_string() else {
            continue;
        };
        output.push('/');
        output.push_str(key);
        output.push('=');
        output.push_str(&value);
    }
    output
}

fn timestamp(time: &::openssl::asn1::Asn1TimeRef) -> Option<i64> {
    let epoch = ::openssl::asn1::Asn1Time::from_unix(0).ok()?;
    let difference = epoch.diff(time).ok()?;
    Some(i64::from(difference.days) * 86_400 + i64::from(difference.secs))
}

fn civil_date(days: i64) -> (i64, u32, u32) {
    let z = days + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let day_of_era = z - era * 146_097;
    let year_of_era =
        (day_of_era - day_of_era / 1460 + day_of_era / 36_524 - day_of_era / 146_096) / 365;
    let mut year = year_of_era + era * 400;
    let day_of_year = day_of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
    let month_prime = (5 * day_of_year + 2) / 153;
    let day = day_of_year - (153 * month_prime + 2) / 5 + 1;
    let month = month_prime + if month_prime < 10 { 3 } else { -9 };
    year += i64::from(month <= 2);
    (year, month as u32, day as u32)
}

fn asn1_time_string(timestamp: i64) -> String {
    let days = timestamp.div_euclid(86_400);
    let seconds = timestamp.rem_euclid(86_400);
    let (year, month, day) = civil_date(days);
    let hour = seconds / 3600;
    let minute = seconds / 60 % 60;
    let second = seconds % 60;
    if (1950..2050).contains(&year) {
        format!(
            "{:02}{month:02}{day:02}{hour:02}{minute:02}{second:02}Z",
            year % 100
        )
    } else {
        format!("{year:04}{month:02}{day:02}{hour:02}{minute:02}{second:02}Z")
    }
}

fn subject_alt_name(certificate: &X509) -> Option<String> {
    let names = certificate.subject_alt_names()?;
    let mut values = Vec::new();
    for name in names {
        if let Some(value) = name.dnsname() {
            values.push(format!("DNS:{value}"));
        } else if let Some(value) = name.email() {
            values.push(format!("email:{value}"));
        } else if let Some(value) = name.uri() {
            values.push(format!("URI:{value}"));
        } else if let Some(value) = name.ipaddress() {
            let value = match value {
                [a, b, c, d] => format!("{a}.{b}.{c}.{d}"),
                bytes if bytes.len() == 16 => bytes
                    .chunks_exact(2)
                    .map(|part| format!("{:x}", u16::from_be_bytes([part[0], part[1]])))
                    .collect::<Vec<_>>()
                    .join(":"),
                _ => continue,
            };
            values.push(format!("IP Address:{value}"));
        }
    }
    (!values.is_empty()).then(|| values.join(", "))
}

fn x509_parse(
    ed: *mut ExecuteData,
    rv: *mut Value,
    _eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let value = arg!(ed, 0).dereferenced();
    let Some(bytes) = value.php_string_bytes() else {
        return write(rv, Value::bool(false));
    };
    let Some(certificate) = certificate(&bytes) else {
        return write(rv, Value::bool(false));
    };
    let short = arg_opt!(ed, 1).map_or(true, Value::is_truthy);
    let mut result = PhpArray::with_hash_capacity(16);
    result.set_str(
        "name",
        Value::string(display_name(certificate.subject_name())),
    );
    result.set_str(
        "subject",
        Value::array(name_array(certificate.subject_name(), short)),
    );
    result.set_str(
        "hash",
        Value::string(format!("{:08x}", certificate.subject_name_hash())),
    );
    result.set_str(
        "issuer",
        Value::array(name_array(certificate.issuer_name(), short)),
    );
    result.set_str("version", Value::long(i64::from(certificate.version())));
    if let Ok(serial) = certificate.serial_number().to_bn()
        && let Ok(hex) = serial.to_hex_str()
    {
        let hex = hex.to_string();
        result.set_str("serialNumber", Value::string(format!("0x{hex}")));
        result.set_str("serialNumberHex", Value::string(hex));
    }
    if let Some(from) = timestamp(certificate.not_before()) {
        result.set_str("validFrom", Value::string(asn1_time_string(from)));
        result.set_str("validFrom_time_t", Value::long(from));
    }
    if let Some(to) = timestamp(certificate.not_after()) {
        result.set_str("validTo", Value::string(asn1_time_string(to)));
        result.set_str("validTo_time_t", Value::long(to));
    }
    let signature = certificate.signature_algorithm().object().nid();
    if let Ok(name) = signature.short_name() {
        result.set_str("signatureTypeSN", Value::string(name));
    }
    if let Ok(name) = signature.long_name() {
        result.set_str("signatureTypeLN", Value::string(name));
    }
    result.set_str(
        "signatureTypeNID",
        Value::long(i64::from(signature.as_raw())),
    );
    if let Some(names) = subject_alt_name(&certificate) {
        let mut extensions = PhpArray::with_hash_capacity(1);
        extensions.set_str("subjectAltName", Value::string(names));
        result.set_str("extensions", Value::array(extensions));
    }
    write(rv, Value::array(result))
}

const DIGEST_METHODS: &[&str] = &[
    "blake2b512",
    "blake2s256",
    "md4",
    "md5",
    "md5-sha1",
    "ripemd160",
    "sha1",
    "sha224",
    "sha256",
    "sha3-224",
    "sha3-256",
    "sha3-384",
    "sha3-512",
    "sha384",
    "sha512",
    "sha512-224",
    "sha512-256",
    "shake128",
    "shake256",
    "sm3",
    "whirlpool",
];

fn get_md_methods(
    ed: *mut ExecuteData,
    rv: *mut Value,
    _eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let aliases = arg_opt!(ed, 0).is_some_and(Value::is_truthy);
    let methods: Vec<&str> = if aliases {
        // Composer only relies on canonical names. Keep the aliases projection
        // truthful by returning the canonical subset instead of inventing
        // provider-dependent aliases unavailable through rust-openssl.
        DIGEST_METHODS.to_vec()
    } else {
        DIGEST_METHODS.to_vec()
    };
    let mut result = PhpArray::with_packed_capacity(methods.len());
    for method in methods {
        if MessageDigest::from_name(method).is_some() {
            result.push(Value::string(method));
        }
    }
    write(rv, Value::array(result))
}

fn pkey_get_public(
    ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let supplied = arg!(ed, 0).dereferenced();
    if supplied
        .as_object()
        .is_some_and(|object| object.native_object_state::<PublicKeyState>().is_some())
    {
        return write(rv, supplied.clone());
    }
    let Some(bytes) = supplied.php_string_bytes() else {
        return write(rv, Value::bool(false));
    };
    let Some(pem) = public_key(&bytes).and_then(|key| key.public_key_to_pem().ok()) else {
        return write(rv, Value::bool(false));
    };
    write(
        rv,
        key_object(eg, pem).unwrap_or_else(|| Value::bool(false)),
    )
}

fn key_type(key: &PKey<Public>) -> i64 {
    match key.id() {
        Id::RSA => 0,
        Id::DSA => 1,
        Id::DH => 2,
        Id::EC => 3,
        _ => -1,
    }
}

fn pkey_get_details(
    ed: *mut ExecuteData,
    rv: *mut Value,
    _eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let Some(key) = key_from_value(arg!(ed, 0)) else {
        return write(rv, Value::bool(false));
    };
    let Some(pem) = key.public_key_to_pem().ok() else {
        return write(rv, Value::bool(false));
    };
    let mut result = PhpArray::with_hash_capacity(4);
    result.set_str("bits", Value::long(i64::from(key.bits())));
    result.set_str("key", php_byte_result(pem, false));
    if let Ok(rsa) = key.rsa() {
        let mut details = PhpArray::with_hash_capacity(2);
        details.set_str("n", php_byte_result(rsa.n().to_vec(), true));
        details.set_str("e", php_byte_result(rsa.e().to_vec(), true));
        result.set_str("rsa", Value::array(details));
    }
    result.set_str("type", Value::long(key_type(&key)));
    write(rv, Value::array(result))
}

fn digest(value: &Value) -> Option<MessageDigest> {
    if let Some(algorithm) = value.as_long() {
        return match algorithm {
            1 => Some(MessageDigest::sha1()),
            2 => Some(MessageDigest::md5()),
            6 => Some(MessageDigest::sha224()),
            7 => Some(MessageDigest::sha256()),
            8 => Some(MessageDigest::sha384()),
            9 => Some(MessageDigest::sha512()),
            _ => None,
        };
    }
    MessageDigest::from_name(value.as_str()?)
}

fn verify(ed: *mut ExecuteData, rv: *mut Value, _eg: &mut ExecutorGlobals) -> Result<(), VmError> {
    let Some(data) = arg!(ed, 0).php_string_bytes() else {
        return write(rv, Value::bool(false));
    };
    let Some(signature) = arg!(ed, 1).php_string_bytes() else {
        return write(rv, Value::bool(false));
    };
    let Some(key) = key_from_value(arg!(ed, 2)) else {
        return write(rv, Value::bool(false));
    };
    let algorithm = arg_opt!(ed, 3).cloned().unwrap_or_else(|| Value::long(1));
    let Some(digest) = digest(&algorithm) else {
        return write(rv, Value::bool(false));
    };
    let Ok(mut verifier) = Verifier::new(digest, &key) else {
        return write(rv, Value::bool(false));
    };
    let padding = arg_opt!(ed, 4).and_then(Value::as_long).unwrap_or(0);
    let padding = match padding {
        0 => None,
        1 => Some(Padding::PKCS1),
        6 => Some(Padding::PKCS1_PSS),
        _ => return write(rv, Value::bool(false)),
    };
    if let Some(padding) = padding
        && verifier.set_rsa_padding(padding).is_err()
    {
        return write(rv, Value::bool(false));
    }
    match verifier.verify_oneshot(&signature, &data) {
        Ok(valid) => write(rv, Value::long(i64::from(valid))),
        Err(_) => write(rv, Value::bool(false)),
    }
}

fn free_key(
    _ed: *mut ExecuteData,
    _rv: *mut Value,
    _eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    Ok(())
}

pub(crate) fn version_text() -> &'static str {
    ::openssl::version::version()
}

pub(crate) fn version_number() -> i64 {
    ::openssl::version::number() as i64
}
