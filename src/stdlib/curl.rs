//! Composer-facing cURL contracts backed by libcurl.
//!
//! The PHP extension is itself a libcurl binding.  This module keeps PHP
//! handles request-owned objects, projects only the options Composer uses, and
//! lets libcurl own TLS, proxy and content-decoding behavior.  The multi API is
//! deliberately driven synchronously for now, but preserves PHP's handle,
//! completion-message and error contracts instead of falling back to host PHP
//! or the curl executable.

use std::collections::{HashSet, VecDeque};
use std::rc::Rc;
use std::time::Duration;

use ::curl::Version;
use ::curl::easy::{Auth, Easy, HttpVersion, IpResolve, List};

use super::*;
use crate::compiler::compile::ClassDef;
use crate::value::{NativeObjectState, ObjectLayout};
use crate::vm::function::{InternalFunctionDeprecation, InternalFunctionHandler};

pub(crate) const EASY_CLASS: &str = "CurlHandle";
const MULTI_CLASS: &str = "CurlMultiHandle";
const SHARE_CLASS: &str = "CurlShareHandle";

const PHP_CURLOPT_RETURNTRANSFER: i64 = 19_913;
const CURLM_ADDED_ALREADY: i64 = 7;

#[derive(Clone, Default)]
struct CurlOptions {
    url: String,
    follow_location: bool,
    connect_timeout: u64,
    timeout: u64,
    body_stream: Option<i64>,
    header_stream: Option<i64>,
    header_callback: Option<Value>,
    accept_encoding: Option<String>,
    protocols: i64,
    ip_resolve: i64,
    http_version: i64,
    ca_info: Option<String>,
    ca_path: Option<String>,
    verify_peer: bool,
    verify_host: bool,
    certificate: Option<String>,
    private_key: Option<String>,
    private_key_password: Option<String>,
    headers: Vec<String>,
    custom_request: Option<String>,
    post_fields: Option<Vec<u8>>,
    proxy: Option<String>,
    no_proxy: Option<String>,
    proxy_auth: i64,
    proxy_user_password: Option<String>,
    proxy_ca_info: Option<String>,
    proxy_ca_path: Option<String>,
    return_transfer: bool,
    include_header: bool,
}

impl CurlOptions {
    fn fresh() -> Self {
        Self {
            verify_peer: true,
            verify_host: true,
            protocols: i64::from(curl_sys::CURLPROTO_HTTP | curl_sys::CURLPROTO_HTTPS),
            ..Self::default()
        }
    }
}

#[derive(Clone, Default)]
struct CurlInfo {
    url: String,
    http_code: i64,
    total_time: f64,
    namelookup_time: f64,
    connect_time: f64,
    pretransfer_time: f64,
    starttransfer_time: f64,
    redirect_time: f64,
    redirect_count: i64,
    size_download: f64,
    download_content_length: f64,
    header_size: i64,
    request_size: i64,
    primary_ip: String,
    primary_port: i64,
    local_ip: String,
    local_port: i64,
    content_type: Option<String>,
}

#[derive(Clone)]
struct CurlEasyState {
    options: CurlOptions,
    info: CurlInfo,
    errno: i64,
    error: String,
    returned_body: Vec<u8>,
}

impl Default for CurlEasyState {
    fn default() -> Self {
        Self {
            options: CurlOptions::fresh(),
            info: CurlInfo::default(),
            errno: 0,
            error: String::new(),
            returned_body: Vec::new(),
        }
    }
}

impl NativeObjectState for CurlEasyState {
    fn clone_state(&self) -> Box<dyn NativeObjectState> {
        Box::new(self.clone())
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }

    fn as_any_mut(&mut self) -> &mut dyn std::any::Any {
        self
    }

    fn for_each_value(&self, visit: &mut dyn FnMut(&Value)) {
        if let Some(callback) = &self.options.header_callback {
            visit(callback);
        }
    }

    fn append_values_reversed(&mut self, pending: &mut Vec<Value>) {
        if let Some(callback) = self.options.header_callback.take() {
            pending.push(callback);
        }
    }
}

#[derive(Clone, Default)]
struct CurlMultiState {
    handles: Vec<Value>,
    completed: HashSet<usize>,
    messages: VecDeque<(Value, i64)>,
    errno: i64,
    max_host_connections: i64,
    pipelining: i64,
}

impl NativeObjectState for CurlMultiState {
    fn clone_state(&self) -> Box<dyn NativeObjectState> {
        Box::new(self.clone())
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }

    fn as_any_mut(&mut self) -> &mut dyn std::any::Any {
        self
    }

    fn for_each_value(&self, visit: &mut dyn FnMut(&Value)) {
        for value in &self.handles {
            visit(value);
        }
        for (value, _) in &self.messages {
            visit(value);
        }
    }

    fn append_values_reversed(&mut self, pending: &mut Vec<Value>) {
        for (value, _) in self.messages.drain(..).rev() {
            pending.push(value);
        }
        pending.extend(self.handles.drain(..).rev());
        self.completed.clear();
    }
}

#[derive(Clone, Default)]
struct CurlShareState {
    shared: Vec<i64>,
    errno: i64,
}

impl NativeObjectState for CurlShareState {
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

struct Contract {
    name: &'static str,
    handler: InternalFunctionHandler,
    required: u32,
    parameters: &'static [&'static str],
    hints: Vec<ParamTypeHint>,
    result: ParamTypeHint,
    defaults: Vec<Option<Value>>,
    ref_args: u64,
}

static CLOSE_DEPRECATION: InternalFunctionDeprecation = InternalFunctionDeprecation {
    since: "8.5",
    message: "as it has no effect since PHP 8.0",
};

fn empty_final_class(name: &str) -> ClassDef {
    ClassDef {
        attributes: Vec::new(),
        name: name.to_string(),
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
    }
}

#[cold]
pub(super) fn register(eg: &mut ExecutorGlobals, functions: &mut Vec<Box<InternalFunction>>) {
    for name in [EASY_CLASS, MULTI_CLASS, SHARE_CLASS] {
        eg.register_class(empty_final_class(name))
            .expect("cURL handle class name is unique");
    }

    let easy = || ParamTypeHint::ClassName(EASY_CLASS.into());
    let multi = || ParamTypeHint::ClassName(MULTI_CLASS.into());
    let share = || ParamTypeHint::ClassName(SHARE_CLASS.into());
    let false_type = || ParamTypeHint::ClassName("false".into());
    let contracts = [
        Contract {
            name: "curl_init",
            handler: curl_init,
            required: 0,
            parameters: &["url"],
            hints: vec![ParamTypeHint::Nullable(Box::new(ParamTypeHint::String))],
            result: ParamTypeHint::Union(vec![easy(), false_type()]),
            defaults: vec![Some(Value::null())],
            ref_args: 0,
        },
        Contract {
            name: "curl_close",
            handler: curl_close,
            required: 1,
            parameters: &["handle"],
            hints: vec![easy()],
            result: ParamTypeHint::Void,
            defaults: vec![None],
            ref_args: 0,
        },
        Contract {
            name: "curl_setopt",
            handler: curl_setopt,
            required: 3,
            parameters: &["handle", "option", "value"],
            hints: vec![easy(), ParamTypeHint::Int, ParamTypeHint::Mixed],
            result: ParamTypeHint::Bool,
            defaults: vec![None, None, None],
            ref_args: 0,
        },
        Contract {
            name: "curl_setopt_array",
            handler: curl_setopt_array,
            required: 2,
            parameters: &["handle", "options"],
            hints: vec![easy(), ParamTypeHint::Array],
            result: ParamTypeHint::Bool,
            defaults: vec![None, None],
            ref_args: 0,
        },
        Contract {
            name: "curl_exec",
            handler: curl_exec,
            required: 1,
            parameters: &["handle"],
            hints: vec![easy()],
            result: ParamTypeHint::Union(vec![ParamTypeHint::String, ParamTypeHint::Bool]),
            defaults: vec![None],
            ref_args: 0,
        },
        Contract {
            name: "curl_getinfo",
            handler: curl_getinfo,
            required: 1,
            parameters: &["handle", "option"],
            hints: vec![
                easy(),
                ParamTypeHint::Nullable(Box::new(ParamTypeHint::Int)),
            ],
            result: ParamTypeHint::Mixed,
            defaults: vec![None, Some(Value::null())],
            ref_args: 0,
        },
        Contract {
            name: "curl_errno",
            handler: curl_errno,
            required: 1,
            parameters: &["handle"],
            hints: vec![easy()],
            result: ParamTypeHint::Int,
            defaults: vec![None],
            ref_args: 0,
        },
        Contract {
            name: "curl_error",
            handler: curl_error,
            required: 1,
            parameters: &["handle"],
            hints: vec![easy()],
            result: ParamTypeHint::String,
            defaults: vec![None],
            ref_args: 0,
        },
        Contract {
            name: "curl_strerror",
            handler: curl_strerror,
            required: 1,
            parameters: &["error_code"],
            hints: vec![ParamTypeHint::Int],
            result: ParamTypeHint::Nullable(Box::new(ParamTypeHint::String)),
            defaults: vec![None],
            ref_args: 0,
        },
        Contract {
            name: "curl_version",
            handler: curl_version,
            required: 0,
            parameters: &[],
            hints: vec![],
            result: ParamTypeHint::Union(vec![ParamTypeHint::Array, false_type()]),
            defaults: vec![],
            ref_args: 0,
        },
        Contract {
            name: "curl_multi_init",
            handler: curl_multi_init,
            required: 0,
            parameters: &[],
            hints: vec![],
            result: multi(),
            defaults: vec![],
            ref_args: 0,
        },
        Contract {
            name: "curl_multi_setopt",
            handler: curl_multi_setopt,
            required: 3,
            parameters: &["multi_handle", "option", "value"],
            hints: vec![multi(), ParamTypeHint::Int, ParamTypeHint::Mixed],
            result: ParamTypeHint::Bool,
            defaults: vec![None, None, None],
            ref_args: 0,
        },
        Contract {
            name: "curl_multi_add_handle",
            handler: curl_multi_add_handle,
            required: 2,
            parameters: &["multi_handle", "handle"],
            hints: vec![multi(), easy()],
            result: ParamTypeHint::Int,
            defaults: vec![None, None],
            ref_args: 0,
        },
        Contract {
            name: "curl_multi_remove_handle",
            handler: curl_multi_remove_handle,
            required: 2,
            parameters: &["multi_handle", "handle"],
            hints: vec![multi(), easy()],
            result: ParamTypeHint::Int,
            defaults: vec![None, None],
            ref_args: 0,
        },
        Contract {
            name: "curl_multi_exec",
            handler: curl_multi_exec,
            required: 2,
            parameters: &["multi_handle", "still_running"],
            hints: vec![multi(), ParamTypeHint::Mixed],
            result: ParamTypeHint::Int,
            defaults: vec![None, None],
            ref_args: 0b10,
        },
        Contract {
            name: "curl_multi_select",
            handler: curl_multi_select,
            required: 1,
            parameters: &["multi_handle", "timeout"],
            hints: vec![multi(), ParamTypeHint::Float],
            result: ParamTypeHint::Int,
            defaults: vec![None, Some(Value::double(1.0))],
            ref_args: 0,
        },
        Contract {
            name: "curl_multi_info_read",
            handler: curl_multi_info_read,
            required: 1,
            parameters: &["multi_handle", "queued_messages"],
            hints: vec![multi(), ParamTypeHint::Mixed],
            result: ParamTypeHint::Union(vec![ParamTypeHint::Array, false_type()]),
            defaults: vec![None, Some(Value::null())],
            ref_args: 0b10,
        },
        Contract {
            name: "curl_share_init",
            handler: curl_share_init,
            required: 0,
            parameters: &[],
            hints: vec![],
            result: share(),
            defaults: vec![],
            ref_args: 0,
        },
        Contract {
            name: "curl_share_setopt",
            handler: curl_share_setopt,
            required: 3,
            parameters: &["share_handle", "option", "value"],
            hints: vec![share(), ParamTypeHint::Int, ParamTypeHint::Mixed],
            result: ParamTypeHint::Bool,
            defaults: vec![None, None, None],
            ref_args: 0,
        },
    ];

    for contract in contracts {
        let mut function = Box::new(
            if contract.ref_args == 0 {
                make_internal_function(
                    contract.handler,
                    contract.parameters.len() as u32,
                    contract.required,
                    Vec::new(),
                )
            } else {
                make_internal_function_ref(
                    contract.handler,
                    contract.parameters.len() as u32,
                    contract.required,
                    contract.ref_args,
                    Vec::new(),
                )
            }
            .with_static_parameter_names(contract.parameters),
        );
        function.common.sig.param_type_hints = contract.hints;
        function.common.sig.return_type_hint = contract.result;
        if contract.name == "curl_close" {
            function.set_deprecation(&CLOSE_DEPRECATION);
        }
        let pointer = &function.common as *const FunctionCommon;
        eg.register_function(contract.name, pointer)
            .expect("cURL function name is unique");
        eg.register_internal_function_reflection_metadata(pointer, contract.defaults, "curl");
        functions.push(function);
    }
}

fn native_object<T: NativeObjectState + Default>(
    eg: &ExecutorGlobals,
    class_name: &str,
) -> Option<Value> {
    let class = eg.find_class(class_name)?;
    let value = Value::object(PhpObject::with_layout_from_defaults(
        class.class_id,
        Rc::clone(&class.property_layout),
        class.property_defaults.as_ref(),
    ));
    value.as_object_mut()?.native_object_state_mut::<T>();
    Some(value)
}

fn easy_options(value: &Value) -> Option<CurlOptions> {
    value
        .dereferenced()
        .as_object()?
        .native_object_state::<CurlEasyState>()
        .map(|state| state.options.clone())
}

fn set_easy_state(value: &Value, state: CurlEasyState) -> bool {
    let value = value.dereferenced();
    let Some(mut object) = value.as_object_mut() else {
        return false;
    };
    *object.native_object_state_mut::<CurlEasyState>() = state;
    true
}

fn with_easy_state<T>(value: &Value, operation: impl FnOnce(&CurlEasyState) -> T) -> Option<T> {
    let value = value.dereferenced();
    let object = value.as_object()?;
    Some(operation(object.native_object_state::<CurlEasyState>()?))
}

fn with_easy_state_mut<T>(
    value: &Value,
    operation: impl FnOnce(&mut CurlEasyState) -> T,
) -> Option<T> {
    let value = value.dereferenced();
    let mut object = value.as_object_mut()?;
    Some(operation(object.native_object_state_mut::<CurlEasyState>()))
}

fn with_multi_state_mut<T>(
    value: &Value,
    operation: impl FnOnce(&mut CurlMultiState) -> T,
) -> Option<T> {
    let value = value.dereferenced();
    let mut object = value.as_object_mut()?;
    Some(operation(
        object.native_object_state_mut::<CurlMultiState>(),
    ))
}

fn with_share_state_mut<T>(
    value: &Value,
    operation: impl FnOnce(&mut CurlShareState) -> T,
) -> Option<T> {
    let value = value.dereferenced();
    let mut object = value.as_object_mut()?;
    Some(operation(
        object.native_object_state_mut::<CurlShareState>(),
    ))
}

fn write(rv: *mut Value, value: Value) -> Result<(), VmError> {
    write_return_value(rv, value);
    Ok(())
}

fn string_option(value: &Value) -> Option<String> {
    let bytes = value.dereferenced().php_string_bytes()?;
    Some(String::from_utf8_lossy(bytes.as_ref()).into_owned())
}

fn bytes_option(value: &Value) -> Option<Vec<u8>> {
    value
        .dereferenced()
        .php_string_bytes()
        .map(|bytes| bytes.into_owned())
}

fn set_option(state: &mut CurlEasyState, option: i64, value: &Value) -> bool {
    let options = &mut state.options;
    match option {
        code if code == i64::from(curl_sys::CURLOPT_URL) => {
            let Some(value) = string_option(value) else {
                return false;
            };
            options.url = value;
        }
        code if code == i64::from(curl_sys::CURLOPT_FOLLOWLOCATION) => {
            options.follow_location = value.is_truthy();
        }
        code if code == i64::from(curl_sys::CURLOPT_CONNECTTIMEOUT) => {
            options.connect_timeout = value.to_long_val().max(0) as u64;
        }
        code if code == i64::from(curl_sys::CURLOPT_TIMEOUT) => {
            options.timeout = value.to_long_val().max(0) as u64;
        }
        code if code == i64::from(curl_sys::CURLOPT_FILE) => {
            options.body_stream = value.as_resource_id();
            if options.body_stream.is_none() {
                return false;
            }
        }
        code if code == i64::from(curl_sys::CURLOPT_WRITEHEADER) => {
            options.header_stream = value.as_resource_id();
            if options.header_stream.is_none() {
                return false;
            }
        }
        code if code == i64::from(curl_sys::CURLOPT_HEADERFUNCTION) => {
            options.header_callback = Some(value.clone());
        }
        code if code == i64::from(curl_sys::CURLOPT_ACCEPT_ENCODING) => {
            let Some(value) = string_option(value) else {
                return false;
            };
            options.accept_encoding = Some(value);
        }
        code if code == i64::from(curl_sys::CURLOPT_PROTOCOLS) => {
            options.protocols = value.to_long_val();
        }
        code if code == i64::from(curl_sys::CURLOPT_IPRESOLVE) => {
            options.ip_resolve = value.to_long_val();
        }
        code if code == i64::from(curl_sys::CURLOPT_SHARE) => {
            if value
                .as_object()
                .is_none_or(|object| !object.class_name.eq_ignore_ascii_case(SHARE_CLASS))
            {
                return false;
            }
        }
        code if code == i64::from(curl_sys::CURLOPT_HTTP_VERSION) => {
            options.http_version = value.to_long_val();
        }
        code if code == i64::from(curl_sys::CURLOPT_CAINFO) => {
            options.ca_info = string_option(value);
            if options.ca_info.is_none() {
                return false;
            }
        }
        code if code == i64::from(curl_sys::CURLOPT_CAPATH) => {
            options.ca_path = string_option(value);
            if options.ca_path.is_none() {
                return false;
            }
        }
        code if code == i64::from(curl_sys::CURLOPT_SSL_VERIFYPEER) => {
            options.verify_peer = value.is_truthy();
        }
        code if code == i64::from(curl_sys::CURLOPT_SSL_VERIFYHOST) => {
            options.verify_host = value.to_long_val() != 0;
        }
        code if code == i64::from(curl_sys::CURLOPT_SSLCERT) => {
            options.certificate = string_option(value);
            if options.certificate.is_none() {
                return false;
            }
        }
        code if code == i64::from(curl_sys::CURLOPT_SSLKEY) => {
            options.private_key = string_option(value);
            if options.private_key.is_none() {
                return false;
            }
        }
        code if code == i64::from(curl_sys::CURLOPT_KEYPASSWD) => {
            options.private_key_password = string_option(value);
            if options.private_key_password.is_none() {
                return false;
            }
        }
        code if code == i64::from(curl_sys::CURLOPT_HTTPHEADER) => {
            let Some(array) = value.as_array() else {
                return false;
            };
            let mut headers = Vec::with_capacity(array.len());
            for header in array.values() {
                let Some(header) = string_option(header) else {
                    return false;
                };
                headers.push(header);
            }
            options.headers = headers;
        }
        code if code == i64::from(curl_sys::CURLOPT_CUSTOMREQUEST) => {
            options.custom_request = string_option(value);
            if options.custom_request.is_none() {
                return false;
            }
        }
        code if code == i64::from(curl_sys::CURLOPT_POSTFIELDS) => {
            options.post_fields = bytes_option(value);
            if options.post_fields.is_none() {
                return false;
            }
        }
        code if code == i64::from(curl_sys::CURLOPT_PROXY) => {
            options.proxy = string_option(value);
            if options.proxy.is_none() {
                return false;
            }
        }
        code if code == i64::from(curl_sys::CURLOPT_NOPROXY) => {
            options.no_proxy = string_option(value);
            if options.no_proxy.is_none() {
                return false;
            }
        }
        code if code == i64::from(curl_sys::CURLOPT_PROXYAUTH) => {
            options.proxy_auth = value.to_long_val();
        }
        code if code == i64::from(curl_sys::CURLOPT_PROXYUSERPWD) => {
            options.proxy_user_password = string_option(value);
            if options.proxy_user_password.is_none() {
                return false;
            }
        }
        code if code == i64::from(curl_sys::CURLOPT_PROXY_CAINFO) => {
            options.proxy_ca_info = string_option(value);
            if options.proxy_ca_info.is_none() {
                return false;
            }
        }
        code if code == i64::from(curl_sys::CURLOPT_PROXY_CAPATH) => {
            options.proxy_ca_path = string_option(value);
            if options.proxy_ca_path.is_none() {
                return false;
            }
        }
        PHP_CURLOPT_RETURNTRANSFER => options.return_transfer = value.is_truthy(),
        code if code == i64::from(curl_sys::CURLOPT_HEADER) => {
            options.include_header = value.is_truthy();
        }
        _ => return false,
    }
    true
}

fn apply_options(easy: &mut Easy, options: &CurlOptions) -> Result<(), ::curl::Error> {
    easy.url(&options.url)?;
    easy.follow_location(options.follow_location)?;
    if options.connect_timeout != 0 {
        easy.connect_timeout(Duration::from_secs(options.connect_timeout))?;
    }
    if options.timeout != 0 {
        easy.timeout(Duration::from_secs(options.timeout))?;
    }
    if let Some(encoding) = &options.accept_encoding {
        easy.accept_encoding(encoding)?;
    }
    match options.ip_resolve {
        value if value == i64::from(curl_sys::CURL_IPRESOLVE_V4) => {
            easy.ip_resolve(IpResolve::V4)?;
        }
        value if value == i64::from(curl_sys::CURL_IPRESOLVE_V6) => {
            easy.ip_resolve(IpResolve::V6)?;
        }
        _ => {}
    }
    match options.http_version {
        value if value == i64::from(curl_sys::CURL_HTTP_VERSION_2_0) => {
            easy.http_version(HttpVersion::V2)?;
        }
        value if value == i64::from(curl_sys::CURL_HTTP_VERSION_1_0) => {
            easy.http_version(HttpVersion::V10)?;
        }
        value if value == i64::from(curl_sys::CURL_HTTP_VERSION_1_1) => {
            easy.http_version(HttpVersion::V11)?;
        }
        _ => {}
    }
    easy.ssl_verify_peer(options.verify_peer)?;
    easy.ssl_verify_host(options.verify_host)?;
    if let Some(path) = &options.ca_info {
        easy.cainfo(path)?;
    }
    if let Some(path) = &options.ca_path {
        easy.capath(path)?;
    }
    if let Some(path) = &options.certificate {
        easy.ssl_cert(path)?;
    }
    if let Some(path) = &options.private_key {
        easy.ssl_key(path)?;
    }
    if let Some(password) = &options.private_key_password {
        easy.key_password(password)?;
    }
    if !options.headers.is_empty() {
        let mut headers = List::new();
        for header in &options.headers {
            headers.append(header)?;
        }
        easy.http_headers(headers)?;
    }
    if let Some(method) = &options.custom_request {
        easy.custom_request(method)?;
    }
    if let Some(fields) = &options.post_fields {
        easy.post_fields_copy(fields)?;
    }
    if let Some(proxy) = &options.proxy {
        easy.proxy(proxy)?;
    }
    if let Some(no_proxy) = &options.no_proxy {
        easy.noproxy(no_proxy)?;
    }
    if options.proxy_auth & curl_sys::CURLAUTH_BASIC as i64 != 0 {
        let mut auth = Auth::new();
        auth.basic(true);
        easy.proxy_auth(&auth)?;
    }
    if let Some(credentials) = &options.proxy_user_password {
        let (user, password) = credentials.split_once(':').unwrap_or((credentials, ""));
        easy.proxy_username(user)?;
        easy.proxy_password(password)?;
    }
    if let Some(path) = &options.proxy_ca_info {
        easy.proxy_cainfo(path)?;
    }
    if let Some(path) = &options.proxy_ca_path {
        easy.proxy_capath(path)?;
    }
    Ok(())
}

fn duration_seconds(value: Result<Duration, ::curl::Error>) -> f64 {
    value.map_or(0.0, |duration| duration.as_secs_f64())
}

fn perform(value: &Value, eg: &mut ExecutorGlobals, frame: *mut ExecuteData) -> i64 {
    let Some(options) = easy_options(value) else {
        return i64::from(curl_sys::CURLE_FAILED_INIT);
    };
    let mut state = CurlEasyState {
        options: options.clone(),
        ..CurlEasyState::default()
    };
    state.info.url = options.url.clone();
    if options.url.is_empty()
        || !options.url.starts_with("http://") && !options.url.starts_with("https://")
        || options.url.starts_with("http://")
            && options.protocols & i64::from(curl_sys::CURLPROTO_HTTP) == 0
        || options.url.starts_with("https://")
            && options.protocols & i64::from(curl_sys::CURLPROTO_HTTPS) == 0
    {
        state.errno = i64::from(curl_sys::CURLE_UNSUPPORTED_PROTOCOL);
        state.error = ::curl::Error::new(curl_sys::CURLE_UNSUPPORTED_PROTOCOL)
            .description()
            .to_string();
        set_easy_state(value, state);
        return i64::from(curl_sys::CURLE_UNSUPPORTED_PROTOCOL);
    }

    let mut easy = Easy::new();
    if let Err(error) = apply_options(&mut easy, &options) {
        state.errno = i64::from(error.code());
        state.error = error.to_string();
        set_easy_state(value, state);
        return i64::from(error.code());
    }
    let mut body = Vec::new();
    let mut headers = Vec::new();
    let result = {
        let mut transfer = easy.transfer();
        let write = transfer.write_function(|data| {
            body.extend_from_slice(data);
            Ok(data.len())
        });
        let header = transfer.header_function(|data| {
            headers.extend_from_slice(data);
            true
        });
        match (write, header) {
            (Ok(()), Ok(())) => transfer.perform(),
            (Err(error), _) | (_, Err(error)) => Err(error),
        }
    };

    state.info.url = easy
        .effective_url()
        .ok()
        .flatten()
        .unwrap_or(&options.url)
        .to_string();
    state.info.http_code = i64::from(easy.response_code().unwrap_or_default());
    state.info.total_time = duration_seconds(easy.total_time());
    state.info.namelookup_time = duration_seconds(easy.namelookup_time());
    state.info.connect_time = duration_seconds(easy.connect_time());
    state.info.pretransfer_time = duration_seconds(easy.pretransfer_time());
    state.info.starttransfer_time = duration_seconds(easy.starttransfer_time());
    state.info.redirect_time = duration_seconds(easy.redirect_time());
    state.info.redirect_count = i64::from(easy.redirect_count().unwrap_or_default());
    state.info.size_download = easy.download_size().unwrap_or_default();
    state.info.download_content_length = easy.content_length_download().unwrap_or(-1.0);
    state.info.header_size = easy.header_size().unwrap_or_default() as i64;
    state.info.request_size = easy.request_size().unwrap_or_default() as i64;
    state.info.primary_ip = easy
        .primary_ip()
        .ok()
        .flatten()
        .unwrap_or_default()
        .to_string();
    state.info.primary_port = i64::from(easy.primary_port().unwrap_or_default());
    state.info.local_ip = easy
        .local_ip()
        .ok()
        .flatten()
        .unwrap_or_default()
        .to_string();
    state.info.local_port = i64::from(easy.local_port().unwrap_or_default());
    state.info.content_type = easy.content_type().ok().flatten().map(str::to_string);

    if let Err(error) = result {
        state.errno = i64::from(error.code());
        state.error = error.to_string();
        set_easy_state(value, state);
        return i64::from(error.code());
    }

    let stream_failed = if let Some(id) = options.header_stream {
        !matches!(
            super::streams::write_stream_bytes(eg, frame, id, &headers),
            Ok(Some(Ok(written))) if written == headers.len()
        )
    } else {
        false
    } || if let Some(id) = options.body_stream {
        !matches!(
            super::streams::write_stream_bytes(eg, frame, id, &body),
            Ok(Some(Ok(written))) if written == body.len()
        )
    } else {
        false
    };
    if stream_failed {
        state.errno = i64::from(curl_sys::CURLE_WRITE_ERROR);
        state.error = ::curl::Error::new(curl_sys::CURLE_WRITE_ERROR)
            .description()
            .to_string();
        set_easy_state(value, state);
        return i64::from(curl_sys::CURLE_WRITE_ERROR);
    }

    if options.return_transfer {
        if options.include_header {
            state.returned_body.extend_from_slice(&headers);
        }
        state.returned_body.extend_from_slice(&body);
    } else if options.body_stream.is_none() {
        if options.include_header
            && let Err(error) = eg.write_output(&headers)
        {
            state.errno = i64::from(curl_sys::CURLE_WRITE_ERROR);
            state.error = error.to_string();
            set_easy_state(value, state);
            return i64::from(curl_sys::CURLE_WRITE_ERROR);
        }
        if let Err(error) = eg.write_output(&body) {
            state.errno = i64::from(curl_sys::CURLE_WRITE_ERROR);
            state.error = error.to_string();
            set_easy_state(value, state);
            return i64::from(curl_sys::CURLE_WRITE_ERROR);
        }
    }
    state.errno = 0;
    state.error.clear();
    set_easy_state(value, state);
    i64::from(curl_sys::CURLE_OK)
}

fn curl_init(
    ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let Some(value) = native_object::<CurlEasyState>(eg, EASY_CLASS) else {
        return write(rv, Value::bool(false));
    };
    if let Some(url) = arg_opt!(ed, 0).filter(|value| value.value_type() != ValueType::Null)
        && let Some(url) = string_option(url)
    {
        with_easy_state_mut(&value, |state| state.options.url = url);
    }
    write(rv, value)
}

fn curl_close(
    _ed: *mut ExecuteData,
    rv: *mut Value,
    _eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    write(rv, Value::null())
}

fn curl_setopt(
    ed: *mut ExecuteData,
    rv: *mut Value,
    _eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let option = arg!(ed, 1).to_long_val();
    let accepted = with_easy_state_mut(arg!(ed, 0), |state| set_option(state, option, arg!(ed, 2)))
        .unwrap_or(false);
    write(rv, Value::bool(accepted))
}

fn curl_setopt_array(
    ed: *mut ExecuteData,
    rv: *mut Value,
    _eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let options = arg!(ed, 1)
        .as_array()
        .expect("validated cURL options array")
        .iter()
        .map(|(key, value)| (key, value.clone()))
        .collect::<Vec<_>>();
    let accepted = with_easy_state_mut(arg!(ed, 0), |state| {
        for (key, value) in &options {
            let ArrayKey::Int(option) = key else {
                return false;
            };
            if !set_option(state, *option, value) {
                return false;
            }
        }
        true
    })
    .unwrap_or(false);
    write(rv, Value::bool(accepted))
}

fn curl_exec(
    ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let handle = arg!(ed, 0).clone();
    let code = perform(&handle, eg, ed);
    if code != i64::from(curl_sys::CURLE_OK) {
        return write(rv, Value::bool(false));
    }
    let result = with_easy_state(&handle, |state| {
        if state.options.return_transfer {
            super::php_byte_result(state.returned_body.clone(), false)
        } else {
            Value::bool(true)
        }
    })
    .unwrap_or_else(|| Value::bool(false));
    write(rv, result)
}

fn info_array(state: &CurlEasyState) -> PhpArray {
    let info = &state.info;
    let mut result = PhpArray::with_hash_capacity(20);
    result.set_str("url", Value::string(&info.url));
    result.set_str(
        "content_type",
        info.content_type
            .as_ref()
            .map_or_else(Value::null, Value::string),
    );
    result.set_str("http_code", Value::long(info.http_code));
    result.set_str("header_size", Value::long(info.header_size));
    result.set_str("request_size", Value::long(info.request_size));
    result.set_str("total_time", Value::double(info.total_time));
    result.set_str("namelookup_time", Value::double(info.namelookup_time));
    result.set_str("connect_time", Value::double(info.connect_time));
    result.set_str("pretransfer_time", Value::double(info.pretransfer_time));
    result.set_str("starttransfer_time", Value::double(info.starttransfer_time));
    result.set_str("redirect_count", Value::long(info.redirect_count));
    result.set_str("redirect_time", Value::double(info.redirect_time));
    result.set_str("size_download", Value::double(info.size_download));
    result.set_str(
        "download_content_length",
        Value::double(info.download_content_length),
    );
    result.set_str("primary_ip", Value::string(&info.primary_ip));
    result.set_str("primary_port", Value::long(info.primary_port));
    result.set_str("local_ip", Value::string(&info.local_ip));
    result.set_str("local_port", Value::long(info.local_port));
    result
}

fn selected_info(state: &CurlEasyState, option: i64) -> Value {
    let info = &state.info;
    match option {
        value if value == i64::from(curl_sys::CURLINFO_EFFECTIVE_URL) => Value::string(&info.url),
        value if value == i64::from(curl_sys::CURLINFO_RESPONSE_CODE) => {
            Value::long(info.http_code)
        }
        value if value == i64::from(curl_sys::CURLINFO_TOTAL_TIME) => {
            Value::double(info.total_time)
        }
        value if value == i64::from(curl_sys::CURLINFO_NAMELOOKUP_TIME) => {
            Value::double(info.namelookup_time)
        }
        value if value == i64::from(curl_sys::CURLINFO_CONNECT_TIME) => {
            Value::double(info.connect_time)
        }
        value if value == i64::from(curl_sys::CURLINFO_PRETRANSFER_TIME) => {
            Value::double(info.pretransfer_time)
        }
        value if value == i64::from(curl_sys::CURLINFO_STARTTRANSFER_TIME) => {
            Value::double(info.starttransfer_time)
        }
        value if value == i64::from(curl_sys::CURLINFO_REDIRECT_TIME) => {
            Value::double(info.redirect_time)
        }
        value if value == i64::from(curl_sys::CURLINFO_SIZE_DOWNLOAD) => {
            Value::double(info.size_download)
        }
        value if value == i64::from(curl_sys::CURLINFO_CONTENT_LENGTH_DOWNLOAD) => {
            Value::double(info.download_content_length)
        }
        value if value == i64::from(curl_sys::CURLINFO_PRIMARY_IP) => {
            Value::string(&info.primary_ip)
        }
        _ => Value::bool(false),
    }
}

fn curl_getinfo(
    ed: *mut ExecuteData,
    rv: *mut Value,
    _eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let option = arg_opt!(ed, 1)
        .filter(|value| value.value_type() != ValueType::Null)
        .map(Value::to_long_val);
    let result = with_easy_state(arg!(ed, 0), |state| match option {
        Some(option) => selected_info(state, option),
        None => Value::array(info_array(state)),
    })
    .unwrap_or_else(|| Value::bool(false));
    write(rv, result)
}

fn curl_errno(
    ed: *mut ExecuteData,
    rv: *mut Value,
    _eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    write(
        rv,
        Value::long(with_easy_state(arg!(ed, 0), |state| state.errno).unwrap_or_default()),
    )
}

fn curl_error(
    ed: *mut ExecuteData,
    rv: *mut Value,
    _eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    write(
        rv,
        Value::string(
            with_easy_state(arg!(ed, 0), |state| state.error.clone()).unwrap_or_default(),
        ),
    )
}

fn curl_strerror(
    ed: *mut ExecuteData,
    rv: *mut Value,
    _eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let code = arg!(ed, 0).to_long_val();
    if code < 0 || code > i64::from(u32::MAX) {
        return write(rv, Value::null());
    }
    write(
        rv,
        Value::string(::curl::Error::new(code as curl_sys::CURLcode).description()),
    )
}

fn curl_version(
    _ed: *mut ExecuteData,
    rv: *mut Value,
    _eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let version = Version::get();
    let mut features = 0i64;
    for (enabled, flag) in [
        (version.feature_ipv6(), curl_sys::CURL_VERSION_IPV6),
        (version.feature_ssl(), curl_sys::CURL_VERSION_SSL),
        (version.feature_libz(), curl_sys::CURL_VERSION_LIBZ),
        (
            version.feature_async_dns(),
            curl_sys::CURL_VERSION_ASYNCHDNS,
        ),
        (
            version.feature_largefile(),
            curl_sys::CURL_VERSION_LARGEFILE,
        ),
        (version.feature_http2(), curl_sys::CURL_VERSION_HTTP2),
        (
            version.feature_https_proxy(),
            curl_sys::CURL_VERSION_HTTPS_PROXY,
        ),
        (version.feature_brotli(), curl_sys::CURL_VERSION_BROTLI),
        (version.feature_zstd(), curl_sys::CURL_VERSION_ZSTD),
    ] {
        if enabled {
            features |= i64::from(flag);
        }
    }
    let mut protocols = PhpArray::new();
    for protocol in version.protocols() {
        protocols.push(Value::string(protocol));
    }
    let mut result = PhpArray::with_hash_capacity(12);
    result.set_str(
        "version_number",
        Value::long(i64::from(version.version_num())),
    );
    result.set_str("age", Value::long(i64::from(curl_sys::CURLVERSION_NOW)));
    result.set_str("features", Value::long(features));
    result.set_str("ssl_version_number", Value::long(0));
    result.set_str("version", Value::string(version.version()));
    result.set_str("host", Value::string(version.host()));
    result.set_str(
        "ssl_version",
        version
            .ssl_version()
            .map_or_else(|| Value::string(""), Value::string),
    );
    result.set_str(
        "libz_version",
        version
            .libz_version()
            .map_or_else(|| Value::string(""), Value::string),
    );
    result.set_str("protocols", Value::array(protocols));
    write(rv, Value::array(result))
}

fn curl_multi_init(
    _ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    write(
        rv,
        native_object::<CurlMultiState>(eg, MULTI_CLASS).expect("registered CurlMultiHandle"),
    )
}

fn curl_multi_setopt(
    ed: *mut ExecuteData,
    rv: *mut Value,
    _eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let option = arg!(ed, 1).to_long_val();
    let value = arg!(ed, 2).to_long_val();
    let accepted = with_multi_state_mut(arg!(ed, 0), |state| {
        if option == i64::from(curl_sys::CURLMOPT_PIPELINING) {
            state.pipelining = value;
            true
        } else if option == i64::from(curl_sys::CURLMOPT_MAX_HOST_CONNECTIONS) {
            state.max_host_connections = value;
            true
        } else {
            false
        }
    })
    .unwrap_or(false);
    write(rv, Value::bool(accepted))
}

fn curl_multi_add_handle(
    ed: *mut ExecuteData,
    rv: *mut Value,
    _eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let handle = arg!(ed, 1).clone();
    let identity = handle.object_identity().unwrap_or_default();
    let code = with_multi_state_mut(arg!(ed, 0), |state| {
        if state
            .handles
            .iter()
            .any(|registered| registered.object_identity() == Some(identity))
        {
            state.errno = CURLM_ADDED_ALREADY;
            return state.errno;
        }
        state.completed.remove(&identity);
        state.handles.push(handle);
        state.errno = i64::from(curl_sys::CURLM_OK);
        state.errno
    })
    .unwrap_or(i64::from(curl_sys::CURLM_BAD_HANDLE));
    write(rv, Value::long(code))
}

fn curl_multi_remove_handle(
    ed: *mut ExecuteData,
    rv: *mut Value,
    _eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let identity = arg!(ed, 1).object_identity().unwrap_or_default();
    let code = with_multi_state_mut(arg!(ed, 0), |state| {
        if let Some(index) = state
            .handles
            .iter()
            .position(|registered| registered.object_identity() == Some(identity))
        {
            state.handles.remove(index);
            state.completed.remove(&identity);
            state.errno = i64::from(curl_sys::CURLM_OK);
        } else {
            state.errno = i64::from(curl_sys::CURLM_BAD_EASY_HANDLE);
        }
        state.errno
    })
    .unwrap_or(i64::from(curl_sys::CURLM_BAD_HANDLE));
    write(rv, Value::long(code))
}

fn curl_multi_exec(
    ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let handles = with_multi_state_mut(arg!(ed, 0), |state| {
        state
            .handles
            .iter()
            .filter(|handle| {
                handle
                    .object_identity()
                    .is_some_and(|identity| !state.completed.contains(&identity))
            })
            .cloned()
            .collect::<Vec<_>>()
    })
    .unwrap_or_default();
    let mut messages = Vec::with_capacity(handles.len());
    for handle in handles {
        let code = perform(&handle, eg, ed);
        messages.push((handle, code));
    }
    let active = with_multi_state_mut(arg!(ed, 0), |state| {
        for (handle, code) in messages {
            if let Some(identity) = handle.object_identity() {
                state.completed.insert(identity);
            }
            state.messages.push_back((handle, code));
        }
        state.errno = i64::from(curl_sys::CURLM_OK);
        state
            .handles
            .iter()
            .filter(|handle| {
                handle
                    .object_identity()
                    .is_none_or(|identity| !state.completed.contains(&identity))
            })
            .count() as i64
    })
    .unwrap_or_default();
    arg_mut!(ed, 1, Value::long(active));
    write(rv, Value::long(i64::from(curl_sys::CURLM_OK)))
}

fn curl_multi_select(
    _ed: *mut ExecuteData,
    rv: *mut Value,
    _eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    // curl_multi_exec() above has already driven every current transfer.  A
    // zero readiness count is the non-error result Composer expects here.
    write(rv, Value::long(0))
}

fn curl_multi_info_read(
    ed: *mut ExecuteData,
    rv: *mut Value,
    _eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let message = with_multi_state_mut(arg!(ed, 0), |state| {
        let message = state.messages.pop_front();
        (message, state.messages.len())
    });
    let Some((message, remaining)) = message else {
        return write(rv, Value::bool(false));
    };
    if arg_opt!(ed, 1).is_some() {
        arg_mut!(ed, 1, Value::long(remaining as i64));
    }
    let Some((handle, code)) = message else {
        return write(rv, Value::bool(false));
    };
    let mut result = PhpArray::with_hash_capacity(3);
    result.set_str("msg", Value::long(i64::from(curl_sys::CURLMSG_DONE)));
    result.set_str("result", Value::long(code));
    result.set_str("handle", handle);
    write(rv, Value::array(result))
}

fn curl_share_init(
    _ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    write(
        rv,
        native_object::<CurlShareState>(eg, SHARE_CLASS).expect("registered CurlShareHandle"),
    )
}

fn curl_share_setopt(
    ed: *mut ExecuteData,
    rv: *mut Value,
    _eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let option = arg!(ed, 1).to_long_val();
    let value = arg!(ed, 2).to_long_val();
    let accepted = with_share_state_mut(arg!(ed, 0), |state| {
        if option != i64::from(curl_sys::CURLSHOPT_SHARE) {
            state.errno = i64::from(curl_sys::CURLSHE_BAD_OPTION);
            return false;
        }
        if !state.shared.contains(&value) {
            state.shared.push(value);
        }
        state.errno = i64::from(curl_sys::CURLSHE_OK);
        true
    })
    .unwrap_or(false);
    write(rv, Value::bool(accepted))
}
