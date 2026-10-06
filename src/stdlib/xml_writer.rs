//! PHP 8.5 XMLWriter extension.
//!
//! The serializer is deliberately implemented in RPHP.  It does not delegate
//! XML construction to the host libxml installation, so byte output and
//! lifetime rules remain deterministic across supported hosts.  Procedural
//! functions and the `XMLWriter` methods share the same native object state.

use std::cell::RefCell;
use std::fs::File;
use std::io::Write as _;
use std::rc::Rc;

use crate::compiler::compile::ClassDef;
use crate::compiler::{make_internal_function, make_internal_method};
use crate::runtime::ExecutorGlobals;
use crate::value::{NativeObjectState, ObjectLayout, PhpObject, Value, ValueType};
use crate::vm::execute::VmError;
use crate::vm::frame::ExecuteData;
use crate::vm::function::{
    FunctionCommon, InternalFunction, InternalFunctionHandler, ParamTypeHint,
};

const XML_WRITER: &str = "XMLWriter";

#[derive(Clone)]
enum Target {
    Uninitialized,
    Memory,
    Uri(Rc<RefCell<File>>),
    Stream(Value),
}

impl Default for Target {
    fn default() -> Self {
        Self::Uninitialized
    }
}

#[derive(Clone)]
struct Element {
    name: Vec<u8>,
    start_open: bool,
    has_child: bool,
    has_text: bool,
    namespace: Option<(Option<Vec<u8>>, Vec<u8>)>,
    namespaces: Vec<(Option<Vec<u8>>, Vec<u8>)>,
}

#[derive(Clone)]
enum Section {
    Attribute,
    Cdata,
    Comment,
    ProcessingInstruction { has_text: bool },
    Dtd,
    DtdAttlist,
    DtdElement,
    DtdEntity,
}

#[derive(Clone, Default)]
struct State {
    target: Target,
    buffer: Vec<u8>,
    elements: Vec<Element>,
    section: Option<Section>,
    indent: bool,
    indent_string: Vec<u8>,
    document_started: bool,
    wrote_root: bool,
    inside_dtd: bool,
    dtd_has_subset: bool,
    attribute_namespace: Option<(Option<Vec<u8>>, Vec<u8>)>,
    encoding: Option<Vec<u8>>,
    stream_since_flush: usize,
    stream_failed: bool,
    stream_flush_ready: bool,
    suppress_next_top_level_newline: bool,
}

impl Drop for State {
    fn drop(&mut self) {
        if self.buffer.is_empty() {
            return;
        }
        if let Target::Uri(file) = &self.target {
            if let Ok(mut file) = file.try_borrow_mut() {
                let _ = file.write_all(&self.buffer);
                let _ = file.flush();
                self.buffer.clear();
            }
        }
    }
}

impl NativeObjectState for State {
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
        if let Target::Stream(stream) = &self.target {
            visit(stream);
        }
    }

    fn append_values_reversed(&mut self, pending: &mut Vec<Value>) {
        if let Target::Stream(stream) = std::mem::take(&mut self.target) {
            pending.push(stream);
        }
    }
}

impl State {
    fn initialized(&self) -> bool {
        !matches!(self.target, Target::Uninitialized)
    }

    fn reset(&mut self, target: Target) {
        self.target = target;
        self.buffer.clear();
        self.elements.clear();
        self.section = None;
        self.indent = false;
        self.indent_string = b" ".to_vec();
        self.document_started = false;
        self.wrote_root = false;
        self.inside_dtd = false;
        self.dtd_has_subset = false;
        self.attribute_namespace = None;
        self.encoding = None;
        self.stream_since_flush = 0;
        self.stream_failed = false;
        self.stream_flush_ready = false;
        self.suppress_next_top_level_newline = false;
    }

    fn indent_bytes(&mut self, depth: usize) {
        if !self.indent {
            return;
        }
        self.buffer.push(b'\n');
        for _ in 0..depth {
            self.buffer.extend_from_slice(&self.indent_string);
        }
    }

    fn finish_open_start(&mut self) {
        if self
            .elements
            .last()
            .is_some_and(|element| element.start_open)
        {
            self.write_pending_element_namespace();
            self.buffer.push(b'>');
            self.elements.last_mut().unwrap().start_open = false;
        }
    }

    fn write_pending_element_namespace(&mut self) {
        let namespace = self
            .elements
            .last_mut()
            .and_then(|element| element.namespace.take());
        let Some((prefix, namespace)) = namespace else {
            return;
        };
        self.buffer.extend_from_slice(b" xmlns");
        if let Some(prefix) = prefix.filter(|prefix| !prefix.is_empty()) {
            self.buffer.push(b':');
            self.buffer.extend_from_slice(&prefix);
        }
        self.buffer.extend_from_slice(b"=\"");
        escape_attribute(&mut self.buffer, &namespace);
        self.buffer.push(b'"');
    }

    fn before_node(&mut self) -> bool {
        if self.section.is_some() {
            return false;
        }
        self.finish_open_start();
        let depth = self.elements.len();
        let parent_allows_indent = self.elements.last().is_some_and(|parent| !parent.has_text);
        if depth == 0 {
            if self.document_started
                && !self.buffer.ends_with(b"\n")
                && !self.suppress_next_top_level_newline
            {
                self.buffer.push(b'\n');
            }
            self.suppress_next_top_level_newline = false;
        } else {
            if self.indent && parent_allows_indent {
                self.indent_bytes(depth);
            }
            if let Some(parent) = self.elements.last_mut() {
                parent.has_child = true;
            }
        }
        true
    }

    fn start_element(&mut self, name: &[u8]) -> bool {
        if name.is_empty() || !self.before_node() {
            return false;
        }
        self.buffer.push(b'<');
        self.buffer.extend_from_slice(name);
        self.elements.push(Element {
            name: name.to_vec(),
            start_open: true,
            has_child: false,
            has_text: false,
            namespace: None,
            namespaces: Vec::new(),
        });
        if self.elements.len() == 1 {
            self.wrote_root = true;
        }
        true
    }

    fn end_element(&mut self, full: bool) -> bool {
        if self.section.is_some() {
            return false;
        }
        let Some(element) = self.elements.pop() else {
            return false;
        };
        if element.start_open {
            if let Some((prefix, namespace)) = element.namespace {
                self.buffer.extend_from_slice(b" xmlns");
                if let Some(prefix) = prefix.filter(|prefix| !prefix.is_empty()) {
                    self.buffer.push(b':');
                    self.buffer.extend_from_slice(&prefix);
                }
                self.buffer.extend_from_slice(b"=\"");
                escape_attribute(&mut self.buffer, &namespace);
                self.buffer.push(b'"');
            }
            if full {
                self.buffer.push(b'>');
                self.buffer.extend_from_slice(b"</");
                self.buffer.extend_from_slice(&element.name);
                self.buffer.push(b'>');
            } else {
                self.buffer.extend_from_slice(b"/>");
            }
        } else {
            if self.indent && element.has_child && !element.has_text {
                self.indent_bytes(self.elements.len());
            }
            self.buffer.extend_from_slice(b"</");
            self.buffer.extend_from_slice(&element.name);
            self.buffer.push(b'>');
        }
        if self.elements.is_empty() {
            self.stream_flush_ready = true;
        }
        true
    }

    fn end_document(&mut self) -> bool {
        if self.section.is_some() {
            return false;
        }
        while !self.elements.is_empty() {
            self.end_element(false);
        }
        if self.wrote_root && !self.buffer.ends_with(b"\n") {
            self.buffer.push(b'\n');
        }
        self.stream_flush_ready = true;
        true
    }

    fn write_text(&mut self, text: &[u8]) -> bool {
        match self.section.as_mut() {
            Some(Section::Attribute) => escape_attribute(&mut self.buffer, text),
            Some(Section::Cdata)
            | Some(Section::Comment)
            | Some(Section::Dtd)
            | Some(Section::DtdAttlist)
            | Some(Section::DtdElement)
            | Some(Section::DtdEntity) => self.buffer.extend_from_slice(text),
            Some(Section::ProcessingInstruction { has_text }) => {
                if !*has_text && !text.is_empty() {
                    self.buffer.push(b' ');
                }
                self.buffer.extend_from_slice(text);
                *has_text = true;
            }
            None => {
                if self.elements.is_empty() {
                    return false;
                }
                self.finish_open_start();
                escape_text(&mut self.buffer, text);
                self.elements.last_mut().unwrap().has_text = true;
            }
        }
        true
    }

    fn write_raw(&mut self, text: &[u8]) -> bool {
        if self.section.is_some() || self.elements.is_empty() {
            return false;
        }
        self.finish_open_start();
        self.buffer.extend_from_slice(text);
        self.elements.last_mut().unwrap().has_text = true;
        true
    }

    fn start_attribute(&mut self, name: &[u8]) -> bool {
        if name.is_empty()
            || self.section.is_some()
            || !self
                .elements
                .last()
                .is_some_and(|element| element.start_open)
        {
            return false;
        }
        self.buffer.push(b' ');
        self.buffer.extend_from_slice(name);
        self.buffer.extend_from_slice(b"=\"");
        self.section = Some(Section::Attribute);
        self.attribute_namespace = None;
        true
    }

    fn end_attribute(&mut self) -> bool {
        if !matches!(self.section, Some(Section::Attribute)) {
            return false;
        }
        self.buffer.push(b'"');
        self.section = None;
        if let Some((prefix, namespace)) = self.attribute_namespace.take() {
            self.buffer.extend_from_slice(b" xmlns");
            if let Some(prefix) = prefix.filter(|prefix| !prefix.is_empty()) {
                self.buffer.push(b':');
                self.buffer.extend_from_slice(&prefix);
            }
            self.buffer.extend_from_slice(b"=\"");
            escape_attribute(&mut self.buffer, &namespace);
            self.buffer.push(b'"');
        }
        true
    }

    fn start_cdata(&mut self) -> bool {
        if self.section.is_some() || self.elements.is_empty() {
            return false;
        }
        self.finish_open_start();
        self.buffer.extend_from_slice(b"<![CDATA[");
        self.elements.last_mut().unwrap().has_text = true;
        self.section = Some(Section::Cdata);
        true
    }

    fn end_cdata(&mut self) -> bool {
        if !matches!(self.section, Some(Section::Cdata)) {
            return false;
        }
        self.buffer.extend_from_slice(b"]]>");
        self.section = None;
        true
    }

    fn start_comment(&mut self) -> bool {
        if !self.before_node() {
            return false;
        }
        self.buffer.extend_from_slice(b"<!--");
        self.section = Some(Section::Comment);
        true
    }

    fn end_comment(&mut self) -> bool {
        if !matches!(self.section, Some(Section::Comment)) {
            return false;
        }
        self.buffer.extend_from_slice(b"-->");
        self.section = None;
        if self.elements.is_empty() {
            self.stream_flush_ready = true;
        }
        true
    }

    fn start_pi(&mut self, target: &[u8]) -> bool {
        if target.is_empty() || self.section.is_some() {
            return false;
        }
        if self.elements.is_empty() {
            if !self.before_node() {
                return false;
            }
        } else {
            self.finish_open_start();
            self.elements.last_mut().unwrap().has_text = true;
        }
        self.buffer.extend_from_slice(b"<?");
        self.buffer.extend_from_slice(target);
        self.section = Some(Section::ProcessingInstruction { has_text: false });
        true
    }

    fn end_pi(&mut self) -> bool {
        if !matches!(self.section, Some(Section::ProcessingInstruction { .. })) {
            return false;
        }
        self.buffer.extend_from_slice(b"?>");
        self.section = None;
        if self.elements.is_empty() {
            self.stream_flush_ready = true;
        }
        true
    }

    fn start_dtd(&mut self, name: &[u8], public: Option<&[u8]>, system: Option<&[u8]>) -> bool {
        if name.is_empty() || !self.before_node() {
            return false;
        }
        self.buffer.extend_from_slice(b"<!DOCTYPE ");
        self.buffer.extend_from_slice(name);
        if let Some(public) = public {
            self.buffer.extend_from_slice(b" PUBLIC \"");
            self.buffer.extend_from_slice(public);
            self.buffer.push(b'"');
            if let Some(system) = system {
                self.buffer.extend_from_slice(b" \"");
                self.buffer.extend_from_slice(system);
                self.buffer.push(b'"');
            }
        } else if let Some(system) = system {
            self.buffer.extend_from_slice(b" SYSTEM \"");
            self.buffer.extend_from_slice(system);
            self.buffer.push(b'"');
        }
        self.section = Some(Section::Dtd);
        self.inside_dtd = true;
        self.dtd_has_subset = false;
        true
    }

    fn end_dtd(&mut self) -> bool {
        if !matches!(self.section, Some(Section::Dtd)) {
            return false;
        }
        self.buffer
            .extend_from_slice(if self.dtd_has_subset { b"]>" } else { b">" });
        self.section = None;
        self.inside_dtd = false;
        self.dtd_has_subset = false;
        self.suppress_next_top_level_newline = true;
        self.stream_flush_ready = true;
        true
    }

    fn ensure_dtd_subset(&mut self) {
        if self.inside_dtd && !self.dtd_has_subset {
            self.buffer.extend_from_slice(b" [");
            self.dtd_has_subset = true;
        }
    }

    fn start_dtd_declaration(&mut self, prefix: &[u8], name: &[u8], section: Section) -> bool {
        if name.is_empty() {
            return false;
        }
        if !matches!(self.section, Some(Section::Dtd)) {
            if self.section.is_some() || !self.before_node() {
                return false;
            }
        } else {
            self.ensure_dtd_subset();
        }
        self.buffer.extend_from_slice(prefix);
        self.buffer.extend_from_slice(name);
        self.buffer.push(b' ');
        self.section = Some(section);
        true
    }

    fn end_dtd_declaration(&mut self, expected: fn(&Section) -> bool) -> bool {
        if !self.section.as_ref().is_some_and(expected) {
            return false;
        }
        self.buffer.push(b'>');
        self.section = self.inside_dtd.then_some(Section::Dtd);
        true
    }
}

fn escape_text(output: &mut Vec<u8>, text: &[u8]) {
    for &byte in text {
        match byte {
            b'&' => output.extend_from_slice(b"&amp;"),
            b'<' => output.extend_from_slice(b"&lt;"),
            b'>' => output.extend_from_slice(b"&gt;"),
            b'"' => output.extend_from_slice(b"&quot;"),
            _ => output.push(byte),
        }
    }
}

fn escape_attribute(output: &mut Vec<u8>, text: &[u8]) {
    for &byte in text {
        match byte {
            b'&' => output.extend_from_slice(b"&amp;"),
            b'<' => output.extend_from_slice(b"&lt;"),
            b'>' => output.extend_from_slice(b"&gt;"),
            b'"' => output.extend_from_slice(b"&quot;"),
            _ => output.push(byte),
        }
    }
}

fn bytes(value: &Value) -> Vec<u8> {
    value
        .dereferenced()
        .php_string_bytes()
        .map_or_else(Vec::new, |bytes| bytes.into_owned())
}

fn optional_bytes(ed: *mut ExecuteData, index: u32) -> Option<Vec<u8>> {
    let value = super::owned_argument(ed, index);
    (!matches!(
        value.dereferenced().value_type(),
        ValueType::Undef | ValueType::Null
    ))
    .then(|| bytes(&value))
}

fn receiver(ed: *mut ExecuteData) -> Value {
    super::owned_argument(ed, 0)
}

fn uninitialized(eg: &mut ExecutorGlobals) {
    eg.exception = Some(crate::value::make_error_value(
        "Error",
        "Invalid or uninitialized XMLWriter object",
    ));
}

fn with_state_mut<R>(
    ed: *mut ExecuteData,
    eg: &mut ExecutorGlobals,
    operation: impl FnOnce(&mut State) -> R,
) -> Option<R> {
    let writer = receiver(ed);
    let mut object = writer.as_object_mut()?;
    let state = object.native_object_state_mut::<State>();
    if !state.initialized() {
        drop(object);
        uninitialized(eg);
        return None;
    }
    Some(operation(state))
}

fn initialize_receiver(ed: *mut ExecuteData, target: Target) -> bool {
    let writer = receiver(ed);
    let Some(mut object) = writer.as_object_mut() else {
        return false;
    };
    object.native_object_state_mut::<State>().reset(target);
    true
}

fn new_writer(eg: &ExecutorGlobals, target: Target) -> Value {
    let class = eg
        .find_class(XML_WRITER)
        .expect("XMLWriter is registered before factories execute");
    let mut object = PhpObject::with_layout(
        class.class_id,
        Rc::clone(&class.property_layout),
        class.property_defaults.to_vec(),
    );
    object.native_object_state_mut::<State>().reset(target);
    Value::object(object)
}

fn new_static_writer(
    ed: *mut ExecuteData,
    eg: &mut ExecutorGlobals,
    target: Target,
) -> Result<Option<Value>, VmError> {
    let class_name = crate::vm::execute::called_class_name_for_internal_call(eg, ed)
        .unwrap_or(XML_WRITER)
        .to_string();
    let class_id = eg
        .find_class(&class_name)
        .expect("called XMLWriter class remains registered")
        .class_id;
    let Some(writer) = crate::vm::execute::instantiate_protocol_object(eg, ed, class_id)? else {
        return Ok(None);
    };
    let _ = super::call_object_public_method(eg, &writer, "__construct", &[])?;
    if eg.exception.is_some() {
        return Ok(None);
    }
    let Some(mut object) = writer.as_object_mut() else {
        return Ok(None);
    };
    object.native_object_state_mut::<State>().reset(target);
    drop(object);
    Ok(Some(writer))
}

fn bool_result(
    ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
    operation: impl FnOnce(&mut State) -> bool,
) -> Result<(), VmError> {
    let writer = receiver(ed);
    let Some((result, flush_stream)) = with_state_mut(ed, eg, |state| {
        let result = operation(state);
        let flush_stream =
            result && matches!(state.target, Target::Stream(_)) && state.stream_flush_ready;
        (result, flush_stream)
    }) else {
        return Ok(());
    };
    if flush_stream {
        flush_stream_incremental(ed, eg, &writer)?;
    }
    super::write_return_value(rv, Value::bool(result));
    Ok(())
}

fn encoded_output(encoding: Option<&[u8]>, bytes: Vec<u8>) -> Vec<u8> {
    let Some(encoding) = encoding.filter(|encoding| !encoding.eq_ignore_ascii_case(b"UTF-8"))
    else {
        return bytes;
    };
    #[cfg(target_os = "linux")]
    {
        super::native_process::convert_encoding(b"UTF-8", encoding, &bytes).unwrap_or(bytes)
    }
    #[cfg(not(target_os = "linux"))]
    {
        let _ = encoding;
        bytes
    }
}

fn flush_stream_incremental(
    ed: *mut ExecuteData,
    eg: &mut ExecutorGlobals,
    writer: &Value,
) -> Result<(), VmError> {
    let pending = {
        let Some(mut object) = writer.as_object_mut() else {
            return Ok(());
        };
        let state = object.native_object_state_mut::<State>();
        let Target::Stream(stream) = &state.target else {
            return Ok(());
        };
        let Some(id) = stream.as_resource_id() else {
            state.stream_failed = true;
            return Ok(());
        };
        if state.document_started && !state.buffer.is_empty() && !state.buffer.ends_with(b"\n") {
            state.buffer.push(b'\n');
        }
        let bytes = encoded_output(state.encoding.as_deref(), std::mem::take(&mut state.buffer));
        state.stream_flush_ready = false;
        (id, bytes)
    };
    if pending.1.is_empty() {
        return Ok(());
    }
    let written =
        super::streams::write_stream_bytes(eg, ed, pending.0, &pending.1)?.and_then(Result::ok);
    if let Some(mut object) = writer.as_object_mut() {
        let state = object.native_object_state_mut::<State>();
        match written {
            Some(written) if written == pending.1.len() => {
                state.stream_since_flush += written;
            }
            _ => state.stream_failed = true,
        }
    }
    Ok(())
}

fn fn_open_memory(
    ed: *mut ExecuteData,
    rv: *mut Value,
    _eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    super::write_return_value(rv, Value::bool(initialize_receiver(ed, Target::Memory)));
    Ok(())
}

fn fn_xmlwriter_open_memory(
    _ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    super::write_return_value(rv, new_writer(eg, Target::Memory));
    Ok(())
}

fn uri_target(path: &[u8]) -> Result<Target, ()> {
    if path.eq_ignore_ascii_case(b"php://memory") {
        return Ok(Target::Memory);
    }
    let path = if let Some(path) = path.strip_prefix(b"file://") {
        if !path.starts_with(b"/") {
            return Err(());
        }
        path
    } else if path.starts_with(b"php://") || path.contains(&0) {
        return Err(());
    } else {
        path
    };
    File::create(String::from_utf8_lossy(path).as_ref())
        .map(|file| Target::Uri(Rc::new(RefCell::new(file))))
        .map_err(|_| ())
}

fn current_function_name(eg: &ExecutorGlobals, ed: *mut ExecuteData) -> String {
    super::with_internal_frame_and_raw_argument(ed, 0, |frame, _| {
        crate::vm::execute::displayed_function_name(eg, frame.func)
    })
}

fn valid_xml_name(name: &[u8]) -> bool {
    let Some((&first, rest)) = name.split_first() else {
        return false;
    };
    let first_valid = first.is_ascii_alphabetic() || matches!(first, b'_' | b':') || first >= 0x80;
    first_valid
        && rest.iter().all(|byte| {
            byte.is_ascii_alphanumeric()
                || matches!(byte, b'_' | b':' | b'-' | b'.')
                || *byte >= 0x80
        })
}

fn validate_xml_name(
    eg: &mut ExecutorGlobals,
    ed: *mut ExecuteData,
    argument: usize,
    parameter: &str,
    subject: &str,
    name: &[u8],
) -> bool {
    if valid_xml_name(name) {
        return true;
    }
    eg.exception = Some(crate::value::make_error_value(
        "ValueError",
        &format!(
            "{}(): Argument #{argument} (${parameter}) must be a valid {subject}, \"{}\" given",
            current_function_name(eg, ed),
            String::from_utf8_lossy(name)
        ),
    ));
    false
}

fn reject_empty_uri(eg: &mut ExecutorGlobals, ed: *mut ExecuteData, path: &[u8]) -> bool {
    if !path.is_empty() {
        return false;
    }
    let function = current_function_name(eg, ed);
    eg.exception = Some(crate::value::make_error_value(
        "ValueError",
        &format!("{function}(): Argument #1 ($uri) must not be empty"),
    ));
    true
}

fn fn_open_uri(
    ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let path = bytes(&super::owned_argument(ed, 1));
    if reject_empty_uri(eg, ed, &path) {
        return Ok(());
    }
    let success = uri_target(&path).is_ok_and(|target| initialize_receiver(ed, target));
    if !success {
        super::report_internal_diagnostic(
            eg,
            ed,
            2,
            "Warning",
            &format!(
                "{}(): Unable to resolve file path",
                current_function_name(eg, ed)
            ),
        )?;
    }
    super::write_return_value(rv, Value::bool(success));
    Ok(())
}

fn fn_xmlwriter_open_uri(
    ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let path = bytes(&super::owned_argument(ed, 0));
    if reject_empty_uri(eg, ed, &path) {
        return Ok(());
    }
    let Ok(target) = uri_target(&path) else {
        super::report_internal_diagnostic(
            eg,
            ed,
            2,
            "Warning",
            "xmlwriter_open_uri(): Unable to resolve file path",
        )?;
        super::write_return_value(rv, Value::bool(false));
        return Ok(());
    };
    super::write_return_value(rv, new_writer(eg, target));
    Ok(())
}

fn fn_to_memory(
    ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    if let Some(writer) = new_static_writer(ed, eg, Target::Memory)? {
        super::write_return_value(rv, writer);
    }
    Ok(())
}

fn fn_to_uri(
    ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let path = bytes(&super::owned_argument(ed, 1));
    if reject_empty_uri(eg, ed, &path) {
        return Ok(());
    }
    let Ok(target) = uri_target(&path) else {
        eg.exception = Some(crate::value::make_error_value(
            "ValueError",
            "XMLWriter::toUri(): Argument #1 ($uri) must resolve to a valid file path",
        ));
        return Ok(());
    };
    if let Some(writer) = new_static_writer(ed, eg, target)? {
        super::write_return_value(rv, writer);
    }
    Ok(())
}

fn fn_to_stream(
    ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let stream = super::owned_argument(ed, 1);
    let Some(resource) = stream.as_resource_id() else {
        eg.exception = Some(crate::value::make_error_value(
            "TypeError",
            &format!(
                "XMLWriter::toStream(): Argument #1 ($stream) must be a valid stream resource, {} given",
                stream.dereferenced().diagnostic_type_name()
            ),
        ));
        return Ok(());
    };
    if super::streams::with_stream(eg, resource, |_| ()).is_none() {
        eg.exception = Some(crate::value::make_error_value(
            "TypeError",
            "XMLWriter::toStream(): supplied resource is not a valid stream resource",
        ));
        return Ok(());
    }
    if let Some(writer) = new_static_writer(ed, eg, Target::Stream(stream))? {
        super::write_return_value(rv, writer);
    }
    Ok(())
}

fn fn_set_indent(
    ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let enabled = super::owned_argument(ed, 1).is_truthy();
    bool_result(ed, rv, eg, |state| {
        state.indent = enabled;
        true
    })
}

fn fn_set_indent_string(
    ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let indentation = bytes(&super::owned_argument(ed, 1));
    bool_result(ed, rv, eg, |state| {
        state.indent_string = indentation;
        true
    })
}

fn fn_start_document(
    ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let version = optional_bytes(ed, 1).unwrap_or_else(|| b"1.0".to_vec());
    let encoding = optional_bytes(ed, 2);
    let standalone = optional_bytes(ed, 3);
    bool_result(ed, rv, eg, |state| {
        if state.section.is_some() || !state.elements.is_empty() || !state.buffer.is_empty() {
            return false;
        }
        state.buffer.extend_from_slice(b"<?xml version=\"");
        state.buffer.extend_from_slice(&version);
        state.buffer.push(b'"');
        if let Some(encoding) = encoding {
            state.encoding = Some(encoding.clone());
            state.buffer.extend_from_slice(b" encoding=\"");
            state.buffer.extend_from_slice(&encoding);
            state.buffer.push(b'"');
        }
        if let Some(standalone) = standalone {
            state.buffer.extend_from_slice(b" standalone=\"");
            state.buffer.extend_from_slice(&standalone);
            state.buffer.push(b'"');
        }
        state.buffer.extend_from_slice(b"?>");
        state.document_started = true;
        true
    })
}

fn fn_end_document(
    ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    bool_result(ed, rv, eg, State::end_document)
}

fn fn_start_element(
    ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let name = bytes(&super::owned_argument(ed, 1));
    if !validate_xml_name(eg, ed, 2, "name", "element name", &name) {
        return Ok(());
    }
    bool_result(ed, rv, eg, |state| state.start_element(&name))
}

fn qualified_name(prefix: Option<&[u8]>, name: &[u8]) -> Vec<u8> {
    let mut qualified = Vec::new();
    if let Some(prefix) = prefix.filter(|prefix| !prefix.is_empty()) {
        qualified.extend_from_slice(prefix);
        qualified.push(b':');
    }
    qualified.extend_from_slice(name);
    qualified
}

fn namespace_is_in_scope(state: &State, prefix: Option<&[u8]>, namespace: &[u8]) -> bool {
    state.elements.iter().rev().any(|element| {
        element
            .namespaces
            .iter()
            .rev()
            .any(|(declared_prefix, declared_namespace)| {
                declared_prefix.as_deref() == prefix && declared_namespace == namespace
            })
    })
}

fn fn_start_element_ns(
    ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let prefix = optional_bytes(ed, 1);
    let name = bytes(&super::owned_argument(ed, 2));
    let namespace = optional_bytes(ed, 3);
    if !validate_xml_name(eg, ed, 3, "name", "element name", &name) {
        return Ok(());
    }
    let qualified = qualified_name(prefix.as_deref(), &name);
    bool_result(ed, rv, eg, |state| {
        if !state.start_element(&qualified) {
            return false;
        }
        if let Some(namespace) = namespace {
            state.elements.last_mut().unwrap().namespace =
                Some((prefix.clone(), namespace.clone()));
            state
                .elements
                .last_mut()
                .unwrap()
                .namespaces
                .push((prefix, namespace));
        }
        true
    })
}

fn fn_end_element(
    ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    bool_result(ed, rv, eg, |state| state.end_element(false))
}

fn fn_full_end_element(
    ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    bool_result(ed, rv, eg, |state| state.end_element(true))
}

fn fn_write_element(
    ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let name = bytes(&super::owned_argument(ed, 1));
    let content = optional_bytes(ed, 2);
    if !validate_xml_name(eg, ed, 2, "name", "element name", &name) {
        return Ok(());
    }
    bool_result(ed, rv, eg, |state| {
        if !state.start_element(&name) {
            return false;
        }
        if let Some(content) = &content
            && !state.write_text(content)
        {
            return false;
        }
        state.end_element(content.is_some())
    })
}

fn fn_write_element_ns(
    ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let prefix = optional_bytes(ed, 1);
    let name = bytes(&super::owned_argument(ed, 2));
    let namespace = optional_bytes(ed, 3);
    let content = optional_bytes(ed, 4);
    if !validate_xml_name(eg, ed, 3, "name", "element name", &name) {
        return Ok(());
    }
    let qualified = qualified_name(prefix.as_deref(), &name);
    bool_result(ed, rv, eg, |state| {
        if !state.start_element(&qualified) {
            return false;
        }
        if let Some(namespace) = namespace {
            state.elements.last_mut().unwrap().namespace =
                Some((prefix.clone(), namespace.clone()));
            state
                .elements
                .last_mut()
                .unwrap()
                .namespaces
                .push((prefix.clone(), namespace));
        }
        if let Some(content) = &content
            && !state.write_text(content)
        {
            return false;
        }
        state.end_element(content.is_some())
    })
}

fn fn_start_attribute(
    ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let name = bytes(&super::owned_argument(ed, 1));
    if !validate_xml_name(eg, ed, 2, "name", "attribute name", &name) {
        return Ok(());
    }
    bool_result(ed, rv, eg, |state| state.start_attribute(&name))
}

fn fn_start_attribute_ns(
    ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let prefix = optional_bytes(ed, 1);
    let name = bytes(&super::owned_argument(ed, 2));
    let namespace = optional_bytes(ed, 3);
    if !validate_xml_name(eg, ed, 3, "name", "attribute name", &name) {
        return Ok(());
    }
    let qualified = qualified_name(prefix.as_deref(), &name);
    bool_result(ed, rv, eg, |state| {
        if !state.start_attribute(&qualified) {
            return false;
        }
        if let Some(namespace) = namespace
            && !namespace_is_in_scope(state, prefix.as_deref(), &namespace)
        {
            state.attribute_namespace = Some((prefix.clone(), namespace.clone()));
            state
                .elements
                .last_mut()
                .unwrap()
                .namespaces
                .push((prefix, namespace));
        }
        true
    })
}

fn fn_end_attribute(
    ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    bool_result(ed, rv, eg, State::end_attribute)
}

fn fn_write_attribute(
    ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let name = bytes(&super::owned_argument(ed, 1));
    let value = bytes(&super::owned_argument(ed, 2));
    if !validate_xml_name(eg, ed, 2, "name", "attribute name", &name) {
        return Ok(());
    }
    bool_result(ed, rv, eg, |state| {
        state.start_attribute(&name) && state.write_text(&value) && state.end_attribute()
    })
}

fn fn_write_attribute_ns(
    ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let prefix = optional_bytes(ed, 1);
    let name = bytes(&super::owned_argument(ed, 2));
    let namespace = optional_bytes(ed, 3);
    let value = bytes(&super::owned_argument(ed, 4));
    if !validate_xml_name(eg, ed, 3, "name", "attribute name", &name) {
        return Ok(());
    }
    let qualified = qualified_name(prefix.as_deref(), &name);
    bool_result(ed, rv, eg, |state| {
        if !state.start_attribute(&qualified) {
            return false;
        }
        if let Some(namespace) = namespace
            && !namespace_is_in_scope(state, prefix.as_deref(), &namespace)
        {
            state.attribute_namespace = Some((prefix.clone(), namespace.clone()));
            state
                .elements
                .last_mut()
                .unwrap()
                .namespaces
                .push((prefix, namespace));
        }
        if !state.write_text(&value) || !state.end_attribute() {
            return false;
        }
        true
    })
}

fn fn_text(ed: *mut ExecuteData, rv: *mut Value, eg: &mut ExecutorGlobals) -> Result<(), VmError> {
    let content = bytes(&super::owned_argument(ed, 1));
    bool_result(ed, rv, eg, |state| state.write_text(&content))
}

fn fn_write_raw(
    ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let content = bytes(&super::owned_argument(ed, 1));
    bool_result(ed, rv, eg, |state| state.write_raw(&content))
}

fn fn_start_cdata(
    ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    bool_result(ed, rv, eg, State::start_cdata)
}

fn fn_end_cdata(
    ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    bool_result(ed, rv, eg, State::end_cdata)
}

fn fn_write_cdata(
    ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let content = bytes(&super::owned_argument(ed, 1));
    bool_result(ed, rv, eg, |state| {
        state.start_cdata() && state.write_text(&content) && state.end_cdata()
    })
}

fn fn_start_comment(
    ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    bool_result(ed, rv, eg, State::start_comment)
}

fn fn_end_comment(
    ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    bool_result(ed, rv, eg, State::end_comment)
}

fn fn_write_comment(
    ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let content = bytes(&super::owned_argument(ed, 1));
    bool_result(ed, rv, eg, |state| {
        state.start_comment() && state.write_text(&content) && state.end_comment()
    })
}

fn fn_start_pi(
    ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let target = bytes(&super::owned_argument(ed, 1));
    if !validate_xml_name(eg, ed, 2, "target", "PI target", &target) {
        return Ok(());
    }
    bool_result(ed, rv, eg, |state| state.start_pi(&target))
}

fn fn_end_pi(
    ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    bool_result(ed, rv, eg, State::end_pi)
}

fn fn_write_pi(
    ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let target = bytes(&super::owned_argument(ed, 1));
    let content = bytes(&super::owned_argument(ed, 2));
    if !validate_xml_name(eg, ed, 2, "target", "PI target", &target) {
        return Ok(());
    }
    bool_result(ed, rv, eg, |state| {
        state.start_pi(&target) && state.write_text(&content) && state.end_pi()
    })
}

fn fn_start_dtd(
    ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let name = bytes(&super::owned_argument(ed, 1));
    let public = optional_bytes(ed, 2);
    let system = optional_bytes(ed, 3);
    bool_result(ed, rv, eg, |state| {
        state.start_dtd(&name, public.as_deref(), system.as_deref())
    })
}

fn fn_end_dtd(
    ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    bool_result(ed, rv, eg, State::end_dtd)
}

fn fn_write_dtd(
    ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let name = bytes(&super::owned_argument(ed, 1));
    let public = optional_bytes(ed, 2);
    let system = optional_bytes(ed, 3);
    let subset = optional_bytes(ed, 4);
    bool_result(ed, rv, eg, |state| {
        if name.is_empty() || !state.before_node() {
            return false;
        }
        state.buffer.extend_from_slice(b"<!DOCTYPE ");
        state.buffer.extend_from_slice(&name);
        if let Some(public) = &public {
            state.buffer.extend_from_slice(b" PUBLIC \"");
            state.buffer.extend_from_slice(public);
            state.buffer.push(b'"');
            if let Some(system) = &system {
                state.buffer.extend_from_slice(b" \"");
                state.buffer.extend_from_slice(system);
                state.buffer.push(b'"');
            }
        } else if let Some(system) = &system {
            state.buffer.extend_from_slice(b" SYSTEM \"");
            state.buffer.extend_from_slice(system);
            state.buffer.push(b'"');
        }
        if let Some(subset) = &subset {
            state.buffer.extend_from_slice(b" [");
            state.buffer.extend_from_slice(subset);
            state.buffer.push(b']');
        }
        state.buffer.push(b'>');
        state.stream_flush_ready = true;
        true
    })
}

fn fn_start_dtd_attlist(
    ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let name = bytes(&super::owned_argument(ed, 1));
    if !validate_xml_name(eg, ed, 2, "name", "element name", &name) {
        return Ok(());
    }
    bool_result(ed, rv, eg, |state| {
        state.start_dtd_declaration(b"<!ATTLIST ", &name, Section::DtdAttlist)
    })
}

fn fn_end_dtd_attlist(
    ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    bool_result(ed, rv, eg, |state| {
        state.end_dtd_declaration(|section| matches!(section, Section::DtdAttlist))
    })
}

fn fn_write_dtd_attlist(
    ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let name = bytes(&super::owned_argument(ed, 1));
    let content = bytes(&super::owned_argument(ed, 2));
    if !validate_xml_name(eg, ed, 2, "name", "element name", &name) {
        return Ok(());
    }
    bool_result(ed, rv, eg, |state| {
        state.start_dtd_declaration(b"<!ATTLIST ", &name, Section::DtdAttlist)
            && state.write_text(&content)
            && state.end_dtd_declaration(|section| matches!(section, Section::DtdAttlist))
    })
}

fn fn_start_dtd_element(
    ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let name = bytes(&super::owned_argument(ed, 1));
    if !validate_xml_name(eg, ed, 2, "name", "element name", &name) {
        return Ok(());
    }
    bool_result(ed, rv, eg, |state| {
        state.start_dtd_declaration(b"<!ELEMENT ", &name, Section::DtdElement)
    })
}

fn fn_end_dtd_element(
    ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    bool_result(ed, rv, eg, |state| {
        state.end_dtd_declaration(|section| matches!(section, Section::DtdElement))
    })
}

fn fn_write_dtd_element(
    ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let name = bytes(&super::owned_argument(ed, 1));
    let content = bytes(&super::owned_argument(ed, 2));
    if !validate_xml_name(eg, ed, 2, "name", "element name", &name) {
        return Ok(());
    }
    bool_result(ed, rv, eg, |state| {
        state.start_dtd_declaration(b"<!ELEMENT ", &name, Section::DtdElement)
            && state.write_text(&content)
            && state.end_dtd_declaration(|section| matches!(section, Section::DtdElement))
    })
}

fn fn_start_dtd_entity(
    ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let name = bytes(&super::owned_argument(ed, 1));
    let parameter = super::owned_argument(ed, 2).is_truthy();
    if !validate_xml_name(eg, ed, 2, "name", "attribute name", &name) {
        return Ok(());
    }
    bool_result(ed, rv, eg, |state| {
        let prefix = if parameter {
            b"<!ENTITY % ".as_slice()
        } else {
            b"<!ENTITY ".as_slice()
        };
        if !state.start_dtd_declaration(prefix, &name, Section::DtdEntity) {
            return false;
        }
        state.buffer.push(b'"');
        true
    })
}

fn fn_end_dtd_entity(
    ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    bool_result(ed, rv, eg, |state| {
        if !matches!(state.section, Some(Section::DtdEntity)) {
            return false;
        }
        state.buffer.extend_from_slice(b"\">");
        state.section = state.inside_dtd.then_some(Section::Dtd);
        true
    })
}

fn fn_write_dtd_entity(
    ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let name = bytes(&super::owned_argument(ed, 1));
    let content = bytes(&super::owned_argument(ed, 2));
    let parameter = super::owned_argument(ed, 3).is_truthy();
    let public = optional_bytes(ed, 4);
    let system = optional_bytes(ed, 5);
    let notation = optional_bytes(ed, 6);
    if !validate_xml_name(eg, ed, 2, "name", "element name", &name) {
        return Ok(());
    }
    bool_result(ed, rv, eg, |state| {
        if matches!(state.section, Some(Section::Dtd)) {
            state.ensure_dtd_subset();
        } else if state.section.is_some() || !state.before_node() {
            return false;
        }
        state.buffer.extend_from_slice(if parameter {
            b"<!ENTITY % ".as_slice()
        } else {
            b"<!ENTITY ".as_slice()
        });
        state.buffer.extend_from_slice(&name);
        if let Some(public) = &public {
            state.buffer.extend_from_slice(b" PUBLIC \"");
            state.buffer.extend_from_slice(public);
            state.buffer.push(b'"');
            if let Some(system) = &system {
                state.buffer.extend_from_slice(b" \"");
                state.buffer.extend_from_slice(system);
                state.buffer.push(b'"');
            }
        } else if let Some(system) = &system {
            state.buffer.extend_from_slice(b" SYSTEM \"");
            state.buffer.extend_from_slice(system);
            state.buffer.push(b'"');
        } else {
            state.buffer.extend_from_slice(b" \"");
            state.buffer.extend_from_slice(&content);
            state.buffer.push(b'"');
        }
        if let Some(notation) = &notation {
            state.buffer.extend_from_slice(b" NDATA ");
            state.buffer.extend_from_slice(notation);
        }
        state.buffer.push(b'>');
        true
    })
}

fn flush_target(
    ed: *mut ExecuteData,
    eg: &mut ExecutorGlobals,
    writer: &Value,
    memory_empty: bool,
) -> Result<Option<Value>, VmError> {
    enum Pending {
        Memory(Vec<u8>),
        Uri(Rc<RefCell<File>>, Vec<u8>),
        Stream(i64, Vec<u8>, usize, bool),
    }
    let pending = {
        let Some(mut object) = writer.as_object_mut() else {
            return Ok(None);
        };
        let state = object.native_object_state_mut::<State>();
        if !state.initialized() {
            drop(object);
            uninitialized(eg);
            return Ok(None);
        }
        if state.document_started
            && state.elements.is_empty()
            && state.section.is_none()
            && !state.buffer.is_empty()
            && !state.buffer.ends_with(b"\n")
        {
            state.buffer.push(b'\n');
        }
        let encoding = state.encoding.clone();
        match &state.target {
            Target::Memory => Pending::Memory(encoded_output(
                encoding.as_deref(),
                if memory_empty {
                    std::mem::take(&mut state.buffer)
                } else {
                    state.buffer.clone()
                },
            )),
            Target::Uri(file) => Pending::Uri(
                Rc::clone(file),
                encoded_output(encoding.as_deref(), std::mem::take(&mut state.buffer)),
            ),
            Target::Stream(stream) => {
                let Some(id) = stream.as_resource_id() else {
                    return Ok(Some(Value::long(-1)));
                };
                let previous = std::mem::take(&mut state.stream_since_flush);
                Pending::Stream(
                    id,
                    encoded_output(encoding.as_deref(), std::mem::take(&mut state.buffer)),
                    previous,
                    state.stream_failed,
                )
            }
            Target::Uninitialized => unreachable!(),
        }
    };
    match pending {
        Pending::Memory(bytes) => Ok(Some(super::php_byte_result(bytes, false))),
        Pending::Uri(file, bytes) => {
            let written = {
                let mut file = file.borrow_mut();
                file.write_all(&bytes).and_then(|_| file.flush())
            }
            .map(|_| bytes.len())
            .unwrap_or(0);
            Ok(Some(Value::long(written as i64)))
        }
        Pending::Stream(id, bytes, previous, failed) => {
            if failed {
                return Ok(Some(Value::long(-1)));
            }
            let written = super::streams::write_stream_bytes(eg, ed, id, &bytes)?
                .and_then(Result::ok)
                .filter(|written| *written == bytes.len());
            Ok(Some(match written {
                Some(written) => Value::long((previous + written) as i64),
                None => Value::long(-1),
            }))
        }
    }
}

fn fn_flush(ed: *mut ExecuteData, rv: *mut Value, eg: &mut ExecutorGlobals) -> Result<(), VmError> {
    let empty = match super::owned_argument(ed, 1).dereferenced().value_type() {
        ValueType::Undef => true,
        _ => super::owned_argument(ed, 1).is_truthy(),
    };
    let writer = receiver(ed);
    if let Some(result) = flush_target(ed, eg, &writer, empty)? {
        super::write_return_value(rv, result);
    }
    Ok(())
}

fn fn_output_memory(
    ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let flush = match super::owned_argument(ed, 1).dereferenced().value_type() {
        ValueType::Undef => true,
        _ => super::owned_argument(ed, 1).is_truthy(),
    };
    let writer = receiver(ed);
    let memory = writer.as_object().is_some_and(|object| {
        object
            .native_object_state::<State>()
            .is_some_and(|state| matches!(state.target, Target::Memory))
    });
    let result = flush_target(ed, eg, &writer, flush)?;
    if eg.exception.is_none() {
        super::write_return_value(
            rv,
            if memory {
                result.unwrap_or_else(|| Value::string(""))
            } else {
                Value::string("")
            },
        );
    }
    Ok(())
}

struct Procedure {
    name: &'static str,
    handler: InternalFunctionHandler,
    parameters: &'static [&'static str],
    required: u32,
    hints: fn() -> Vec<ParamTypeHint>,
    result: fn() -> ParamTypeHint,
    defaults: fn() -> Vec<Option<Value>>,
}

struct Method {
    name: &'static str,
    handler: InternalFunctionHandler,
    parameters: Vec<&'static str>,
    required: u32,
    hints: Vec<ParamTypeHint>,
    defaults: Vec<Option<Value>>,
    default_spellings: Vec<Option<&'static str>>,
    is_static: bool,
    result: ParamTypeHint,
}

fn writer_hint() -> ParamTypeHint {
    ParamTypeHint::ClassName(XML_WRITER.into())
}

fn nullable_string() -> ParamTypeHint {
    ParamTypeHint::Nullable(std::rc::Rc::new(ParamTypeHint::String))
}

fn writer_or_false() -> ParamTypeHint {
    ParamTypeHint::Union(vec![writer_hint(), ParamTypeHint::ClassName("false".into())].into())
}

fn string_or_int() -> ParamTypeHint {
    ParamTypeHint::Union(vec![ParamTypeHint::String, ParamTypeHint::Int].into())
}

fn no_hints() -> Vec<ParamTypeHint> {
    vec![]
}
fn bool_hint() -> ParamTypeHint {
    ParamTypeHint::Bool
}
fn string_hint() -> ParamTypeHint {
    ParamTypeHint::String
}
fn static_hint() -> ParamTypeHint {
    ParamTypeHint::ClassName("static".into())
}
fn none_hint() -> ParamTypeHint {
    ParamTypeHint::None
}
fn no_defaults() -> Vec<Option<Value>> {
    vec![]
}

fn procedures() -> Vec<Procedure> {
    vec![
        Procedure {
            name: "xmlwriter_open_uri",
            handler: fn_xmlwriter_open_uri,
            parameters: &["uri"],
            required: 1,
            hints: || vec![ParamTypeHint::String],
            result: writer_or_false,
            defaults: || vec![None],
        },
        Procedure {
            name: "xmlwriter_open_memory",
            handler: fn_xmlwriter_open_memory,
            parameters: &[],
            required: 0,
            hints: no_hints,
            result: writer_or_false,
            defaults: no_defaults,
        },
        Procedure {
            name: "xmlwriter_set_indent",
            handler: fn_set_indent,
            parameters: &["writer", "enable"],
            required: 2,
            hints: || vec![writer_hint(), ParamTypeHint::Bool],
            result: bool_hint,
            defaults: || vec![None, None],
        },
        Procedure {
            name: "xmlwriter_set_indent_string",
            handler: fn_set_indent_string,
            parameters: &["writer", "indentation"],
            required: 2,
            hints: || vec![writer_hint(), ParamTypeHint::String],
            result: bool_hint,
            defaults: || vec![None, None],
        },
        Procedure {
            name: "xmlwriter_start_comment",
            handler: fn_start_comment,
            parameters: &["writer"],
            required: 1,
            hints: || vec![writer_hint()],
            result: bool_hint,
            defaults: || vec![None],
        },
        Procedure {
            name: "xmlwriter_end_comment",
            handler: fn_end_comment,
            parameters: &["writer"],
            required: 1,
            hints: || vec![writer_hint()],
            result: bool_hint,
            defaults: || vec![None],
        },
        Procedure {
            name: "xmlwriter_start_attribute",
            handler: fn_start_attribute,
            parameters: &["writer", "name"],
            required: 2,
            hints: || vec![writer_hint(), ParamTypeHint::String],
            result: bool_hint,
            defaults: || vec![None, None],
        },
        Procedure {
            name: "xmlwriter_end_attribute",
            handler: fn_end_attribute,
            parameters: &["writer"],
            required: 1,
            hints: || vec![writer_hint()],
            result: bool_hint,
            defaults: || vec![None],
        },
        Procedure {
            name: "xmlwriter_write_attribute",
            handler: fn_write_attribute,
            parameters: &["writer", "name", "value"],
            required: 3,
            hints: || vec![writer_hint(), ParamTypeHint::String, ParamTypeHint::String],
            result: bool_hint,
            defaults: || vec![None, None, None],
        },
        Procedure {
            name: "xmlwriter_start_attribute_ns",
            handler: fn_start_attribute_ns,
            parameters: &["writer", "prefix", "name", "namespace"],
            required: 4,
            hints: || {
                vec![
                    writer_hint(),
                    nullable_string(),
                    ParamTypeHint::String,
                    nullable_string(),
                ]
            },
            result: bool_hint,
            defaults: || vec![None, None, None, None],
        },
        Procedure {
            name: "xmlwriter_write_attribute_ns",
            handler: fn_write_attribute_ns,
            parameters: &["writer", "prefix", "name", "namespace", "value"],
            required: 5,
            hints: || {
                vec![
                    writer_hint(),
                    nullable_string(),
                    ParamTypeHint::String,
                    nullable_string(),
                    ParamTypeHint::String,
                ]
            },
            result: bool_hint,
            defaults: || vec![None, None, None, None, None],
        },
        Procedure {
            name: "xmlwriter_start_element",
            handler: fn_start_element,
            parameters: &["writer", "name"],
            required: 2,
            hints: || vec![writer_hint(), ParamTypeHint::String],
            result: bool_hint,
            defaults: || vec![None, None],
        },
        Procedure {
            name: "xmlwriter_end_element",
            handler: fn_end_element,
            parameters: &["writer"],
            required: 1,
            hints: || vec![writer_hint()],
            result: bool_hint,
            defaults: || vec![None],
        },
        Procedure {
            name: "xmlwriter_full_end_element",
            handler: fn_full_end_element,
            parameters: &["writer"],
            required: 1,
            hints: || vec![writer_hint()],
            result: bool_hint,
            defaults: || vec![None],
        },
        Procedure {
            name: "xmlwriter_start_element_ns",
            handler: fn_start_element_ns,
            parameters: &["writer", "prefix", "name", "namespace"],
            required: 4,
            hints: || {
                vec![
                    writer_hint(),
                    nullable_string(),
                    ParamTypeHint::String,
                    nullable_string(),
                ]
            },
            result: bool_hint,
            defaults: || vec![None, None, None, None],
        },
        Procedure {
            name: "xmlwriter_write_element",
            handler: fn_write_element,
            parameters: &["writer", "name", "content"],
            required: 2,
            hints: || vec![writer_hint(), ParamTypeHint::String, nullable_string()],
            result: bool_hint,
            defaults: || vec![None, None, Some(Value::null())],
        },
        Procedure {
            name: "xmlwriter_write_element_ns",
            handler: fn_write_element_ns,
            parameters: &["writer", "prefix", "name", "namespace", "content"],
            required: 4,
            hints: || {
                vec![
                    writer_hint(),
                    nullable_string(),
                    ParamTypeHint::String,
                    nullable_string(),
                    nullable_string(),
                ]
            },
            result: bool_hint,
            defaults: || vec![None, None, None, None, Some(Value::null())],
        },
        Procedure {
            name: "xmlwriter_start_pi",
            handler: fn_start_pi,
            parameters: &["writer", "target"],
            required: 2,
            hints: || vec![writer_hint(), ParamTypeHint::String],
            result: bool_hint,
            defaults: || vec![None, None],
        },
        Procedure {
            name: "xmlwriter_end_pi",
            handler: fn_end_pi,
            parameters: &["writer"],
            required: 1,
            hints: || vec![writer_hint()],
            result: bool_hint,
            defaults: || vec![None],
        },
        Procedure {
            name: "xmlwriter_write_pi",
            handler: fn_write_pi,
            parameters: &["writer", "target", "content"],
            required: 3,
            hints: || vec![writer_hint(), ParamTypeHint::String, ParamTypeHint::String],
            result: bool_hint,
            defaults: || vec![None, None, None],
        },
        Procedure {
            name: "xmlwriter_start_cdata",
            handler: fn_start_cdata,
            parameters: &["writer"],
            required: 1,
            hints: || vec![writer_hint()],
            result: bool_hint,
            defaults: || vec![None],
        },
        Procedure {
            name: "xmlwriter_end_cdata",
            handler: fn_end_cdata,
            parameters: &["writer"],
            required: 1,
            hints: || vec![writer_hint()],
            result: bool_hint,
            defaults: || vec![None],
        },
        Procedure {
            name: "xmlwriter_write_cdata",
            handler: fn_write_cdata,
            parameters: &["writer", "content"],
            required: 2,
            hints: || vec![writer_hint(), ParamTypeHint::String],
            result: bool_hint,
            defaults: || vec![None, None],
        },
        Procedure {
            name: "xmlwriter_text",
            handler: fn_text,
            parameters: &["writer", "content"],
            required: 2,
            hints: || vec![writer_hint(), ParamTypeHint::String],
            result: bool_hint,
            defaults: || vec![None, None],
        },
        Procedure {
            name: "xmlwriter_write_raw",
            handler: fn_write_raw,
            parameters: &["writer", "content"],
            required: 2,
            hints: || vec![writer_hint(), ParamTypeHint::String],
            result: bool_hint,
            defaults: || vec![None, None],
        },
        Procedure {
            name: "xmlwriter_start_document",
            handler: fn_start_document,
            parameters: &["writer", "version", "encoding", "standalone"],
            required: 1,
            hints: || {
                vec![
                    writer_hint(),
                    nullable_string(),
                    nullable_string(),
                    nullable_string(),
                ]
            },
            result: bool_hint,
            defaults: || {
                vec![
                    None,
                    Some(Value::string("1.0")),
                    Some(Value::null()),
                    Some(Value::null()),
                ]
            },
        },
        Procedure {
            name: "xmlwriter_end_document",
            handler: fn_end_document,
            parameters: &["writer"],
            required: 1,
            hints: || vec![writer_hint()],
            result: bool_hint,
            defaults: || vec![None],
        },
        Procedure {
            name: "xmlwriter_write_comment",
            handler: fn_write_comment,
            parameters: &["writer", "content"],
            required: 2,
            hints: || vec![writer_hint(), ParamTypeHint::String],
            result: bool_hint,
            defaults: || vec![None, None],
        },
        Procedure {
            name: "xmlwriter_start_dtd",
            handler: fn_start_dtd,
            parameters: &["writer", "qualifiedName", "publicId", "systemId"],
            required: 2,
            hints: || {
                vec![
                    writer_hint(),
                    ParamTypeHint::String,
                    nullable_string(),
                    nullable_string(),
                ]
            },
            result: bool_hint,
            defaults: || vec![None, None, Some(Value::null()), Some(Value::null())],
        },
        Procedure {
            name: "xmlwriter_end_dtd",
            handler: fn_end_dtd,
            parameters: &["writer"],
            required: 1,
            hints: || vec![writer_hint()],
            result: bool_hint,
            defaults: || vec![None],
        },
        Procedure {
            name: "xmlwriter_write_dtd",
            handler: fn_write_dtd,
            parameters: &["writer", "name", "publicId", "systemId", "content"],
            required: 2,
            hints: || {
                vec![
                    writer_hint(),
                    ParamTypeHint::String,
                    nullable_string(),
                    nullable_string(),
                    nullable_string(),
                ]
            },
            result: bool_hint,
            defaults: || {
                vec![
                    None,
                    None,
                    Some(Value::null()),
                    Some(Value::null()),
                    Some(Value::null()),
                ]
            },
        },
        Procedure {
            name: "xmlwriter_start_dtd_element",
            handler: fn_start_dtd_element,
            parameters: &["writer", "qualifiedName"],
            required: 2,
            hints: || vec![writer_hint(), ParamTypeHint::String],
            result: bool_hint,
            defaults: || vec![None, None],
        },
        Procedure {
            name: "xmlwriter_end_dtd_element",
            handler: fn_end_dtd_element,
            parameters: &["writer"],
            required: 1,
            hints: || vec![writer_hint()],
            result: bool_hint,
            defaults: || vec![None],
        },
        Procedure {
            name: "xmlwriter_write_dtd_element",
            handler: fn_write_dtd_element,
            parameters: &["writer", "name", "content"],
            required: 3,
            hints: || vec![writer_hint(), ParamTypeHint::String, ParamTypeHint::String],
            result: bool_hint,
            defaults: || vec![None, None, None],
        },
        Procedure {
            name: "xmlwriter_start_dtd_attlist",
            handler: fn_start_dtd_attlist,
            parameters: &["writer", "name"],
            required: 2,
            hints: || vec![writer_hint(), ParamTypeHint::String],
            result: bool_hint,
            defaults: || vec![None, None],
        },
        Procedure {
            name: "xmlwriter_end_dtd_attlist",
            handler: fn_end_dtd_attlist,
            parameters: &["writer"],
            required: 1,
            hints: || vec![writer_hint()],
            result: bool_hint,
            defaults: || vec![None],
        },
        Procedure {
            name: "xmlwriter_write_dtd_attlist",
            handler: fn_write_dtd_attlist,
            parameters: &["writer", "name", "content"],
            required: 3,
            hints: || vec![writer_hint(), ParamTypeHint::String, ParamTypeHint::String],
            result: bool_hint,
            defaults: || vec![None, None, None],
        },
        Procedure {
            name: "xmlwriter_start_dtd_entity",
            handler: fn_start_dtd_entity,
            parameters: &["writer", "name", "isParam"],
            required: 3,
            hints: || vec![writer_hint(), ParamTypeHint::String, ParamTypeHint::Bool],
            result: bool_hint,
            defaults: || vec![None, None, None],
        },
        Procedure {
            name: "xmlwriter_end_dtd_entity",
            handler: fn_end_dtd_entity,
            parameters: &["writer"],
            required: 1,
            hints: || vec![writer_hint()],
            result: bool_hint,
            defaults: || vec![None],
        },
        Procedure {
            name: "xmlwriter_write_dtd_entity",
            handler: fn_write_dtd_entity,
            parameters: &[
                "writer",
                "name",
                "content",
                "isParam",
                "publicId",
                "systemId",
                "notationData",
            ],
            required: 3,
            hints: || {
                vec![
                    writer_hint(),
                    ParamTypeHint::String,
                    ParamTypeHint::String,
                    ParamTypeHint::Bool,
                    nullable_string(),
                    nullable_string(),
                    nullable_string(),
                ]
            },
            result: bool_hint,
            defaults: || {
                vec![
                    None,
                    None,
                    None,
                    Some(Value::bool(false)),
                    Some(Value::null()),
                    Some(Value::null()),
                    Some(Value::null()),
                ]
            },
        },
        Procedure {
            name: "xmlwriter_output_memory",
            handler: fn_output_memory,
            parameters: &["writer", "flush"],
            required: 1,
            hints: || vec![writer_hint(), ParamTypeHint::Bool],
            result: string_hint,
            defaults: || vec![None, Some(Value::bool(true))],
        },
        Procedure {
            name: "xmlwriter_flush",
            handler: fn_flush,
            parameters: &["writer", "empty"],
            required: 1,
            hints: || vec![writer_hint(), ParamTypeHint::Bool],
            result: string_or_int,
            defaults: || vec![None, Some(Value::bool(true))],
        },
    ]
}

fn methods() -> Vec<Method> {
    let mut methods = vec![
        Method {
            name: "toMemory",
            handler: fn_to_memory,
            parameters: vec![],
            required: 0,
            hints: vec![],
            defaults: vec![],
            default_spellings: vec![],
            is_static: true,
            result: static_hint(),
        },
        Method {
            name: "toUri",
            handler: fn_to_uri,
            parameters: vec!["uri"],
            required: 1,
            hints: vec![ParamTypeHint::String],
            defaults: vec![None],
            default_spellings: vec![None],
            is_static: true,
            result: static_hint(),
        },
        Method {
            name: "toStream",
            handler: fn_to_stream,
            parameters: vec!["stream"],
            required: 1,
            // PHP validates a stream resource in the handler while exposing
            // no declaration type through Reflection.
            hints: vec![ParamTypeHint::None],
            defaults: vec![None],
            default_spellings: vec![None],
            is_static: true,
            result: static_hint(),
        },
        Method {
            name: "openMemory",
            handler: fn_open_memory,
            parameters: vec![],
            required: 0,
            hints: vec![],
            defaults: vec![],
            default_spellings: vec![],
            is_static: false,
            result: none_hint(),
        },
        Method {
            name: "openUri",
            handler: fn_open_uri,
            parameters: vec!["uri"],
            required: 1,
            hints: vec![ParamTypeHint::String],
            defaults: vec![None],
            default_spellings: vec![None],
            is_static: false,
            result: none_hint(),
        },
    ];
    let names = [
        ("setIndent", fn_set_indent as InternalFunctionHandler),
        ("setIndentString", fn_set_indent_string),
        ("startComment", fn_start_comment),
        ("endComment", fn_end_comment),
        ("startAttribute", fn_start_attribute),
        ("endAttribute", fn_end_attribute),
        ("writeAttribute", fn_write_attribute),
        ("startAttributeNs", fn_start_attribute_ns),
        ("writeAttributeNs", fn_write_attribute_ns),
        ("startElement", fn_start_element),
        ("endElement", fn_end_element),
        ("fullEndElement", fn_full_end_element),
        ("startElementNs", fn_start_element_ns),
        ("writeElement", fn_write_element),
        ("writeElementNs", fn_write_element_ns),
        ("startPi", fn_start_pi),
        ("endPi", fn_end_pi),
        ("writePi", fn_write_pi),
        ("startCdata", fn_start_cdata),
        ("endCdata", fn_end_cdata),
        ("writeCdata", fn_write_cdata),
        ("text", fn_text),
        ("writeRaw", fn_write_raw),
        ("startDocument", fn_start_document),
        ("endDocument", fn_end_document),
        ("writeComment", fn_write_comment),
        ("startDtd", fn_start_dtd),
        ("endDtd", fn_end_dtd),
        ("writeDtd", fn_write_dtd),
        ("startDtdElement", fn_start_dtd_element),
        ("endDtdElement", fn_end_dtd_element),
        ("writeDtdElement", fn_write_dtd_element),
        ("startDtdAttlist", fn_start_dtd_attlist),
        ("endDtdAttlist", fn_end_dtd_attlist),
        ("writeDtdAttlist", fn_write_dtd_attlist),
        ("startDtdEntity", fn_start_dtd_entity),
        ("endDtdEntity", fn_end_dtd_entity),
        ("writeDtdEntity", fn_write_dtd_entity),
        ("outputMemory", fn_output_memory),
        ("flush", fn_flush),
    ];
    let procedure_list = procedures();
    for (name, handler) in names {
        let snake = camel_to_procedure(name);
        let procedure = procedure_list
            .iter()
            .find(|declaration| declaration.name == snake)
            .unwrap();
        let public_parameters = procedure.parameters[1..].to_vec();
        let public_hints = (procedure.hints)().into_iter().skip(1).collect::<Vec<_>>();
        let public_defaults = (procedure.defaults)()
            .into_iter()
            .skip(1)
            .collect::<Vec<_>>();
        let spellings = default_spellings_for(name, public_parameters.len());
        methods.push(Method {
            name,
            handler,
            parameters: public_parameters,
            required: procedure.required - 1,
            hints: public_hints,
            defaults: public_defaults,
            default_spellings: spellings,
            is_static: false,
            result: none_hint(),
        });
    }
    methods
}

fn camel_to_procedure(name: &str) -> &'static str {
    match name {
        "setIndent" => "xmlwriter_set_indent",
        "setIndentString" => "xmlwriter_set_indent_string",
        "startComment" => "xmlwriter_start_comment",
        "endComment" => "xmlwriter_end_comment",
        "startAttribute" => "xmlwriter_start_attribute",
        "endAttribute" => "xmlwriter_end_attribute",
        "writeAttribute" => "xmlwriter_write_attribute",
        "startAttributeNs" => "xmlwriter_start_attribute_ns",
        "writeAttributeNs" => "xmlwriter_write_attribute_ns",
        "startElement" => "xmlwriter_start_element",
        "endElement" => "xmlwriter_end_element",
        "fullEndElement" => "xmlwriter_full_end_element",
        "startElementNs" => "xmlwriter_start_element_ns",
        "writeElement" => "xmlwriter_write_element",
        "writeElementNs" => "xmlwriter_write_element_ns",
        "startPi" => "xmlwriter_start_pi",
        "endPi" => "xmlwriter_end_pi",
        "writePi" => "xmlwriter_write_pi",
        "startCdata" => "xmlwriter_start_cdata",
        "endCdata" => "xmlwriter_end_cdata",
        "writeCdata" => "xmlwriter_write_cdata",
        "text" => "xmlwriter_text",
        "writeRaw" => "xmlwriter_write_raw",
        "startDocument" => "xmlwriter_start_document",
        "endDocument" => "xmlwriter_end_document",
        "writeComment" => "xmlwriter_write_comment",
        "startDtd" => "xmlwriter_start_dtd",
        "endDtd" => "xmlwriter_end_dtd",
        "writeDtd" => "xmlwriter_write_dtd",
        "startDtdElement" => "xmlwriter_start_dtd_element",
        "endDtdElement" => "xmlwriter_end_dtd_element",
        "writeDtdElement" => "xmlwriter_write_dtd_element",
        "startDtdAttlist" => "xmlwriter_start_dtd_attlist",
        "endDtdAttlist" => "xmlwriter_end_dtd_attlist",
        "writeDtdAttlist" => "xmlwriter_write_dtd_attlist",
        "startDtdEntity" => "xmlwriter_start_dtd_entity",
        "endDtdEntity" => "xmlwriter_end_dtd_entity",
        "writeDtdEntity" => "xmlwriter_write_dtd_entity",
        "outputMemory" => "xmlwriter_output_memory",
        "flush" => "xmlwriter_flush",
        _ => unreachable!(),
    }
}

fn default_spellings_for(name: &str, length: usize) -> Vec<Option<&'static str>> {
    let mut values = vec![None; length];
    match name {
        "writeElement" => values[1] = Some("null"),
        "writeElementNs" => values[3] = Some("null"),
        "startDocument" => {
            values[0] = Some("'1.0'");
            values[1] = Some("null");
            values[2] = Some("null");
        }
        "startDtd" => {
            values[1] = Some("null");
            values[2] = Some("null");
        }
        "writeDtd" => {
            values[1] = Some("null");
            values[2] = Some("null");
            values[3] = Some("null");
        }
        "writeDtdEntity" => {
            values[2] = Some("false");
            values[3] = Some("null");
            values[4] = Some("null");
            values[5] = Some("null");
        }
        "outputMemory" | "flush" => values[0] = Some("true"),
        _ => {}
    }
    values
}

pub(super) fn register_class(eg: &mut ExecutorGlobals) -> Vec<Box<InternalFunction>> {
    eg.register_class(ClassDef {
        attributes: vec![],
        name: XML_WRITER.into(),
        source_file: None,
        declaration_line: 0,
        end_line: 0,
        doc_comment: None,
        parent: None,
        implements: vec![],
        is_interface: false,
        is_abstract: false,
        is_final: false,
        is_readonly: false,
        allow_dynamic_properties: false,
        is_trait: false,
        is_enum: false,
        uses: vec![],
        trait_aliases: vec![],
        trait_precedences: vec![],
        properties: vec![],
        static_properties: vec![],
        constants: vec![],
        property_layout: Rc::new(ObjectLayout::empty()),
        property_defaults: Rc::from([]),
        readonly_props: vec![],
        methods: vec![],
        abstract_methods: vec![],
        enum_backing_error: None,
        deferred_instance_defaults: None,
        class_id: 0,
    })
    .expect("XMLWriter registers once per request");

    let mut functions = Vec::with_capacity(45);
    for method in methods() {
        let hints = method.hints.clone();
        eg.register_internal_method_contract(
            XML_WRITER,
            method.name,
            method.is_static,
            method.required,
            &method.parameters,
            hints.clone(),
            method.result.clone(),
            &method.default_spellings,
            false,
        );
        let mut function = Box::new(
            make_internal_method(
                method.handler,
                method.parameters.len() as u32 + 1,
                method.required,
                vec![],
            )
            .with_static_parameter_names(&method.parameters),
        );
        function.common.sig.param_type_hints = hints;
        function.common.sig.return_type_hint = method.result;
        let pointer = &function.common as *const FunctionCommon;
        eg.insert_function_entry(
            super::builtin_classes::internal_method_lookup_name(XML_WRITER, method.name),
            pointer,
        );
        eg.method_declaring_class.insert(pointer, XML_WRITER.into());
        if method.is_static {
            eg.register_internal_static_method(pointer);
        }
        eg.register_internal_function_display_name(
            pointer,
            super::builtin_classes::internal_method_display_name(XML_WRITER, method.name),
        );
        eg.register_internal_function_reflection_metadata(pointer, method.defaults, "xmlwriter");
        functions.push(function);
    }
    functions
}

pub(super) fn register_functions(eg: &mut ExecutorGlobals) -> Vec<Box<InternalFunction>> {
    let declarations = procedures();
    let mut functions = Vec::with_capacity(declarations.len());
    for declaration in declarations {
        let mut function = Box::new(
            make_internal_function(
                declaration.handler,
                declaration.parameters.len() as u32,
                declaration.required,
                vec![],
            )
            .with_static_parameter_names(declaration.parameters),
        );
        function.common.sig.param_type_hints = (declaration.hints)();
        function.common.sig.return_type_hint = (declaration.result)();
        let pointer = &function.common as *const FunctionCommon;
        eg.register_function(declaration.name, pointer)
            .expect("XMLWriter function registers once per request");
        eg.register_internal_function_reflection_metadata(
            pointer,
            (declaration.defaults)(),
            "xmlwriter",
        );
        functions.push(function);
    }
    functions
}
