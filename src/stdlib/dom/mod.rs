//! Independently implemented PHP 8.5 DOM foundation.
//!
//! The legacy DOM tree used by PHPUnit is request-owned Rust state.  Parsing,
//! traversal, mutation, serialization and the admitted XPath subset do not
//! delegate to a host libxml2 installation.

mod model;
mod parser;
mod xpath;

use std::cell::RefCell;
use std::fs;
use std::rc::Rc;

use crate::compiler::compile::{ClassConstantDefinition, ClassDef, PropertyDefinition};
use crate::compiler::{
    make_internal_function, make_internal_method, make_internal_method_variadic,
};
use crate::parser::Visibility;
use crate::runtime::ExecutorGlobals;
use crate::value::{NativeObjectState, ObjectLayout, PhpArray, PhpObject, Value, ValueType};
use crate::vm::execute::VmError;
use crate::vm::frame::ExecuteData;
use crate::vm::function::{
    FunctionCommon, InternalFunction, InternalFunctionHandler, ParamTypeHint,
};

use model::{Attribute, NodeId, NodeKind, Tree, split_name, valid_xml_name};

const DOM_NODE: &str = "DOMNode";
const DOM_DOCUMENT: &str = "DOMDocument";
const DOM_ELEMENT: &str = "DOMElement";
const DOM_ATTR: &str = "DOMAttr";
const DOM_TEXT: &str = "DOMText";
const DOM_COMMENT: &str = "DOMComment";
const DOM_CDATA: &str = "DOMCdataSection";
const DOM_CHARACTER_DATA: &str = "DOMCharacterData";
const DOM_FRAGMENT: &str = "DOMDocumentFragment";
const DOM_DOCUMENT_TYPE: &str = "DOMDocumentType";
const DOM_PI: &str = "DOMProcessingInstruction";
const DOM_NODE_LIST: &str = "DOMNodeList";
const DOM_NAMED_NODE_MAP: &str = "DOMNamedNodeMap";
const DOM_IMPLEMENTATION: &str = "DOMImplementation";
const DOM_XPATH: &str = "DOMXPath";

#[derive(Clone)]
enum Handle {
    Node(NodeId),
    Attribute { element: NodeId, index: usize },
}

#[derive(Clone)]
struct DomState {
    tree: Rc<RefCell<Tree>>,
    handle: Handle,
}

impl Default for DomState {
    fn default() -> Self {
        Self {
            tree: Rc::new(RefCell::new(Tree::default())),
            handle: Handle::Node(0),
        }
    }
}

impl NativeObjectState for DomState {
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

#[derive(Clone, Default)]
struct DomListState {
    values: Vec<Value>,
    live: Option<DomListSource>,
}

#[derive(Clone)]
enum DomListSource {
    Children {
        tree: Rc<RefCell<Tree>>,
        parent: NodeId,
    },
    Attributes {
        tree: Rc<RefCell<Tree>>,
        element: NodeId,
    },
}

impl NativeObjectState for DomListState {
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
        for value in &self.values {
            visit(value);
        }
    }

    fn append_values_reversed(&mut self, pending: &mut Vec<Value>) {
        while let Some(value) = self.values.pop() {
            pending.push(value);
        }
    }
}

#[derive(Clone)]
struct XPathState {
    tree: Rc<RefCell<Tree>>,
    document: Value,
    register_node_namespaces: bool,
    namespaces: Vec<(Vec<u8>, Vec<u8>)>,
}

impl Default for XPathState {
    fn default() -> Self {
        Self {
            tree: Rc::new(RefCell::new(Tree::default())),
            document: Value::null(),
            register_node_namespaces: true,
            namespaces: Vec::new(),
        }
    }
}

impl NativeObjectState for XPathState {
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
        visit(&self.document);
    }

    fn append_values_reversed(&mut self, pending: &mut Vec<Value>) {
        pending.push(std::mem::replace(&mut self.document, Value::null()));
    }
}

fn bytes(value: &Value) -> Vec<u8> {
    value
        .dereferenced()
        .php_string_bytes()
        .unwrap_or_default()
        .into_owned()
}

fn receiver(ed: *mut ExecuteData) -> Value {
    arg!(ed, 0).clone()
}

fn state(value: &Value) -> Option<DomState> {
    value
        .as_object()?
        .native_object_state::<DomState>()
        .cloned()
}

fn node_state(value: &Value) -> Option<(Rc<RefCell<Tree>>, NodeId)> {
    let state = state(value)?;
    match state.handle {
        Handle::Node(node) => Some((state.tree, node)),
        Handle::Attribute { .. } => None,
    }
}

fn set_state(value: &Value, state: DomState) -> bool {
    let Some(mut object) = value.as_object_mut() else {
        return false;
    };
    *object.native_object_state_mut::<DomState>() = state;
    true
}

fn new_object(eg: &ExecutorGlobals, class_name: &str) -> Option<Value> {
    let class = eg.find_class(class_name)?;
    Some(Value::object(PhpObject::with_layout(
        class.class_id,
        Rc::clone(&class.property_layout),
        class.property_defaults.to_vec(),
    )))
}

fn bytes_value(value: Vec<u8>) -> Value {
    super::php_byte_result(value, false)
}

fn namespace_declaration(prefix: &[u8], namespace: &[u8]) -> Attribute {
    let (qualified_name, local_name) = if prefix.is_empty() {
        (b"xmlns".to_vec(), b"xmlns".to_vec())
    } else {
        let mut qualified_name = b"xmlns:".to_vec();
        qualified_name.extend_from_slice(prefix);
        (qualified_name, prefix.to_vec())
    };
    Attribute {
        qualified_name,
        prefix: if prefix.is_empty() {
            Vec::new()
        } else {
            b"xmlns".to_vec()
        },
        local_name,
        namespace: Some(b"http://www.w3.org/2000/xmlns/".to_vec()),
        value: namespace.to_vec(),
        is_id: false,
    }
}

fn optional_bytes_value(value: Option<Vec<u8>>) -> Value {
    value.map_or_else(Value::null, bytes_value)
}

fn wrap_node(
    eg: &ExecutorGlobals,
    tree: Rc<RefCell<Tree>>,
    node: NodeId,
    relationships: bool,
) -> Value {
    let class = {
        let tree_ref = tree.borrow();
        match tree_ref.nodes[node].kind {
            NodeKind::Document => DOM_DOCUMENT,
            NodeKind::Element { .. } => DOM_ELEMENT,
            NodeKind::Text(_) => DOM_TEXT,
            NodeKind::Cdata(_) => DOM_CDATA,
            NodeKind::Comment(_) => DOM_COMMENT,
            NodeKind::ProcessingInstruction { .. } => DOM_PI,
            NodeKind::Fragment => DOM_FRAGMENT,
            NodeKind::DocumentType { .. } => DOM_DOCUMENT_TYPE,
        }
    };
    let mut value = new_object(eg, class).expect("DOM classes are registered before use");
    set_state(
        &value,
        DomState {
            tree: Rc::clone(&tree),
            handle: Handle::Node(node),
        },
    );
    refresh_node_properties(eg, &mut value, relationships);
    value
}

fn wrap_attribute(
    eg: &ExecutorGlobals,
    tree: Rc<RefCell<Tree>>,
    element: NodeId,
    index: usize,
    relationships: bool,
) -> Value {
    let mut value = new_object(eg, DOM_ATTR).expect("DOMAttr is registered before use");
    set_state(
        &value,
        DomState {
            tree: Rc::clone(&tree),
            handle: Handle::Attribute { element, index },
        },
    );
    refresh_attribute_properties(eg, &mut value, relationships);
    value
}

fn new_node_list(eg: &ExecutorGlobals, tree: Rc<RefCell<Tree>>, nodes: &[NodeId]) -> Value {
    let values = nodes
        .iter()
        .map(|node| wrap_node(eg, Rc::clone(&tree), *node, true))
        .collect::<Vec<_>>();
    new_value_list(eg, DOM_NODE_LIST, values)
}

fn new_child_node_list(eg: &ExecutorGlobals, tree: Rc<RefCell<Tree>>, parent: NodeId) -> Value {
    let length = tree.borrow().nodes[parent].children.len();
    let value = new_object(eg, DOM_NODE_LIST).expect("DOMNodeList is registered");
    if let Some(mut object) = value.as_object_mut() {
        object.set_property("length", Value::long(length as i64));
        object.native_object_state_mut::<DomListState>().live =
            Some(DomListSource::Children { tree, parent });
    }
    value
}

fn new_named_node_map(eg: &ExecutorGlobals, tree: Rc<RefCell<Tree>>, element: NodeId) -> Value {
    let length = tree
        .borrow()
        .attributes(element)
        .map_or(0, <[Attribute]>::len);
    let value = new_object(eg, DOM_NAMED_NODE_MAP).expect("DOMNamedNodeMap is registered");
    if let Some(mut object) = value.as_object_mut() {
        object.set_property("length", Value::long(length as i64));
        object.native_object_state_mut::<DomListState>().live =
            Some(DomListSource::Attributes { tree, element });
    }
    value
}

fn new_value_list(eg: &ExecutorGlobals, class_name: &str, values: Vec<Value>) -> Value {
    let value = new_object(eg, class_name).expect("DOM collection class is registered");
    if let Some(mut object) = value.as_object_mut() {
        object.set_property("length", Value::long(values.len() as i64));
        object.native_object_state_mut::<DomListState>().values = values;
    }
    value
}

fn refresh_node_properties(eg: &ExecutorGlobals, value: &mut Value, relationships: bool) {
    let Some((tree, node)) = node_state(value) else {
        return;
    };
    let (
        name,
        node_type,
        node_value,
        text_content,
        parent,
        _children,
        first,
        last,
        previous,
        next,
        owner,
        namespace,
        prefix,
        local_name,
        element_children,
        previous_element,
        next_element,
        kind,
    ) = {
        let tree_ref = tree.borrow();
        let parent = tree_ref.nodes[node].parent;
        let children = tree_ref.nodes[node].children.clone();
        let first = children.first().copied();
        let last = children.last().copied();
        let previous = tree_ref.sibling(node, false, false);
        let next = tree_ref.sibling(node, true, false);
        let previous_element = tree_ref.sibling(node, false, true);
        let next_element = tree_ref.sibling(node, true, true);
        (
            tree_ref.node_name(node),
            tree_ref.node_type(node),
            tree_ref.node_value(node),
            tree_ref.text_content(node),
            parent,
            children,
            first,
            last,
            previous,
            next,
            tree_ref.owner_document(node),
            tree_ref.namespace(node).map(<[u8]>::to_vec),
            tree_ref.prefix(node).to_vec(),
            tree_ref.local_name(node).map(<[u8]>::to_vec),
            tree_ref.element_children(node),
            previous_element,
            next_element,
            tree_ref.nodes[node].kind.clone(),
        )
    };
    let Some(mut object) = value.as_object_mut() else {
        return;
    };
    object.set_property("nodeName", bytes_value(name.clone()));
    object.set_property("nodeValue", optional_bytes_value(node_value.clone()));
    object.set_property("nodeType", Value::long(node_type));
    object.set_property("textContent", bytes_value(text_content));
    object.set_property("namespaceURI", optional_bytes_value(namespace));
    object.set_property("prefix", bytes_value(prefix));
    object.set_property("localName", optional_bytes_value(local_name));
    object.set_property("baseURI", Value::null());
    object.set_property(
        "isConnected",
        Value::bool(tree.borrow().is_ancestor(0, node) || node == 0),
    );

    if relationships {
        object.set_property(
            "parentNode",
            parent.map_or_else(Value::null, |parent| {
                wrap_node(eg, Rc::clone(&tree), parent, false)
            }),
        );
        object.set_property(
            "parentElement",
            parent
                .filter(|parent| {
                    matches!(tree.borrow().nodes[*parent].kind, NodeKind::Element { .. })
                })
                .map_or_else(Value::null, |parent| {
                    wrap_node(eg, Rc::clone(&tree), parent, false)
                }),
        );
        object.set_property(
            "childNodes",
            new_child_node_list(eg, Rc::clone(&tree), node),
        );
        object.set_property(
            "firstChild",
            first.map_or_else(Value::null, |first| {
                wrap_node(eg, Rc::clone(&tree), first, false)
            }),
        );
        object.set_property(
            "lastChild",
            last.map_or_else(Value::null, |last| {
                wrap_node(eg, Rc::clone(&tree), last, false)
            }),
        );
        object.set_property(
            "previousSibling",
            previous.map_or_else(Value::null, |previous| {
                wrap_node(eg, Rc::clone(&tree), previous, false)
            }),
        );
        object.set_property(
            "nextSibling",
            next.map_or_else(Value::null, |next| {
                wrap_node(eg, Rc::clone(&tree), next, false)
            }),
        );
        object.set_property(
            "ownerDocument",
            owner.map_or_else(Value::null, |owner| {
                wrap_node(eg, Rc::clone(&tree), owner, false)
            }),
        );
        object.set_property(
            "attributes",
            if matches!(kind, NodeKind::Element { .. }) {
                new_named_node_map(eg, Rc::clone(&tree), node)
            } else {
                Value::null()
            },
        );
    } else {
        object.set_property("parentNode", Value::null());
        object.set_property("parentElement", Value::null());
        object.set_property("childNodes", new_value_list(eg, DOM_NODE_LIST, Vec::new()));
        object.set_property("firstChild", Value::null());
        object.set_property("lastChild", Value::null());
        object.set_property("previousSibling", Value::null());
        object.set_property("nextSibling", Value::null());
        object.set_property("attributes", Value::null());
        object.set_property("ownerDocument", Value::null());
    }

    match kind {
        NodeKind::Document => {
            let tree_ref = tree.borrow();
            let document_element = tree_ref.document_element();
            let document_type = tree_ref.document_type();
            let version = tree_ref.version.clone();
            let encoding = tree_ref.encoding.clone();
            let standalone = tree_ref.standalone;
            let document_uri = tree_ref.document_uri.clone();
            drop(tree_ref);
            object.set_property(
                "documentElement",
                document_element.map_or_else(Value::null, |root| {
                    wrap_node(eg, Rc::clone(&tree), root, relationships)
                }),
            );
            object.set_property(
                "doctype",
                document_type.map_or_else(Value::null, |doctype| {
                    wrap_node(eg, Rc::clone(&tree), doctype, false)
                }),
            );
            object.set_property(
                "implementation",
                new_object(eg, DOM_IMPLEMENTATION).unwrap(),
            );
            object.set_property("actualEncoding", optional_bytes_value(encoding.clone()));
            object.set_property("encoding", optional_bytes_value(encoding.clone()));
            object.set_property("xmlEncoding", optional_bytes_value(encoding));
            object.set_property("standalone", Value::bool(standalone));
            object.set_property("xmlStandalone", Value::bool(standalone));
            object.set_property("version", bytes_value(version.clone()));
            object.set_property("xmlVersion", bytes_value(version));
            object.set_property("documentURI", optional_bytes_value(document_uri));
            set_parent_node_properties(eg, &mut object, &tree, &element_children);
        }
        NodeKind::Element {
            qualified_name,
            attributes,
            ..
        } => {
            object.set_property("tagName", bytes_value(qualified_name));
            let class_name = attributes
                .iter()
                .find(|attribute| attribute.qualified_name == b"class")
                .map(|attribute| attribute.value.clone())
                .unwrap_or_default();
            let id = attributes
                .iter()
                .find(|attribute| attribute.qualified_name == b"id")
                .map(|attribute| attribute.value.clone())
                .unwrap_or_default();
            object.set_property("className", bytes_value(class_name));
            object.set_property("id", bytes_value(id));
            object.set_property("schemaTypeInfo", Value::null());
            set_parent_node_properties(eg, &mut object, &tree, &element_children);
            if relationships {
                object.set_property(
                    "previousElementSibling",
                    previous_element.map_or_else(Value::null, |sibling| {
                        wrap_node(eg, Rc::clone(&tree), sibling, false)
                    }),
                );
                object.set_property(
                    "nextElementSibling",
                    next_element.map_or_else(Value::null, |sibling| {
                        wrap_node(eg, Rc::clone(&tree), sibling, false)
                    }),
                );
            } else {
                object.set_property("previousElementSibling", Value::null());
                object.set_property("nextElementSibling", Value::null());
            }
        }
        NodeKind::Text(data) => {
            object.set_property("data", bytes_value(data.clone()));
            object.set_property("length", Value::long(data.len() as i64));
            object.set_property("wholeText", bytes_value(data));
        }
        NodeKind::Cdata(data) | NodeKind::Comment(data) => {
            object.set_property("data", bytes_value(data.clone()));
            object.set_property("length", Value::long(data.len() as i64));
            if object.class_name.eq_ignore_ascii_case(DOM_CDATA) {
                object.set_property("wholeText", bytes_value(data));
            }
        }
        NodeKind::ProcessingInstruction { target, data } => {
            object.set_property("target", bytes_value(target));
            object.set_property("data", bytes_value(data));
        }
        NodeKind::DocumentType {
            name,
            public_id,
            system_id,
            internal_subset,
        } => {
            object.set_property("name", bytes_value(name));
            object.set_property("publicId", bytes_value(public_id));
            object.set_property("systemId", bytes_value(system_id));
            object.set_property("internalSubset", optional_bytes_value(internal_subset));
            object.set_property(
                "entities",
                new_value_list(eg, DOM_NAMED_NODE_MAP, Vec::new()),
            );
            object.set_property(
                "notations",
                new_value_list(eg, DOM_NAMED_NODE_MAP, Vec::new()),
            );
        }
        NodeKind::Fragment => set_parent_node_properties(eg, &mut object, &tree, &element_children),
    }
}

fn set_parent_node_properties(
    eg: &ExecutorGlobals,
    object: &mut PhpObject,
    tree: &Rc<RefCell<Tree>>,
    element_children: &[NodeId],
) {
    object.set_property(
        "firstElementChild",
        element_children.first().map_or_else(Value::null, |child| {
            wrap_node(eg, Rc::clone(tree), *child, false)
        }),
    );
    object.set_property(
        "lastElementChild",
        element_children.last().map_or_else(Value::null, |child| {
            wrap_node(eg, Rc::clone(tree), *child, false)
        }),
    );
    object.set_property(
        "childElementCount",
        Value::long(element_children.len() as i64),
    );
}

fn refresh_attribute_properties(eg: &ExecutorGlobals, value: &mut Value, relationships: bool) {
    let Some(state) = state(value) else {
        return;
    };
    let Handle::Attribute { element, index } = state.handle else {
        return;
    };
    let Some(attribute) = state
        .tree
        .borrow()
        .attributes(element)
        .and_then(|attributes| attributes.get(index))
        .cloned()
    else {
        return;
    };
    let Some(mut object) = value.as_object_mut() else {
        return;
    };
    object.set_property("nodeName", bytes_value(attribute.qualified_name.clone()));
    object.set_property("nodeValue", bytes_value(attribute.value.clone()));
    object.set_property("nodeType", Value::long(2));
    object.set_property("name", bytes_value(attribute.qualified_name.clone()));
    object.set_property("value", bytes_value(attribute.value.clone()));
    object.set_property("specified", Value::bool(true));
    object.set_property("textContent", bytes_value(attribute.value));
    object.set_property("namespaceURI", optional_bytes_value(attribute.namespace));
    object.set_property("prefix", bytes_value(attribute.prefix));
    object.set_property("localName", bytes_value(attribute.local_name));
    object.set_property("isConnected", Value::bool(true));
    object.set_property("schemaTypeInfo", Value::null());
    object.set_property(
        "ownerElement",
        if relationships {
            wrap_node(eg, Rc::clone(&state.tree), element, false)
        } else {
            Value::null()
        },
    );
    object.set_property("parentNode", Value::null());
    object.set_property("parentElement", Value::null());
    object.set_property("childNodes", new_value_list(eg, DOM_NODE_LIST, Vec::new()));
    object.set_property("firstChild", Value::null());
    object.set_property("lastChild", Value::null());
    object.set_property("previousSibling", Value::null());
    object.set_property("nextSibling", Value::null());
    object.set_property("attributes", Value::null());
    object.set_property(
        "ownerDocument",
        if relationships {
            wrap_node(eg, Rc::clone(&state.tree), 0, false)
        } else {
            Value::null()
        },
    );
    object.set_property("baseURI", Value::null());
}

fn publish_parse_error(
    eg: &mut ExecutorGlobals,
    ed: *mut ExecuteData,
    error: &parser::ParseError,
) -> Result<(), VmError> {
    publish_libxml_error(eg, ed, error, "DOMDocument::loadXML")
}

fn publish_libxml_error(
    eg: &mut ExecutorGlobals,
    ed: *mut ExecuteData,
    error: &parser::ParseError,
    function: &str,
) -> Result<(), VmError> {
    let Some(value) = new_object(eg, "LibXMLError") else {
        return Ok(());
    };
    if let Some(mut object) = value.as_object_mut() {
        object.set_property("level", Value::long(3));
        object.set_property("code", Value::long(1));
        object.set_property("column", Value::long(error.column as i64));
        object.set_property("message", Value::string(format!("{}\n", error.message)));
        object.set_property("file", Value::string(""));
        object.set_property("line", Value::long(error.line as i64));
    }
    eg.push_libxml_error(value);
    if !eg.libxml_internal_errors() {
        let _ = super::report_internal_diagnostic(
            eg,
            ed,
            2,
            "Warning",
            &format!("{function}(): {}", error.message),
        )?;
    }
    Ok(())
}

fn throw_dom_error(eg: &mut ExecutorGlobals, message: &str) {
    eg.exception = Some(crate::value::make_error_value("DOMException", message));
}

fn fn_document_construct(
    ed: *mut ExecuteData,
    _rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let version = arg_opt!(ed, 1)
        .map(bytes)
        .unwrap_or_else(|| b"1.0".to_vec());
    let encoding = arg_opt!(ed, 2)
        .map(bytes)
        .filter(|encoding| !encoding.is_empty());
    let document = receiver(ed);
    let tree = Rc::new(RefCell::new(Tree::document(&version, encoding.as_deref())));
    set_state(
        &document,
        DomState {
            tree,
            handle: Handle::Node(0),
        },
    );
    let mut document = document;
    refresh_node_properties(eg, &mut document, true);
    Ok(())
}

fn fn_node_direct_construct(
    _ed: *mut ExecuteData,
    _rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    eg.exception = Some(crate::value::make_error_value(
        "Error",
        "Cannot directly construct DOMNode, use a subclass",
    ));
    Ok(())
}

fn fn_document_load_xml(
    ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let source = bytes(arg!(ed, 1));
    let document = receiver(ed);
    let preserve = document
        .as_object()
        .and_then(|object| object.get_property("preserveWhiteSpace").cloned())
        .is_none_or(|value| value.is_truthy());
    match parser::parse_document(&source, preserve) {
        Ok(tree) => {
            set_state(
                &document,
                DomState {
                    tree: Rc::new(RefCell::new(tree)),
                    handle: Handle::Node(0),
                },
            );
            let mut document = document;
            refresh_node_properties(eg, &mut document, true);
            super::write_return_value(rv, Value::bool(true));
        }
        Err(error) => {
            publish_parse_error(eg, ed, &error)?;
            super::write_return_value(rv, Value::bool(false));
        }
    }
    Ok(())
}

fn fn_document_load(
    ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let path = bytes(arg!(ed, 1));
    let path = path.strip_prefix(b"file://").unwrap_or(&path);
    match fs::read(std::path::Path::new(
        &String::from_utf8_lossy(path).into_owned(),
    )) {
        Ok(source) => {
            let document = receiver(ed);
            let preserve = document
                .as_object()
                .and_then(|object| object.get_property("preserveWhiteSpace").cloned())
                .is_none_or(|value| value.is_truthy());
            match parser::parse_document(&source, preserve) {
                Ok(mut tree) => {
                    tree.document_uri = Some(path.to_vec());
                    set_state(
                        &document,
                        DomState {
                            tree: Rc::new(RefCell::new(tree)),
                            handle: Handle::Node(0),
                        },
                    );
                    let mut document = document;
                    refresh_node_properties(eg, &mut document, true);
                    super::write_return_value(rv, Value::bool(true));
                }
                Err(error) => {
                    publish_parse_error(eg, ed, &error)?;
                    super::write_return_value(rv, Value::bool(false));
                }
            }
        }
        Err(_) => {
            let _ = super::report_internal_diagnostic(
                eg,
                ed,
                2,
                "Warning",
                "DOMDocument::load(): I/O warning : failed to load external entity",
            )?;
            super::write_return_value(rv, Value::bool(false));
        }
    }
    Ok(())
}

fn fn_document_save_xml(
    ed: *mut ExecuteData,
    rv: *mut Value,
    _eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let document = receiver(ed);
    let Some((tree, _)) = node_state(&document) else {
        super::write_return_value(rv, Value::bool(false));
        return Ok(());
    };
    let pretty = document
        .as_object()
        .and_then(|object| object.get_property("formatOutput").cloned())
        .is_some_and(|value| value.is_truthy());
    let options = arg_opt!(ed, 2).and_then(Value::as_long).unwrap_or(0);
    let output = if let Some(node) = arg_opt!(ed, 1)
        .filter(|value| value.value_type() != ValueType::Null)
        .and_then(node_state)
    {
        if !Rc::ptr_eq(&tree, &node.0) {
            super::write_return_value(rv, Value::bool(false));
            return Ok(());
        }
        tree.borrow().serialize_node_only(node.1, pretty, options)
    } else {
        tree.borrow().serialize_document(pretty, options)
    };
    super::write_return_value(rv, bytes_value(output));
    Ok(())
}

fn fn_document_save(
    ed: *mut ExecuteData,
    rv: *mut Value,
    _eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let document = receiver(ed);
    let Some((tree, _)) = node_state(&document) else {
        super::write_return_value(rv, Value::bool(false));
        return Ok(());
    };
    let path = bytes(arg!(ed, 1));
    let path = path.strip_prefix(b"file://").unwrap_or(&path);
    let options = arg_opt!(ed, 2).and_then(Value::as_long).unwrap_or(0);
    let pretty = document
        .as_object()
        .and_then(|object| object.get_property("formatOutput").cloned())
        .is_some_and(|value| value.is_truthy());
    let output = tree.borrow().serialize_document(pretty, options);
    let path = String::from_utf8_lossy(path).into_owned();
    match fs::write(path, &output) {
        Ok(()) => super::write_return_value(rv, Value::long(output.len() as i64)),
        Err(_) => super::write_return_value(rv, Value::bool(false)),
    }
    Ok(())
}

fn fn_document_create_element(
    ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    create_element(ed, rv, eg, false)
}

fn fn_document_create_element_ns(
    ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    create_element(ed, rv, eg, true)
}

fn create_element(
    ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
    namespaced: bool,
) -> Result<(), VmError> {
    let document = receiver(ed);
    let Some((tree, _)) = node_state(&document) else {
        return Ok(());
    };
    let name_index = if namespaced { 2 } else { 1 };
    let qualified_name = bytes(arg!(ed, name_index));
    if !valid_xml_name(&qualified_name) {
        throw_dom_error(eg, "Invalid Character Error");
        return Ok(());
    }
    let namespace = namespaced
        .then(|| bytes(arg!(ed, 1)))
        .filter(|namespace| !namespace.is_empty());
    let value_index = if namespaced { 3 } else { 2 };
    let value = arg_opt!(ed, value_index).map(bytes).unwrap_or_default();
    let (prefix, local_name) = split_name(&qualified_name);
    let node = {
        let mut tree_ref = tree.borrow_mut();
        let attributes = namespace
            .as_deref()
            .map(|namespace| vec![namespace_declaration(&prefix, namespace)])
            .unwrap_or_default();
        let node = tree_ref.add_node(NodeKind::Element {
            qualified_name,
            prefix,
            local_name,
            namespace,
            attributes,
        });
        if !value.is_empty() {
            let text = tree_ref.add_node(NodeKind::Text(value));
            tree_ref.append_child(node, text);
        }
        node
    };
    super::write_return_value(rv, wrap_node(eg, tree, node, true));
    Ok(())
}

fn fn_document_create_text(
    ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    create_character_node(ed, rv, eg, |data| NodeKind::Text(data))
}

fn fn_document_create_comment(
    ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    create_character_node(ed, rv, eg, |data| NodeKind::Comment(data))
}

fn fn_document_create_cdata(
    ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    create_character_node(ed, rv, eg, |data| NodeKind::Cdata(data))
}

fn create_character_node(
    ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
    kind: impl FnOnce(Vec<u8>) -> NodeKind,
) -> Result<(), VmError> {
    let document = receiver(ed);
    let Some((tree, _)) = node_state(&document) else {
        return Ok(());
    };
    let node = tree.borrow_mut().add_node(kind(bytes(arg!(ed, 1))));
    super::write_return_value(rv, wrap_node(eg, tree, node, true));
    Ok(())
}

fn fn_document_create_fragment(
    ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let document = receiver(ed);
    let Some((tree, _)) = node_state(&document) else {
        return Ok(());
    };
    let node = tree.borrow_mut().add_node(NodeKind::Fragment);
    super::write_return_value(rv, wrap_node(eg, tree, node, true));
    Ok(())
}

fn fn_document_create_attribute(
    ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let document = receiver(ed);
    let Some((tree, _)) = node_state(&document) else {
        return Ok(());
    };
    let name_index = if arg_opt!(ed, 2).is_some() { 2 } else { 1 };
    let qualified_name = bytes(arg!(ed, name_index));
    if !valid_xml_name(&qualified_name) {
        throw_dom_error(eg, "Invalid Character Error");
        return Ok(());
    }
    let namespace = (name_index == 2)
        .then(|| bytes(arg!(ed, 1)))
        .filter(|namespace| !namespace.is_empty());
    let (prefix, local_name) = split_name(&qualified_name);
    let holder = {
        let mut tree_ref = tree.borrow_mut();
        let holder = tree_ref.add_node(NodeKind::Element {
            qualified_name: b"__attribute_holder".to_vec(),
            prefix: Vec::new(),
            local_name: b"__attribute_holder".to_vec(),
            namespace: None,
            attributes: vec![Attribute {
                qualified_name,
                prefix,
                local_name,
                namespace,
                value: Vec::new(),
                is_id: false,
            }],
        });
        holder
    };
    super::write_return_value(rv, wrap_attribute(eg, tree, holder, 0, true));
    Ok(())
}

fn fn_document_create_pi(
    ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let document = receiver(ed);
    let Some((tree, _)) = node_state(&document) else {
        return Ok(());
    };
    let target = bytes(arg!(ed, 1));
    if !valid_xml_name(&target) {
        throw_dom_error(eg, "Invalid Character Error");
        return Ok(());
    }
    let data = arg_opt!(ed, 2).map(bytes).unwrap_or_default();
    let node = tree
        .borrow_mut()
        .add_node(NodeKind::ProcessingInstruction { target, data });
    super::write_return_value(rv, wrap_node(eg, tree, node, true));
    Ok(())
}

fn fn_document_get_elements(
    ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let document = receiver(ed);
    let Some((tree, node)) = node_state(&document) else {
        return Ok(());
    };
    let name = bytes(arg!(ed, 1));
    let nodes = tree.borrow().descendants_named(node, &name);
    super::write_return_value(rv, new_node_list(eg, tree, &nodes));
    Ok(())
}

fn fn_document_get_elements_ns(
    ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let document = receiver(ed);
    let Some((tree, node)) = node_state(&document) else {
        return Ok(());
    };
    let namespace = bytes(arg!(ed, 1));
    let local_name = bytes(arg!(ed, 2));
    let namespace = (!namespace.is_empty()).then_some(namespace);
    let nodes = tree
        .borrow()
        .descendants_named_ns(node, namespace.as_deref(), &local_name);
    super::write_return_value(rv, new_node_list(eg, tree, &nodes));
    Ok(())
}

fn fn_document_get_element_by_id(
    ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let document = receiver(ed);
    let Some((tree, _)) = node_state(&document) else {
        return Ok(());
    };
    let id = bytes(arg!(ed, 1));
    let found = {
        let tree_ref = tree.borrow();
        (0..tree_ref.nodes.len()).find(|node| {
            tree_ref.attributes(*node).is_some_and(|attributes| {
                attributes
                    .iter()
                    .any(|attribute| attribute.is_id && attribute.value == id)
            })
        })
    };
    super::write_return_value(
        rv,
        found.map_or_else(Value::null, |node| wrap_node(eg, tree, node, true)),
    );
    Ok(())
}

fn fn_document_normalize(
    ed: *mut ExecuteData,
    _rv: *mut Value,
    _eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let document = receiver(ed);
    if let Some((tree, node)) = node_state(&document) {
        tree.borrow_mut().normalize(node);
    }
    Ok(())
}

fn fn_document_schema_validate_source(
    ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let schema = bytes(arg!(ed, 1));
    if schema.is_empty() {
        eg.exception = Some(crate::value::make_error_value(
            "ValueError",
            "DOMDocument::schemaValidateSource(): Argument #1 ($source) must not be empty",
        ));
        return Ok(());
    }

    let schema = match parser::parse_document(&schema, true) {
        Ok(schema) => schema,
        Err(error) => {
            publish_libxml_error(eg, ed, &error, "DOMDocument::schemaValidateSource")?;
            super::write_return_value(rv, Value::bool(false));
            return Ok(());
        }
    };
    let document = receiver(ed);
    let valid = node_state(&document)
        .is_some_and(|(document, _)| schema_declares_document_root(&schema, &document.borrow()));
    if !valid {
        let root = node_state(&document)
            .and_then(|(tree, _)| {
                let tree = tree.borrow();
                tree.document_element()
                    .and_then(|root| tree.qualified_name(root).map(<[u8]>::to_vec))
            })
            .unwrap_or_default();
        let error = parser::ParseError {
            message: format!(
                "Element '{}': No matching global declaration available for the validation root.",
                String::from_utf8_lossy(&root)
            ),
            line: 1,
            column: 1,
        };
        publish_libxml_error(eg, ed, &error, "DOMDocument::schemaValidateSource")?;
    }
    super::write_return_value(rv, Value::bool(valid));
    Ok(())
}

fn schema_declares_document_root(schema: &Tree, document: &Tree) -> bool {
    const XSD_NAMESPACE: &[u8] = b"http://www.w3.org/2001/XMLSchema";
    let Some(schema_root) = schema.document_element() else {
        return false;
    };
    if schema.local_name(schema_root) != Some(b"schema")
        || schema.namespace(schema_root) != Some(XSD_NAMESPACE)
    {
        return false;
    }
    let Some(document_root) = document.document_element() else {
        return false;
    };
    let Some(document_name) = document.local_name(document_root) else {
        return false;
    };
    schema.nodes[schema_root].children.iter().any(|candidate| {
        schema.local_name(*candidate) == Some(b"element")
            && schema.namespace(*candidate) == Some(XSD_NAMESPACE)
            && schema.attributes(*candidate).is_some_and(|attributes| {
                attributes.iter().any(|attribute| {
                    attribute.qualified_name == b"name" && attribute.value == document_name
                })
            })
    })
}

fn sync_native_node_properties(value: &Value) {
    let Some((tree, node)) = node_state(value) else {
        return;
    };
    let data = value
        .as_object()
        .and_then(|object| object.get_property("data").cloned())
        .map(|value| bytes(&value));
    if let Some(data) = data {
        match &mut tree.borrow_mut().nodes[node].kind {
            NodeKind::ProcessingInstruction {
                data: native_data, ..
            }
            | NodeKind::Text(native_data)
            | NodeKind::Cdata(native_data)
            | NodeKind::Comment(native_data) => *native_data = data,
            _ => {}
        }
    }
}

fn fn_document_adopt_node(
    ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let document = receiver(ed);
    let mut node_value = arg!(ed, 1).clone();
    let Some((target, _)) = node_state(&document) else {
        return Ok(());
    };
    sync_native_node_properties(&node_value);
    let Some((source, source_node)) = node_state(&node_value) else {
        super::write_return_value(rv, Value::bool(false));
        return Ok(());
    };
    let adopted = if Rc::ptr_eq(&target, &source) {
        source.borrow_mut().detach(source_node);
        source_node
    } else {
        Tree::clone_subtree_into(&source, source_node, &mut target.borrow_mut(), true)
    };
    set_state(
        &node_value,
        DomState {
            tree: target,
            handle: Handle::Node(adopted),
        },
    );
    refresh_node_properties(eg, &mut node_value, true);
    super::write_return_value(rv, node_value);
    Ok(())
}

fn fn_document_import_node(
    ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let document = receiver(ed);
    let source_value = arg!(ed, 1).clone();
    let Some((target, _)) = node_state(&document) else {
        return Ok(());
    };
    sync_native_node_properties(&source_value);
    let Some((source, source_node)) = node_state(&source_value) else {
        super::write_return_value(rv, Value::bool(false));
        return Ok(());
    };
    let deep = arg_opt!(ed, 2).is_some_and(Value::is_truthy);
    let imported = Tree::clone_subtree_into(&source, source_node, &mut target.borrow_mut(), deep);
    super::write_return_value(rv, wrap_node(eg, target, imported, true));
    Ok(())
}

fn fn_node_append_child(
    ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let parent = receiver(ed);
    let mut child = arg!(ed, 1).clone();
    let Some((parent_tree, parent_node)) = node_state(&parent) else {
        return Ok(());
    };

    if let Some(attribute_state) = state(&child)
        && let Handle::Attribute { element, index } = attribute_state.handle
    {
        let attribute = attribute_state
            .tree
            .borrow()
            .attributes(element)
            .and_then(|attributes| attributes.get(index))
            .cloned();
        let Some(attribute) = attribute else {
            return Ok(());
        };
        let target_index = {
            let mut tree = parent_tree.borrow_mut();
            let Some(attributes) = tree.attributes_mut(parent_node) else {
                throw_dom_error(eg, "Hierarchy Request Error");
                return Ok(());
            };
            if let Some(index) = attributes
                .iter()
                .position(|candidate| candidate.qualified_name == attribute.qualified_name)
            {
                attributes[index] = attribute;
                index
            } else {
                let index = attributes.len();
                attributes.push(attribute);
                index
            }
        };
        set_state(
            &child,
            DomState {
                tree: Rc::clone(&parent_tree),
                handle: Handle::Attribute {
                    element: parent_node,
                    index: target_index,
                },
            },
        );
        refresh_attribute_properties(eg, &mut child, true);
        let mut parent = parent;
        refresh_node_properties(eg, &mut parent, true);
        super::write_return_value(rv, child);
        return Ok(());
    }

    let Some((child_tree, mut child_node)) = node_state(&child) else {
        return Ok(());
    };
    if !Rc::ptr_eq(&parent_tree, &child_tree) {
        if child_tree.borrow().owner_document(child_node).is_some() {
            throw_dom_error(eg, "Wrong Document Error");
            return Ok(());
        }
        child_node =
            Tree::clone_subtree_into(&child_tree, child_node, &mut parent_tree.borrow_mut(), true);
        set_state(
            &child,
            DomState {
                tree: Rc::clone(&parent_tree),
                handle: Handle::Node(child_node),
            },
        );
    }
    if !parent_tree
        .borrow_mut()
        .append_child(parent_node, child_node)
    {
        throw_dom_error(eg, "Hierarchy Request Error");
        return Ok(());
    }
    refresh_node_properties(eg, &mut child, true);
    let mut parent = parent;
    refresh_node_properties(eg, &mut parent, true);
    super::write_return_value(rv, child);
    Ok(())
}

fn fn_node_remove_child(
    ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let parent = receiver(ed);
    let child = arg!(ed, 1).clone();
    let Some((parent_tree, parent_node)) = node_state(&parent) else {
        return Ok(());
    };
    let Some((child_tree, child_node)) = node_state(&child) else {
        return Ok(());
    };
    if !Rc::ptr_eq(&parent_tree, &child_tree)
        || !parent_tree
            .borrow_mut()
            .remove_child(parent_node, child_node)
    {
        throw_dom_error(eg, "Not Found Error");
        return Ok(());
    }
    let mut parent = parent;
    refresh_node_properties(eg, &mut parent, true);
    let mut child = child;
    refresh_node_properties(eg, &mut child, true);
    super::write_return_value(rv, child);
    Ok(())
}

fn fn_node_insert_before(
    ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let parent = receiver(ed);
    let child = arg!(ed, 1).clone();
    let Some((tree, parent_node)) = node_state(&parent) else {
        return Ok(());
    };
    let Some((child_tree, child_node)) = node_state(&child) else {
        return Ok(());
    };
    let reference = arg_opt!(ed, 2)
        .filter(|value| value.value_type() != ValueType::Null)
        .and_then(node_state);
    if !Rc::ptr_eq(&tree, &child_tree)
        || reference
            .as_ref()
            .is_some_and(|reference| !Rc::ptr_eq(&tree, &reference.0))
        || !tree.borrow_mut().insert_before(
            parent_node,
            child_node,
            reference.as_ref().map(|reference| reference.1),
        )
    {
        throw_dom_error(eg, "Not Found Error");
        return Ok(());
    }
    let mut parent = parent;
    refresh_node_properties(eg, &mut parent, true);
    let mut child = child;
    refresh_node_properties(eg, &mut child, true);
    super::write_return_value(rv, child);
    Ok(())
}

fn fn_node_replace_child(
    ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let parent = receiver(ed);
    let replacement = arg!(ed, 1).clone();
    let previous = arg!(ed, 2).clone();
    let Some((tree, parent_node)) = node_state(&parent) else {
        return Ok(());
    };
    let Some((replacement_tree, replacement_node)) = node_state(&replacement) else {
        return Ok(());
    };
    let Some((previous_tree, previous_node)) = node_state(&previous) else {
        return Ok(());
    };
    if !Rc::ptr_eq(&tree, &replacement_tree)
        || !Rc::ptr_eq(&tree, &previous_tree)
        || !tree
            .borrow_mut()
            .replace_child(parent_node, replacement_node, previous_node)
    {
        throw_dom_error(eg, "Not Found Error");
        return Ok(());
    }
    let mut parent = parent;
    refresh_node_properties(eg, &mut parent, true);
    let mut replacement = replacement;
    refresh_node_properties(eg, &mut replacement, true);
    let mut previous = previous;
    refresh_node_properties(eg, &mut previous, true);
    super::write_return_value(rv, previous);
    Ok(())
}

fn fn_node_has_children(
    ed: *mut ExecuteData,
    rv: *mut Value,
    _eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let result = node_state(&receiver(ed))
        .is_some_and(|(tree, node)| !tree.borrow().nodes[node].children.is_empty());
    super::write_return_value(rv, Value::bool(result));
    Ok(())
}

fn fn_node_has_attributes(
    ed: *mut ExecuteData,
    rv: *mut Value,
    _eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let result = node_state(&receiver(ed)).is_some_and(|(tree, node)| {
        tree.borrow()
            .attributes(node)
            .is_some_and(|attributes| !attributes.is_empty())
    });
    super::write_return_value(rv, Value::bool(result));
    Ok(())
}

fn fn_node_normalize(
    ed: *mut ExecuteData,
    _rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    if let Some((tree, node)) = node_state(&receiver(ed)) {
        tree.borrow_mut().normalize(node);
        let mut receiver = receiver(ed);
        refresh_node_properties(eg, &mut receiver, true);
    }
    Ok(())
}

fn fn_node_clone(
    ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let Some((source, node)) = node_state(&receiver(ed)) else {
        return Ok(());
    };
    let deep = arg_opt!(ed, 1).is_some_and(Value::is_truthy);
    let tree = Rc::new(RefCell::new(Tree::detached()));
    let cloned = Tree::clone_subtree_into(&source, node, &mut tree.borrow_mut(), deep);
    super::write_return_value(rv, wrap_node(eg, tree, cloned, true));
    Ok(())
}

fn fn_node_c14n(
    ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let Some((tree, node)) = node_state(&receiver(ed)) else {
        super::write_return_value(rv, Value::bool(false));
        return Ok(());
    };
    if let Some(xpath) = arg_opt!(ed, 3).filter(|value| value.value_type() != ValueType::Null) {
        let Some(xpath) = xpath.as_array() else {
            return Ok(());
        };
        let Some(query) = xpath.get_str("query") else {
            eg.exception = Some(crate::value::make_error_value(
                "ValueError",
                "DOMNode::C14N(): Argument #3 ($xpath) must have a \"query\" key",
            ));
            return Ok(());
        };
        if query.value_type() != ValueType::String {
            eg.exception = Some(crate::value::make_error_value(
                "TypeError",
                &format!(
                    "DOMNode::C14N(): Argument #3 ($xpath) \"query\" option must be a string, {} given",
                    query.type_name()
                ),
            ));
            return Ok(());
        }
    }
    let with_comments = arg_opt!(ed, 2).is_some_and(Value::is_truthy);
    super::write_return_value(
        rv,
        bytes_value(tree.borrow().canonicalize(node, with_comments)),
    );
    Ok(())
}

fn fn_element_construct(
    ed: *mut ExecuteData,
    _rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let qualified_name = bytes(arg!(ed, 1));
    if !valid_xml_name(&qualified_name) {
        throw_dom_error(eg, "Invalid Character Error");
        return Ok(());
    }
    let value = arg_opt!(ed, 2)
        .filter(|value| value.value_type() != ValueType::Null)
        .map(bytes)
        .unwrap_or_default();
    let namespace = arg_opt!(ed, 3)
        .map(bytes)
        .filter(|namespace| !namespace.is_empty());
    let (prefix, local_name) = split_name(&qualified_name);
    let attributes = namespace
        .as_deref()
        .map(|namespace| vec![namespace_declaration(&prefix, namespace)])
        .unwrap_or_default();
    let mut tree = Tree::detached();
    let node = tree.add_node(NodeKind::Element {
        qualified_name,
        prefix,
        local_name,
        namespace,
        attributes,
    });
    if !value.is_empty() {
        let text = tree.add_node(NodeKind::Text(value));
        tree.append_child(node, text);
    }
    let receiver = receiver(ed);
    let tree = Rc::new(RefCell::new(tree));
    set_state(
        &receiver,
        DomState {
            tree,
            handle: Handle::Node(node),
        },
    );
    let mut receiver = receiver;
    refresh_node_properties(eg, &mut receiver, true);
    Ok(())
}

fn fn_character_construct(
    ed: *mut ExecuteData,
    _rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let class = receiver(ed)
        .as_object()
        .map(|object| object.class_name.clone())
        .unwrap_or_default();
    let data = arg_opt!(ed, 1).map(bytes).unwrap_or_default();
    let kind = if class.eq_ignore_ascii_case(DOM_TEXT) {
        NodeKind::Text(data)
    } else if class.eq_ignore_ascii_case(DOM_CDATA) {
        NodeKind::Cdata(data)
    } else {
        NodeKind::Comment(data)
    };
    let mut tree = Tree::detached();
    let node = tree.add_node(kind);
    let receiver = receiver(ed);
    let tree = Rc::new(RefCell::new(tree));
    set_state(
        &receiver,
        DomState {
            tree,
            handle: Handle::Node(node),
        },
    );
    let mut receiver = receiver;
    refresh_node_properties(eg, &mut receiver, true);
    Ok(())
}

fn fn_fragment_construct(
    ed: *mut ExecuteData,
    _rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let mut tree = Tree::detached();
    let node = tree.add_node(NodeKind::Fragment);
    let receiver = receiver(ed);
    let tree = Rc::new(RefCell::new(tree));
    set_state(
        &receiver,
        DomState {
            tree,
            handle: Handle::Node(node),
        },
    );
    let mut receiver = receiver;
    refresh_node_properties(eg, &mut receiver, true);
    Ok(())
}

fn fn_pi_construct(
    ed: *mut ExecuteData,
    _rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let target = bytes(arg!(ed, 1));
    if !valid_xml_name(&target) {
        throw_dom_error(eg, "Invalid Character Error");
        return Ok(());
    }
    let data = arg_opt!(ed, 2).map(bytes).unwrap_or_default();
    let mut tree = Tree::detached();
    let node = tree.add_node(NodeKind::ProcessingInstruction { target, data });
    let receiver = receiver(ed);
    let tree = Rc::new(RefCell::new(tree));
    set_state(
        &receiver,
        DomState {
            tree,
            handle: Handle::Node(node),
        },
    );
    let mut receiver = receiver;
    refresh_node_properties(eg, &mut receiver, true);
    Ok(())
}

fn fn_attr_construct(
    ed: *mut ExecuteData,
    _rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let qualified_name = bytes(arg!(ed, 1));
    if !valid_xml_name(&qualified_name) {
        throw_dom_error(eg, "Invalid Character Error");
        return Ok(());
    }
    let value = arg_opt!(ed, 2).map(bytes).unwrap_or_default();
    let (prefix, local_name) = split_name(&qualified_name);
    let mut tree = Tree::detached();
    let holder = tree.add_node(NodeKind::Element {
        qualified_name: b"__attribute_holder".to_vec(),
        prefix: Vec::new(),
        local_name: b"__attribute_holder".to_vec(),
        namespace: None,
        attributes: vec![Attribute {
            qualified_name,
            prefix,
            local_name,
            namespace: None,
            value,
            is_id: false,
        }],
    });
    let receiver = receiver(ed);
    let tree = Rc::new(RefCell::new(tree));
    set_state(
        &receiver,
        DomState {
            tree,
            handle: Handle::Attribute {
                element: holder,
                index: 0,
            },
        },
    );
    let mut receiver = receiver;
    refresh_attribute_properties(eg, &mut receiver, true);
    Ok(())
}

fn fn_element_get_attribute(
    ed: *mut ExecuteData,
    rv: *mut Value,
    _eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let name = bytes(arg!(ed, 1));
    let value = node_state(&receiver(ed)).and_then(|(tree, node)| {
        tree.borrow()
            .attributes(node)?
            .iter()
            .find(|attribute| attribute.qualified_name == name)
            .map(|attribute| attribute.value.clone())
    });
    super::write_return_value(rv, bytes_value(value.unwrap_or_default()));
    Ok(())
}

fn fn_element_get_attribute_ns(
    ed: *mut ExecuteData,
    rv: *mut Value,
    _eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let namespace = bytes(arg!(ed, 1));
    let local_name = bytes(arg!(ed, 2));
    let namespace = (!namespace.is_empty()).then_some(namespace);
    let value = node_state(&receiver(ed)).and_then(|(tree, node)| {
        tree.borrow()
            .attributes(node)?
            .iter()
            .find(|attribute| {
                attribute.namespace.as_deref() == namespace.as_deref()
                    && attribute.local_name == local_name
            })
            .map(|attribute| attribute.value.clone())
    });
    super::write_return_value(rv, bytes_value(value.unwrap_or_default()));
    Ok(())
}

fn fn_element_has_attribute(
    ed: *mut ExecuteData,
    rv: *mut Value,
    _eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let name = bytes(arg!(ed, 1));
    let found = node_state(&receiver(ed)).is_some_and(|(tree, node)| {
        tree.borrow().attributes(node).is_some_and(|attributes| {
            attributes
                .iter()
                .any(|attribute| attribute.qualified_name == name)
        })
    });
    super::write_return_value(rv, Value::bool(found));
    Ok(())
}

fn fn_element_has_attribute_ns(
    ed: *mut ExecuteData,
    rv: *mut Value,
    _eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let namespace = bytes(arg!(ed, 1));
    let local_name = bytes(arg!(ed, 2));
    let namespace = (!namespace.is_empty()).then_some(namespace);
    let found = node_state(&receiver(ed)).is_some_and(|(tree, node)| {
        tree.borrow().attributes(node).is_some_and(|attributes| {
            attributes.iter().any(|attribute| {
                attribute.namespace.as_deref() == namespace.as_deref()
                    && attribute.local_name == local_name
            })
        })
    });
    super::write_return_value(rv, Value::bool(found));
    Ok(())
}

fn fn_element_set_attribute(
    ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    set_attribute(ed, rv, eg, false)
}

fn fn_element_set_attribute_ns(
    ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    set_attribute(ed, rv, eg, true)
}

fn set_attribute(
    ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
    namespaced: bool,
) -> Result<(), VmError> {
    let element = receiver(ed);
    let Some((tree, node)) = node_state(&element) else {
        return Ok(());
    };
    let name_index = if namespaced { 2 } else { 1 };
    let value_index = if namespaced { 3 } else { 2 };
    let qualified_name = bytes(arg!(ed, name_index));
    if !valid_xml_name(&qualified_name) {
        throw_dom_error(eg, "Invalid Character Error");
        return Ok(());
    }
    let value = bytes(arg!(ed, value_index));
    let namespace = namespaced
        .then(|| bytes(arg!(ed, 1)))
        .filter(|namespace| !namespace.is_empty());
    let (prefix, local_name) = split_name(&qualified_name);
    let index = {
        let mut tree_ref = tree.borrow_mut();
        let Some(attributes) = tree_ref.attributes_mut(node) else {
            return Ok(());
        };
        if let Some((index, attribute)) =
            attributes.iter_mut().enumerate().find(|(_, attribute)| {
                if namespaced {
                    attribute.namespace.as_deref() == namespace.as_deref()
                        && attribute.local_name == local_name
                } else {
                    attribute.qualified_name == qualified_name
                }
            })
        {
            attribute.value = value;
            index
        } else {
            if let (Some(namespace), false) = (
                namespace.as_deref(),
                prefix.is_empty()
                    || attributes.iter().any(|attribute| {
                        attribute.prefix == b"xmlns" && attribute.local_name == prefix
                    }),
            ) {
                attributes.push(namespace_declaration(&prefix, namespace));
            }
            let index = attributes.len();
            attributes.push(Attribute {
                is_id: qualified_name.eq_ignore_ascii_case(b"id"),
                qualified_name,
                prefix,
                local_name,
                namespace,
                value,
            });
            index
        }
    };
    let mut element = element;
    refresh_node_properties(eg, &mut element, true);
    super::write_return_value(rv, wrap_attribute(eg, tree, node, index, true));
    Ok(())
}

fn fn_element_remove_attribute(
    ed: *mut ExecuteData,
    _rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let element = receiver(ed);
    let name = bytes(arg!(ed, 1));
    let Some((tree, node)) = node_state(&element) else {
        return Ok(());
    };
    if let Some(attributes) = tree.borrow_mut().attributes_mut(node)
        && let Some(position) = attributes
            .iter()
            .position(|attribute| attribute.qualified_name == name)
    {
        attributes.remove(position);
    }
    let mut element = element;
    refresh_node_properties(eg, &mut element, true);
    Ok(())
}

fn fn_element_get_attribute_names(
    ed: *mut ExecuteData,
    rv: *mut Value,
    _eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let mut result = PhpArray::new();
    if let Some((tree, node)) = node_state(&receiver(ed))
        && let Some(attributes) = tree.borrow().attributes(node)
    {
        for attribute in attributes {
            result.push(bytes_value(attribute.qualified_name.clone()));
        }
    }
    super::write_return_value(rv, Value::array(result));
    Ok(())
}

fn fn_element_get_attribute_node(
    ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let name = bytes(arg!(ed, 1));
    let Some((tree, node)) = node_state(&receiver(ed)) else {
        super::write_return_value(rv, Value::bool(false));
        return Ok(());
    };
    let index = tree.borrow().attributes(node).and_then(|attributes| {
        attributes
            .iter()
            .position(|attribute| attribute.qualified_name == name)
    });
    super::write_return_value(
        rv,
        index.map_or_else(
            || Value::bool(false),
            |index| wrap_attribute(eg, tree, node, index, true),
        ),
    );
    Ok(())
}

fn fn_element_get_elements(
    ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    fn_document_get_elements(ed, rv, eg)
}

fn fn_element_get_elements_ns(
    ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    fn_document_get_elements_ns(ed, rv, eg)
}

fn fn_element_append(
    ed: *mut ExecuteData,
    _rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    append_variadic_nodes(ed, eg, false)
}

fn fn_element_prepend(
    ed: *mut ExecuteData,
    _rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    append_variadic_nodes(ed, eg, true)
}

fn append_variadic_nodes(
    ed: *mut ExecuteData,
    eg: &mut ExecutorGlobals,
    prepend: bool,
) -> Result<(), VmError> {
    let parent = receiver(ed);
    let Some((tree, parent_node)) = node_state(&parent) else {
        return Ok(());
    };
    let arguments = variadic_arguments(ed);
    let mut inserted = Vec::new();
    for argument in arguments {
        if let Some((other_tree, node)) = node_state(&argument) {
            if !Rc::ptr_eq(&tree, &other_tree) {
                throw_dom_error(eg, "Wrong Document Error");
                return Ok(());
            }
            inserted.push(node);
        } else {
            let text = tree.borrow_mut().add_node(NodeKind::Text(bytes(&argument)));
            inserted.push(text);
        }
    }
    if prepend {
        for node in inserted.into_iter().rev() {
            let reference = tree.borrow().nodes[parent_node].children.first().copied();
            tree.borrow_mut()
                .insert_before(parent_node, node, reference);
        }
    } else {
        for node in inserted {
            tree.borrow_mut().append_child(parent_node, node);
        }
    }
    Ok(())
}

fn variadic_arguments(ed: *mut ExecuteData) -> Vec<Value> {
    // Variadic internal methods store their collected public argument array in
    // CV 1, matching the existing Closure/SPL internal call convention.
    arg_opt!(ed, 1)
        .and_then(Value::as_array)
        .map(|array| array.iter().map(|(_, value)| value.clone()).collect())
        .unwrap_or_default()
}

fn fn_fragment_append_xml(
    ed: *mut ExecuteData,
    rv: *mut Value,
    _eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let fragment = receiver(ed);
    let Some((tree, node)) = node_state(&fragment) else {
        return Ok(());
    };
    let source = bytes(arg!(ed, 1));
    let mut wrapper = b"<fragment>".to_vec();
    wrapper.extend_from_slice(&source);
    wrapper.extend_from_slice(b"</fragment>");
    let Ok(parsed) = parser::parse_document(&wrapper, true) else {
        super::write_return_value(rv, Value::bool(false));
        return Ok(());
    };
    let parsed = Rc::new(RefCell::new(parsed));
    let root = parsed.borrow().document_element().unwrap();
    let children = parsed.borrow().nodes[root].children.clone();
    for child in children {
        let copied = Tree::clone_subtree_into(&parsed, child, &mut tree.borrow_mut(), true);
        tree.borrow_mut().append_child(node, copied);
    }
    super::write_return_value(rv, Value::bool(true));
    Ok(())
}

fn fn_attr_is_id(
    ed: *mut ExecuteData,
    rv: *mut Value,
    _eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let result = state(&receiver(ed)).is_some_and(|state| match state.handle {
        Handle::Attribute { element, index } => state
            .tree
            .borrow()
            .attributes(element)
            .and_then(|attributes| attributes.get(index))
            .is_some_and(|attribute| attribute.is_id),
        Handle::Node(_) => false,
    });
    super::write_return_value(rv, Value::bool(result));
    Ok(())
}

fn fn_list_count(
    ed: *mut ExecuteData,
    rv: *mut Value,
    _eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let count = receiver(ed)
        .as_object()
        .and_then(|object| {
            object
                .native_object_state::<DomListState>()
                .map(|state| match &state.live {
                    Some(DomListSource::Children { tree, parent }) => {
                        tree.borrow().nodes[*parent].children.len()
                    }
                    Some(DomListSource::Attributes { tree, element }) => tree
                        .borrow()
                        .attributes(*element)
                        .map_or(0, <[Attribute]>::len),
                    None => state.values.len(),
                })
        })
        .unwrap_or(0);
    super::write_return_value(rv, Value::long(count as i64));
    Ok(())
}

fn fn_list_item(
    ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let index = arg!(ed, 1).as_long().unwrap_or(-1);
    let state = receiver(ed)
        .as_object()
        .and_then(|object| object.native_object_state::<DomListState>().cloned());
    let value = if index < 0 {
        Value::null()
    } else {
        state
            .as_ref()
            .and_then(|state| list_value(eg, state, index as usize))
            .unwrap_or_else(Value::null)
    };
    super::write_return_value(rv, value);
    Ok(())
}

fn fn_list_iterator(
    ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let state = receiver(ed)
        .as_object()
        .and_then(|object| object.native_object_state::<DomListState>().cloned());
    let values = state
        .as_ref()
        .map(|state| list_values(eg, state))
        .unwrap_or_default();
    let mut array = PhpArray::with_packed_capacity(values.len());
    for value in values {
        array.push(value);
    }
    let iterator = super::builtin_classes::array_object::iterator_from_array(eg, array)
        .unwrap_or_else(Value::null);
    super::write_return_value(rv, iterator);
    Ok(())
}

fn fn_named_item(
    ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let name = bytes(arg!(ed, 1));
    let state = receiver(ed)
        .as_object()
        .and_then(|object| object.native_object_state::<DomListState>().cloned());
    let value = state
        .as_ref()
        .and_then(|state| {
            list_values(eg, state).into_iter().find(|value| {
                value
                    .as_object()
                    .and_then(|object| object.get_property("name").cloned())
                    .is_some_and(|candidate| bytes(&candidate) == name)
            })
        })
        .unwrap_or_else(Value::null);
    super::write_return_value(rv, value);
    Ok(())
}

fn list_value(eg: &ExecutorGlobals, state: &DomListState, index: usize) -> Option<Value> {
    match &state.live {
        Some(DomListSource::Children { tree, parent }) => {
            let node = tree.borrow().nodes[*parent].children.get(index).copied()?;
            Some(wrap_node(eg, Rc::clone(tree), node, true))
        }
        Some(DomListSource::Attributes { tree, element }) => (index
            < tree.borrow().attributes(*element)?.len())
        .then(|| wrap_attribute(eg, Rc::clone(tree), *element, index, true)),
        None => state.values.get(index).cloned(),
    }
}

fn list_values(eg: &ExecutorGlobals, state: &DomListState) -> Vec<Value> {
    let length = match &state.live {
        Some(DomListSource::Children { tree, parent }) => {
            tree.borrow().nodes[*parent].children.len()
        }
        Some(DomListSource::Attributes { tree, element }) => tree
            .borrow()
            .attributes(*element)
            .map_or(0, <[Attribute]>::len),
        None => return state.values.clone(),
    };
    (0..length)
        .filter_map(|index| list_value(eg, state, index))
        .collect()
}

fn fn_xpath_construct(
    ed: *mut ExecuteData,
    _rv: *mut Value,
    _eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let document = arg!(ed, 1).clone();
    let Some((tree, _)) = node_state(&document) else {
        return Ok(());
    };
    let register_node_namespaces = arg_opt!(ed, 2).is_none_or(Value::is_truthy);
    let receiver = receiver(ed);
    if let Some(mut object) = receiver.as_object_mut() {
        *object.native_object_state_mut::<XPathState>() = XPathState {
            tree,
            document: document.clone(),
            register_node_namespaces,
            namespaces: Vec::new(),
        };
        object.set_property("document", document);
        object.set_property(
            "registerNodeNamespaces",
            Value::bool(register_node_namespaces),
        );
    }
    Ok(())
}

fn fn_xpath_query(
    ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let expression = bytes(arg!(ed, 1));
    let context = arg_opt!(ed, 2)
        .filter(|value| value.value_type() != ValueType::Null)
        .and_then(node_state);
    let Some(state) = receiver(ed)
        .as_object()
        .and_then(|object| object.native_object_state::<XPathState>().cloned())
    else {
        super::write_return_value(rv, Value::bool(false));
        return Ok(());
    };
    if context
        .as_ref()
        .is_some_and(|context| !Rc::ptr_eq(&state.tree, &context.0))
    {
        super::write_return_value(rv, Value::bool(false));
        return Ok(());
    }
    let context = context.map_or(0, |context| context.1);
    let _register_node_namespaces = state.register_node_namespaces;
    let nodes = {
        let tree = state.tree.borrow();
        xpath::query(&tree, context, &expression)
    };
    super::write_return_value(
        rv,
        nodes.map_or_else(
            || Value::bool(false),
            |nodes| new_node_list(eg, state.tree, &nodes),
        ),
    );
    Ok(())
}

fn fn_xpath_evaluate(
    ed: *mut ExecuteData,
    rv: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    fn_xpath_query(ed, rv, eg)
}

fn fn_xpath_register_namespace(
    ed: *mut ExecuteData,
    rv: *mut Value,
    _eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let prefix = bytes(arg!(ed, 1));
    let namespace = bytes(arg!(ed, 2));
    let receiver = receiver(ed);
    if let Some(mut object) = receiver.as_object_mut() {
        let state = object.native_object_state_mut::<XPathState>();
        if let Some((_, value)) = state
            .namespaces
            .iter_mut()
            .find(|(candidate, _)| *candidate == prefix)
        {
            *value = namespace;
        } else {
            state.namespaces.push((prefix, namespace));
        }
    }
    super::write_return_value(rv, Value::bool(true));
    Ok(())
}

fn fn_xpath_quote(
    ed: *mut ExecuteData,
    rv: *mut Value,
    _eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    let value = bytes(arg!(ed, 1));
    let output = if !value.contains(&b'\'') {
        let mut output = vec![b'\''];
        output.extend_from_slice(&value);
        output.push(b'\'');
        output
    } else if !value.contains(&b'"') {
        let mut output = vec![b'"'];
        output.extend_from_slice(&value);
        output.push(b'"');
        output
    } else {
        let mut output = b"concat(".to_vec();
        let mut offset = 0;
        while offset < value.len() {
            let remaining = &value[offset..];
            let single = remaining
                .iter()
                .position(|byte| *byte == b'\'')
                .unwrap_or(remaining.len());
            let double = remaining
                .iter()
                .position(|byte| *byte == b'"')
                .unwrap_or(remaining.len());
            let length = single.max(double);
            let quote = if single > double { b'\'' } else { b'"' };
            output.push(quote);
            output.extend_from_slice(&remaining[..length]);
            output.push(quote);
            output.push(b',');
            offset += length;
        }
        *output
            .last_mut()
            .expect("concat expression has a separator") = b')';
        output
    };
    super::write_return_value(rv, bytes_value(output));
    Ok(())
}

fn nullable_class(name: &str) -> ParamTypeHint {
    ParamTypeHint::Nullable(Box::new(ParamTypeHint::ClassName(name.to_string())))
}

fn nullable_string() -> ParamTypeHint {
    ParamTypeHint::Nullable(Box::new(ParamTypeHint::String))
}

fn internal_class(
    name: &str,
    parent: Option<&str>,
    implements: &[&str],
    is_interface: bool,
    is_final: bool,
    properties: Vec<PropertyDefinition>,
    constants: Vec<ClassConstantDefinition>,
) -> ClassDef {
    ClassDef {
        attributes: Vec::new(),
        name: name.to_string(),
        source_file: None,
        declaration_line: 0,
        end_line: 0,
        doc_comment: None,
        parent: parent.map(str::to_string),
        implements: implements.iter().map(|name| (*name).to_string()).collect(),
        is_interface,
        is_abstract: false,
        is_final,
        is_trait: false,
        is_enum: false,
        is_readonly: false,
        allow_dynamic_properties: false,
        uses: Vec::new(),
        trait_aliases: Vec::new(),
        trait_precedences: Vec::new(),
        properties,
        static_properties: Vec::new(),
        constants,
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

fn property(owner: &str, name: &str, default: Value, hint: ParamTypeHint) -> PropertyDefinition {
    PropertyDefinition::declared(
        name.to_string(),
        Some(default),
        Visibility::Public,
        owner.to_string(),
        hint,
        false,
        false,
    )
}

fn class_constant(owner: &str, name: &str, value: i64) -> ClassConstantDefinition {
    ClassConstantDefinition {
        attributes: Vec::new(),
        name: name.to_string(),
        value: Value::long(value),
        source_file: String::new(),
        evaluation_error: None,
        source_expression: None,
        callable_factory: None,
        evaluation_scope: None,
        value_is_deferred: false,
        visibility: Visibility::Public,
        declaring_class: owner.to_string(),
        type_hint: ParamTypeHint::Int,
        is_final: false,
    }
}

struct Method {
    owner: &'static str,
    name: &'static str,
    handler: InternalFunctionHandler,
    parameters: &'static [&'static str],
    required: u32,
    defaults: Vec<Option<Value>>,
    default_spellings: Vec<Option<&'static str>>,
    hints: Vec<ParamTypeHint>,
    result: ParamTypeHint,
    is_static: bool,
    variadic: bool,
}

#[allow(clippy::too_many_arguments)]
fn method(
    owner: &'static str,
    name: &'static str,
    handler: InternalFunctionHandler,
    parameters: &'static [&'static str],
    required: u32,
    defaults: Vec<Option<Value>>,
    default_spellings: Vec<Option<&'static str>>,
    hints: Vec<ParamTypeHint>,
    result: ParamTypeHint,
) -> Method {
    Method {
        owner,
        name,
        handler,
        parameters,
        required,
        defaults,
        default_spellings,
        hints,
        result,
        is_static: false,
        variadic: false,
    }
}

fn register_method(
    eg: &mut ExecutorGlobals,
    functions: &mut Vec<Box<InternalFunction>>,
    declaration: Method,
) {
    let reflected_result = match (declaration.owner, declaration.name) {
        (DOM_ELEMENT, "getAttributeNames")
        | (DOM_ELEMENT | DOM_FRAGMENT, "append" | "prepend")
        | (DOM_NODE_LIST | DOM_NAMED_NODE_MAP, "getIterator")
        | (DOM_XPATH, "quote") => declaration.result.clone(),
        _ => ParamTypeHint::None,
    };
    eg.register_internal_method_contract(
        declaration.owner,
        declaration.name,
        declaration.is_static,
        declaration.required,
        declaration.parameters,
        declaration.hints.clone(),
        reflected_result.clone(),
        &declaration.default_spellings,
        declaration.variadic,
    );
    let mut function = if declaration.variadic {
        Box::new(make_internal_method_variadic(
            declaration.handler,
            declaration.required,
            declaration
                .parameters
                .iter()
                .map(|name| (*name).to_string())
                .collect(),
        ))
    } else {
        Box::new(
            make_internal_method(
                declaration.handler,
                declaration.parameters.len() as u32 + 1,
                declaration.required,
                Vec::new(),
            )
            .with_static_parameter_names(declaration.parameters),
        )
    };
    function.handler_validates_types = true;
    function.common.sig.param_type_hints = declaration.hints;
    function.common.sig.return_type_hint = reflected_result;
    let pointer = &function.common as *const FunctionCommon;
    eg.insert_function_entry(
        super::builtin_classes::internal_method_lookup_name(declaration.owner, declaration.name),
        pointer,
    );
    eg.method_declaring_class
        .insert(pointer, declaration.owner.to_string().into());
    if declaration.is_static {
        eg.register_internal_static_method(pointer);
    }
    eg.register_internal_function_display_name(
        pointer,
        super::builtin_classes::internal_method_display_name(declaration.owner, declaration.name),
    );
    eg.register_internal_function_reflection_metadata(pointer, declaration.defaults, "dom");
    functions.push(function);
}

pub(super) fn register_classes(eg: &mut ExecutorGlobals) -> Vec<Box<InternalFunction>> {
    for interface in ["DOMParentNode", "DOMChildNode"] {
        eg.register_class(internal_class(
            interface,
            None,
            &[],
            true,
            false,
            Vec::new(),
            Vec::new(),
        ))
        .expect("DOM interface registers once per request");
    }
    eg.register_class(internal_class(
        "DOMException",
        Some("Exception"),
        &[],
        false,
        true,
        Vec::new(),
        Vec::new(),
    ))
    .expect("DOMException registers once per request");

    let node_properties = [
        ("nodeName", Value::string(""), ParamTypeHint::String),
        ("nodeValue", Value::null(), nullable_string()),
        ("nodeType", Value::long(0), ParamTypeHint::Int),
        ("parentNode", Value::null(), nullable_class(DOM_NODE)),
        ("parentElement", Value::null(), nullable_class(DOM_ELEMENT)),
        (
            "childNodes",
            Value::null(),
            ParamTypeHint::ClassName(DOM_NODE_LIST.into()),
        ),
        ("firstChild", Value::null(), nullable_class(DOM_NODE)),
        ("lastChild", Value::null(), nullable_class(DOM_NODE)),
        ("previousSibling", Value::null(), nullable_class(DOM_NODE)),
        ("nextSibling", Value::null(), nullable_class(DOM_NODE)),
        (
            "attributes",
            Value::null(),
            nullable_class(DOM_NAMED_NODE_MAP),
        ),
        ("isConnected", Value::bool(false), ParamTypeHint::Bool),
        ("ownerDocument", Value::null(), nullable_class(DOM_DOCUMENT)),
        ("namespaceURI", Value::null(), nullable_string()),
        ("prefix", Value::string(""), ParamTypeHint::String),
        ("localName", Value::null(), nullable_string()),
        ("baseURI", Value::null(), nullable_string()),
        ("textContent", Value::string(""), ParamTypeHint::String),
    ]
    .into_iter()
    .map(|(name, default, hint)| property(DOM_NODE, name, default, hint))
    .collect();
    let node_constants = [
        ("DOCUMENT_POSITION_DISCONNECTED", 1),
        ("DOCUMENT_POSITION_PRECEDING", 2),
        ("DOCUMENT_POSITION_FOLLOWING", 4),
        ("DOCUMENT_POSITION_CONTAINS", 8),
        ("DOCUMENT_POSITION_CONTAINED_BY", 16),
        ("DOCUMENT_POSITION_IMPLEMENTATION_SPECIFIC", 32),
    ]
    .into_iter()
    .map(|(name, value)| class_constant(DOM_NODE, name, value))
    .collect();
    eg.register_class(internal_class(
        DOM_NODE,
        None,
        &[],
        false,
        false,
        node_properties,
        node_constants,
    ))
    .expect("DOMNode registers once per request");

    let document_properties = [
        ("doctype", Value::null(), nullable_class(DOM_DOCUMENT_TYPE)),
        (
            "implementation",
            Value::null(),
            ParamTypeHint::ClassName(DOM_IMPLEMENTATION.into()),
        ),
        (
            "documentElement",
            Value::null(),
            nullable_class(DOM_ELEMENT),
        ),
        ("actualEncoding", Value::null(), nullable_string()),
        ("encoding", Value::null(), nullable_string()),
        ("xmlEncoding", Value::null(), nullable_string()),
        ("standalone", Value::bool(false), ParamTypeHint::Bool),
        ("xmlStandalone", Value::bool(false), ParamTypeHint::Bool),
        ("version", Value::string("1.0"), nullable_string()),
        ("xmlVersion", Value::string("1.0"), nullable_string()),
        (
            "strictErrorChecking",
            Value::bool(true),
            ParamTypeHint::Bool,
        ),
        ("documentURI", Value::null(), nullable_string()),
        ("config", Value::null(), ParamTypeHint::Mixed),
        ("formatOutput", Value::bool(false), ParamTypeHint::Bool),
        ("validateOnParse", Value::bool(false), ParamTypeHint::Bool),
        ("resolveExternals", Value::bool(false), ParamTypeHint::Bool),
        ("preserveWhiteSpace", Value::bool(true), ParamTypeHint::Bool),
        ("recover", Value::bool(false), ParamTypeHint::Bool),
        (
            "substituteEntities",
            Value::bool(false),
            ParamTypeHint::Bool,
        ),
        (
            "firstElementChild",
            Value::null(),
            nullable_class(DOM_ELEMENT),
        ),
        (
            "lastElementChild",
            Value::null(),
            nullable_class(DOM_ELEMENT),
        ),
        ("childElementCount", Value::long(0), ParamTypeHint::Int),
    ]
    .into_iter()
    .map(|(name, default, hint)| property(DOM_DOCUMENT, name, default, hint))
    .collect();
    eg.register_class(internal_class(
        DOM_DOCUMENT,
        Some(DOM_NODE),
        &["DOMParentNode"],
        false,
        false,
        document_properties,
        Vec::new(),
    ))
    .expect("DOMDocument registers once per request");

    let element_properties = [
        ("tagName", Value::string(""), ParamTypeHint::String),
        ("className", Value::string(""), ParamTypeHint::String),
        ("id", Value::string(""), ParamTypeHint::String),
        ("schemaTypeInfo", Value::null(), ParamTypeHint::Mixed),
        (
            "firstElementChild",
            Value::null(),
            nullable_class(DOM_ELEMENT),
        ),
        (
            "lastElementChild",
            Value::null(),
            nullable_class(DOM_ELEMENT),
        ),
        ("childElementCount", Value::long(0), ParamTypeHint::Int),
        (
            "previousElementSibling",
            Value::null(),
            nullable_class(DOM_ELEMENT),
        ),
        (
            "nextElementSibling",
            Value::null(),
            nullable_class(DOM_ELEMENT),
        ),
    ]
    .into_iter()
    .map(|(name, default, hint)| property(DOM_ELEMENT, name, default, hint))
    .collect();
    eg.register_class(internal_class(
        DOM_ELEMENT,
        Some(DOM_NODE),
        &["DOMParentNode", "DOMChildNode"],
        false,
        false,
        element_properties,
        Vec::new(),
    ))
    .expect("DOMElement registers once per request");

    let character_properties = vec![
        property(
            DOM_CHARACTER_DATA,
            "data",
            Value::string(""),
            ParamTypeHint::String,
        ),
        property(
            DOM_CHARACTER_DATA,
            "length",
            Value::long(0),
            ParamTypeHint::Int,
        ),
    ];
    eg.register_class(internal_class(
        DOM_CHARACTER_DATA,
        Some(DOM_NODE),
        &[],
        false,
        false,
        character_properties,
        Vec::new(),
    ))
    .expect("DOMCharacterData registers once per request");
    eg.register_class(internal_class(
        DOM_TEXT,
        Some(DOM_CHARACTER_DATA),
        &["DOMChildNode"],
        false,
        false,
        vec![property(
            DOM_TEXT,
            "wholeText",
            Value::string(""),
            ParamTypeHint::String,
        )],
        Vec::new(),
    ))
    .expect("DOMText registers once per request");
    for (name, parent, implements) in [
        (DOM_COMMENT, DOM_CHARACTER_DATA, vec!["DOMChildNode"]),
        (DOM_CDATA, DOM_TEXT, Vec::new()),
    ] {
        eg.register_class(internal_class(
            name,
            Some(parent),
            &implements,
            false,
            false,
            Vec::new(),
            Vec::new(),
        ))
        .expect("DOM character subtype registers once per request");
    }

    let attr_properties = [
        ("name", Value::string(""), ParamTypeHint::String),
        ("specified", Value::bool(true), ParamTypeHint::Bool),
        ("value", Value::string(""), ParamTypeHint::String),
        ("ownerElement", Value::null(), nullable_class(DOM_ELEMENT)),
        ("schemaTypeInfo", Value::null(), ParamTypeHint::Mixed),
    ]
    .into_iter()
    .map(|(name, default, hint)| property(DOM_ATTR, name, default, hint))
    .collect();
    eg.register_class(internal_class(
        DOM_ATTR,
        Some(DOM_NODE),
        &[],
        false,
        false,
        attr_properties,
        Vec::new(),
    ))
    .expect("DOMAttr registers once per request");
    eg.register_class(internal_class(
        DOM_FRAGMENT,
        Some(DOM_NODE),
        &["DOMParentNode"],
        false,
        false,
        vec![
            property(
                DOM_FRAGMENT,
                "firstElementChild",
                Value::null(),
                nullable_class(DOM_ELEMENT),
            ),
            property(
                DOM_FRAGMENT,
                "lastElementChild",
                Value::null(),
                nullable_class(DOM_ELEMENT),
            ),
            property(
                DOM_FRAGMENT,
                "childElementCount",
                Value::long(0),
                ParamTypeHint::Int,
            ),
        ],
        Vec::new(),
    ))
    .expect("DOMDocumentFragment registers once per request");
    eg.register_class(internal_class(
        DOM_DOCUMENT_TYPE,
        Some(DOM_NODE),
        &[],
        false,
        false,
        ["name", "publicId", "systemId", "internalSubset"]
            .into_iter()
            .map(|name| property(DOM_DOCUMENT_TYPE, name, Value::null(), nullable_string()))
            .chain(["entities", "notations"].into_iter().map(|name| {
                property(
                    DOM_DOCUMENT_TYPE,
                    name,
                    Value::null(),
                    ParamTypeHint::ClassName(DOM_NAMED_NODE_MAP.into()),
                )
            }))
            .collect(),
        Vec::new(),
    ))
    .expect("DOMDocumentType registers once per request");
    eg.register_class(internal_class(
        DOM_PI,
        Some(DOM_NODE),
        &[],
        false,
        false,
        vec![
            property(DOM_PI, "target", Value::string(""), ParamTypeHint::String),
            property(DOM_PI, "data", Value::string(""), ParamTypeHint::String),
        ],
        Vec::new(),
    ))
    .expect("DOMProcessingInstruction registers once per request");
    eg.register_class(internal_class(
        DOM_IMPLEMENTATION,
        None,
        &[],
        false,
        false,
        Vec::new(),
        Vec::new(),
    ))
    .expect("DOMImplementation registers once per request");
    for collection in [DOM_NODE_LIST, DOM_NAMED_NODE_MAP] {
        eg.register_class(internal_class(
            collection,
            None,
            &["IteratorAggregate", "Traversable", "Countable"],
            false,
            false,
            vec![property(
                collection,
                "length",
                Value::long(0),
                ParamTypeHint::Int,
            )],
            Vec::new(),
        ))
        .expect("DOM collection registers once per request");
    }
    eg.register_class(internal_class(
        DOM_XPATH,
        None,
        &[],
        false,
        false,
        vec![
            property(
                DOM_XPATH,
                "document",
                Value::null(),
                ParamTypeHint::ClassName(DOM_DOCUMENT.into()),
            ),
            property(
                DOM_XPATH,
                "registerNodeNamespaces",
                Value::bool(true),
                ParamTypeHint::Bool,
            ),
        ],
        Vec::new(),
    ))
    .expect("DOMXPath registers once per request");

    let mut methods = vec![
        method(
            DOM_NODE,
            "__construct",
            fn_node_direct_construct,
            &[],
            0,
            vec![],
            vec![],
            vec![],
            ParamTypeHint::Void,
        ),
        method(
            DOM_NODE,
            "appendChild",
            fn_node_append_child,
            &["node"],
            1,
            vec![None],
            vec![None],
            vec![ParamTypeHint::ClassName(DOM_NODE.into())],
            ParamTypeHint::ClassName(DOM_NODE.into()),
        ),
        method(
            DOM_NODE,
            "removeChild",
            fn_node_remove_child,
            &["child"],
            1,
            vec![None],
            vec![None],
            vec![ParamTypeHint::ClassName(DOM_NODE.into())],
            ParamTypeHint::ClassName(DOM_NODE.into()),
        ),
        method(
            DOM_NODE,
            "insertBefore",
            fn_node_insert_before,
            &["node", "child"],
            1,
            vec![None, Some(Value::null())],
            vec![None, Some("null")],
            vec![
                ParamTypeHint::ClassName(DOM_NODE.into()),
                nullable_class(DOM_NODE),
            ],
            ParamTypeHint::ClassName(DOM_NODE.into()),
        ),
        method(
            DOM_NODE,
            "replaceChild",
            fn_node_replace_child,
            &["node", "child"],
            2,
            vec![None, None],
            vec![None, None],
            vec![
                ParamTypeHint::ClassName(DOM_NODE.into()),
                ParamTypeHint::ClassName(DOM_NODE.into()),
            ],
            ParamTypeHint::ClassName(DOM_NODE.into()),
        ),
        method(
            DOM_NODE,
            "hasChildNodes",
            fn_node_has_children,
            &[],
            0,
            vec![],
            vec![],
            vec![],
            ParamTypeHint::Bool,
        ),
        method(
            DOM_NODE,
            "hasAttributes",
            fn_node_has_attributes,
            &[],
            0,
            vec![],
            vec![],
            vec![],
            ParamTypeHint::Bool,
        ),
        method(
            DOM_NODE,
            "normalize",
            fn_node_normalize,
            &[],
            0,
            vec![],
            vec![],
            vec![],
            ParamTypeHint::Void,
        ),
        method(
            DOM_NODE,
            "cloneNode",
            fn_node_clone,
            &["deep"],
            0,
            vec![Some(Value::bool(false))],
            vec![Some("false")],
            vec![ParamTypeHint::Bool],
            ParamTypeHint::ClassName(DOM_NODE.into()),
        ),
        method(
            DOM_NODE,
            "C14N",
            fn_node_c14n,
            &["exclusive", "withComments", "xpath", "nsPrefixes"],
            0,
            vec![
                Some(Value::bool(false)),
                Some(Value::bool(false)),
                Some(Value::null()),
                Some(Value::null()),
            ],
            vec![Some("false"), Some("false"), Some("null"), Some("null")],
            vec![
                ParamTypeHint::Bool,
                ParamTypeHint::Bool,
                ParamTypeHint::Nullable(Box::new(ParamTypeHint::Array)),
                ParamTypeHint::Nullable(Box::new(ParamTypeHint::Array)),
            ],
            ParamTypeHint::Union(vec![
                ParamTypeHint::String,
                ParamTypeHint::ClassName("false".into()),
            ]),
        ),
        method(
            DOM_DOCUMENT,
            "__construct",
            fn_document_construct,
            &["version", "encoding"],
            0,
            vec![Some(Value::string("1.0")), Some(Value::string(""))],
            vec![Some("'1.0'"), Some("''")],
            vec![ParamTypeHint::String, ParamTypeHint::String],
            ParamTypeHint::Void,
        ),
        method(
            DOM_DOCUMENT,
            "loadXML",
            fn_document_load_xml,
            &["source", "options"],
            1,
            vec![None, Some(Value::long(0))],
            vec![None, Some("0")],
            vec![ParamTypeHint::String, ParamTypeHint::Int],
            ParamTypeHint::Bool,
        ),
        method(
            DOM_DOCUMENT,
            "load",
            fn_document_load,
            &["filename", "options"],
            1,
            vec![None, Some(Value::long(0))],
            vec![None, Some("0")],
            vec![ParamTypeHint::String, ParamTypeHint::Int],
            ParamTypeHint::Bool,
        ),
        method(
            DOM_DOCUMENT,
            "saveXML",
            fn_document_save_xml,
            &["node", "options"],
            0,
            vec![Some(Value::null()), Some(Value::long(0))],
            vec![Some("null"), Some("0")],
            vec![nullable_class(DOM_NODE), ParamTypeHint::Int],
            ParamTypeHint::Union(vec![
                ParamTypeHint::String,
                ParamTypeHint::ClassName("false".into()),
            ]),
        ),
        method(
            DOM_DOCUMENT,
            "save",
            fn_document_save,
            &["filename", "options"],
            1,
            vec![None, Some(Value::long(0))],
            vec![None, Some("0")],
            vec![ParamTypeHint::String, ParamTypeHint::Int],
            ParamTypeHint::Union(vec![
                ParamTypeHint::Int,
                ParamTypeHint::ClassName("false".into()),
            ]),
        ),
        method(
            DOM_DOCUMENT,
            "createElement",
            fn_document_create_element,
            &["localName", "value"],
            1,
            vec![None, Some(Value::string(""))],
            vec![None, Some("''")],
            vec![ParamTypeHint::String, ParamTypeHint::String],
            ParamTypeHint::ClassName(DOM_ELEMENT.into()),
        ),
        method(
            DOM_DOCUMENT,
            "createElementNS",
            fn_document_create_element_ns,
            &["namespace", "qualifiedName", "value"],
            2,
            vec![None, None, Some(Value::string(""))],
            vec![None, None, Some("''")],
            vec![
                nullable_string(),
                ParamTypeHint::String,
                ParamTypeHint::String,
            ],
            ParamTypeHint::ClassName(DOM_ELEMENT.into()),
        ),
        method(
            DOM_DOCUMENT,
            "createTextNode",
            fn_document_create_text,
            &["data"],
            1,
            vec![None],
            vec![None],
            vec![ParamTypeHint::String],
            ParamTypeHint::ClassName(DOM_TEXT.into()),
        ),
        method(
            DOM_DOCUMENT,
            "createComment",
            fn_document_create_comment,
            &["data"],
            1,
            vec![None],
            vec![None],
            vec![ParamTypeHint::String],
            ParamTypeHint::ClassName(DOM_COMMENT.into()),
        ),
        method(
            DOM_DOCUMENT,
            "createCDATASection",
            fn_document_create_cdata,
            &["data"],
            1,
            vec![None],
            vec![None],
            vec![ParamTypeHint::String],
            ParamTypeHint::ClassName(DOM_CDATA.into()),
        ),
        method(
            DOM_DOCUMENT,
            "createDocumentFragment",
            fn_document_create_fragment,
            &[],
            0,
            vec![],
            vec![],
            vec![],
            ParamTypeHint::ClassName(DOM_FRAGMENT.into()),
        ),
        method(
            DOM_DOCUMENT,
            "createAttribute",
            fn_document_create_attribute,
            &["localName"],
            1,
            vec![None],
            vec![None],
            vec![ParamTypeHint::String],
            ParamTypeHint::ClassName(DOM_ATTR.into()),
        ),
        method(
            DOM_DOCUMENT,
            "createAttributeNS",
            fn_document_create_attribute,
            &["namespace", "qualifiedName"],
            2,
            vec![None, None],
            vec![None, None],
            vec![nullable_string(), ParamTypeHint::String],
            ParamTypeHint::ClassName(DOM_ATTR.into()),
        ),
        method(
            DOM_DOCUMENT,
            "createProcessingInstruction",
            fn_document_create_pi,
            &["target", "data"],
            1,
            vec![None, Some(Value::string(""))],
            vec![None, Some("''")],
            vec![ParamTypeHint::String, ParamTypeHint::String],
            ParamTypeHint::ClassName(DOM_PI.into()),
        ),
        method(
            DOM_DOCUMENT,
            "getElementsByTagName",
            fn_document_get_elements,
            &["qualifiedName"],
            1,
            vec![None],
            vec![None],
            vec![ParamTypeHint::String],
            ParamTypeHint::ClassName(DOM_NODE_LIST.into()),
        ),
        method(
            DOM_DOCUMENT,
            "getElementsByTagNameNS",
            fn_document_get_elements_ns,
            &["namespace", "localName"],
            2,
            vec![None, None],
            vec![None, None],
            vec![nullable_string(), ParamTypeHint::String],
            ParamTypeHint::ClassName(DOM_NODE_LIST.into()),
        ),
        method(
            DOM_DOCUMENT,
            "getElementById",
            fn_document_get_element_by_id,
            &["elementId"],
            1,
            vec![None],
            vec![None],
            vec![ParamTypeHint::String],
            nullable_class(DOM_ELEMENT),
        ),
        method(
            DOM_DOCUMENT,
            "normalizeDocument",
            fn_document_normalize,
            &[],
            0,
            vec![],
            vec![],
            vec![],
            ParamTypeHint::Void,
        ),
        method(
            DOM_DOCUMENT,
            "schemaValidateSource",
            fn_document_schema_validate_source,
            &["source", "flags"],
            1,
            vec![None, Some(Value::long(0))],
            vec![None, Some("0")],
            vec![ParamTypeHint::String, ParamTypeHint::Int],
            ParamTypeHint::Bool,
        ),
        method(
            DOM_DOCUMENT,
            "adoptNode",
            fn_document_adopt_node,
            &["node"],
            1,
            vec![None],
            vec![None],
            vec![ParamTypeHint::ClassName(DOM_NODE.into())],
            ParamTypeHint::Union(vec![
                ParamTypeHint::ClassName(DOM_NODE.into()),
                ParamTypeHint::ClassName("false".into()),
            ]),
        ),
        method(
            DOM_DOCUMENT,
            "importNode",
            fn_document_import_node,
            &["node", "deep"],
            1,
            vec![None, Some(Value::bool(false))],
            vec![None, Some("false")],
            vec![
                ParamTypeHint::ClassName(DOM_NODE.into()),
                ParamTypeHint::Bool,
            ],
            ParamTypeHint::ClassName(DOM_NODE.into()),
        ),
        method(
            DOM_ELEMENT,
            "__construct",
            fn_element_construct,
            &["qualifiedName", "value", "namespace"],
            1,
            vec![None, Some(Value::null()), Some(Value::string(""))],
            vec![None, Some("null"), Some("''")],
            vec![
                ParamTypeHint::String,
                nullable_string(),
                ParamTypeHint::String,
            ],
            ParamTypeHint::Void,
        ),
        method(
            DOM_ELEMENT,
            "getAttribute",
            fn_element_get_attribute,
            &["qualifiedName"],
            1,
            vec![None],
            vec![None],
            vec![ParamTypeHint::String],
            ParamTypeHint::String,
        ),
        method(
            DOM_ELEMENT,
            "getAttributeNS",
            fn_element_get_attribute_ns,
            &["namespace", "localName"],
            2,
            vec![None, None],
            vec![None, None],
            vec![nullable_string(), ParamTypeHint::String],
            ParamTypeHint::String,
        ),
        method(
            DOM_ELEMENT,
            "hasAttribute",
            fn_element_has_attribute,
            &["qualifiedName"],
            1,
            vec![None],
            vec![None],
            vec![ParamTypeHint::String],
            ParamTypeHint::Bool,
        ),
        method(
            DOM_ELEMENT,
            "hasAttributeNS",
            fn_element_has_attribute_ns,
            &["namespace", "localName"],
            2,
            vec![None, None],
            vec![None, None],
            vec![nullable_string(), ParamTypeHint::String],
            ParamTypeHint::Bool,
        ),
        method(
            DOM_ELEMENT,
            "setAttribute",
            fn_element_set_attribute,
            &["qualifiedName", "value"],
            2,
            vec![None, None],
            vec![None, None],
            vec![ParamTypeHint::String, ParamTypeHint::String],
            ParamTypeHint::ClassName(DOM_ATTR.into()),
        ),
        method(
            DOM_ELEMENT,
            "setAttributeNS",
            fn_element_set_attribute_ns,
            &["namespace", "qualifiedName", "value"],
            3,
            vec![None, None, None],
            vec![None, None, None],
            vec![
                nullable_string(),
                ParamTypeHint::String,
                ParamTypeHint::String,
            ],
            ParamTypeHint::Void,
        ),
        method(
            DOM_ELEMENT,
            "removeAttribute",
            fn_element_remove_attribute,
            &["qualifiedName"],
            1,
            vec![None],
            vec![None],
            vec![ParamTypeHint::String],
            ParamTypeHint::Void,
        ),
        method(
            DOM_ELEMENT,
            "getAttributeNames",
            fn_element_get_attribute_names,
            &[],
            0,
            vec![],
            vec![],
            vec![],
            ParamTypeHint::Array,
        ),
        method(
            DOM_ELEMENT,
            "getAttributeNode",
            fn_element_get_attribute_node,
            &["qualifiedName"],
            1,
            vec![None],
            vec![None],
            vec![ParamTypeHint::String],
            ParamTypeHint::Union(vec![
                ParamTypeHint::ClassName(DOM_ATTR.into()),
                ParamTypeHint::ClassName("false".into()),
            ]),
        ),
        method(
            DOM_ELEMENT,
            "getElementsByTagName",
            fn_element_get_elements,
            &["qualifiedName"],
            1,
            vec![None],
            vec![None],
            vec![ParamTypeHint::String],
            ParamTypeHint::ClassName(DOM_NODE_LIST.into()),
        ),
        method(
            DOM_ELEMENT,
            "getElementsByTagNameNS",
            fn_element_get_elements_ns,
            &["namespace", "localName"],
            2,
            vec![None, None],
            vec![None, None],
            vec![nullable_string(), ParamTypeHint::String],
            ParamTypeHint::ClassName(DOM_NODE_LIST.into()),
        ),
        method(
            DOM_FRAGMENT,
            "__construct",
            fn_fragment_construct,
            &[],
            0,
            vec![],
            vec![],
            vec![],
            ParamTypeHint::Void,
        ),
        method(
            DOM_FRAGMENT,
            "appendXML",
            fn_fragment_append_xml,
            &["data"],
            1,
            vec![None],
            vec![None],
            vec![ParamTypeHint::String],
            ParamTypeHint::Bool,
        ),
        method(
            DOM_TEXT,
            "__construct",
            fn_character_construct,
            &["data"],
            0,
            vec![Some(Value::string(""))],
            vec![Some("''")],
            vec![ParamTypeHint::String],
            ParamTypeHint::Void,
        ),
        method(
            DOM_COMMENT,
            "__construct",
            fn_character_construct,
            &["data"],
            0,
            vec![Some(Value::string(""))],
            vec![Some("''")],
            vec![ParamTypeHint::String],
            ParamTypeHint::Void,
        ),
        method(
            DOM_CDATA,
            "__construct",
            fn_character_construct,
            &["data"],
            1,
            vec![None],
            vec![None],
            vec![ParamTypeHint::String],
            ParamTypeHint::Void,
        ),
        method(
            DOM_ATTR,
            "__construct",
            fn_attr_construct,
            &["name", "value"],
            1,
            vec![None, Some(Value::string(""))],
            vec![None, Some("''")],
            vec![ParamTypeHint::String, ParamTypeHint::String],
            ParamTypeHint::Void,
        ),
        method(
            DOM_ATTR,
            "isId",
            fn_attr_is_id,
            &[],
            0,
            vec![],
            vec![],
            vec![],
            ParamTypeHint::Bool,
        ),
        method(
            DOM_PI,
            "__construct",
            fn_pi_construct,
            &["name", "value"],
            1,
            vec![None, Some(Value::string(""))],
            vec![None, Some("''")],
            vec![ParamTypeHint::String, ParamTypeHint::String],
            ParamTypeHint::Void,
        ),
        method(
            DOM_NODE_LIST,
            "count",
            fn_list_count,
            &[],
            0,
            vec![],
            vec![],
            vec![],
            ParamTypeHint::Int,
        ),
        method(
            DOM_NODE_LIST,
            "item",
            fn_list_item,
            &["index"],
            1,
            vec![None],
            vec![None],
            vec![ParamTypeHint::Int],
            nullable_class(DOM_NODE),
        ),
        method(
            DOM_NODE_LIST,
            "getIterator",
            fn_list_iterator,
            &[],
            0,
            vec![],
            vec![],
            vec![],
            ParamTypeHint::ClassName("Iterator".into()),
        ),
        method(
            DOM_NAMED_NODE_MAP,
            "count",
            fn_list_count,
            &[],
            0,
            vec![],
            vec![],
            vec![],
            ParamTypeHint::Int,
        ),
        method(
            DOM_NAMED_NODE_MAP,
            "item",
            fn_list_item,
            &["index"],
            1,
            vec![None],
            vec![None],
            vec![ParamTypeHint::Int],
            nullable_class(DOM_NODE),
        ),
        method(
            DOM_NAMED_NODE_MAP,
            "getNamedItem",
            fn_named_item,
            &["qualifiedName"],
            1,
            vec![None],
            vec![None],
            vec![ParamTypeHint::String],
            nullable_class(DOM_NODE),
        ),
        method(
            DOM_NAMED_NODE_MAP,
            "getIterator",
            fn_list_iterator,
            &[],
            0,
            vec![],
            vec![],
            vec![],
            ParamTypeHint::ClassName("Iterator".into()),
        ),
        method(
            DOM_XPATH,
            "__construct",
            fn_xpath_construct,
            &["document", "registerNodeNS"],
            1,
            vec![None, Some(Value::bool(true))],
            vec![None, Some("true")],
            vec![
                ParamTypeHint::ClassName(DOM_DOCUMENT.into()),
                ParamTypeHint::Bool,
            ],
            ParamTypeHint::Void,
        ),
        method(
            DOM_XPATH,
            "query",
            fn_xpath_query,
            &["expression", "contextNode", "registerNodeNS"],
            1,
            vec![None, Some(Value::null()), Some(Value::bool(true))],
            vec![None, Some("null"), Some("true")],
            vec![
                ParamTypeHint::String,
                nullable_class(DOM_NODE),
                ParamTypeHint::Bool,
            ],
            ParamTypeHint::Union(vec![
                ParamTypeHint::ClassName(DOM_NODE_LIST.into()),
                ParamTypeHint::ClassName("false".into()),
            ]),
        ),
        method(
            DOM_XPATH,
            "evaluate",
            fn_xpath_evaluate,
            &["expression", "contextNode", "registerNodeNS"],
            1,
            vec![None, Some(Value::null()), Some(Value::bool(true))],
            vec![None, Some("null"), Some("true")],
            vec![
                ParamTypeHint::String,
                nullable_class(DOM_NODE),
                ParamTypeHint::Bool,
            ],
            ParamTypeHint::Mixed,
        ),
        method(
            DOM_XPATH,
            "registerNamespace",
            fn_xpath_register_namespace,
            &["prefix", "namespace"],
            2,
            vec![None, None],
            vec![None, None],
            vec![ParamTypeHint::String, ParamTypeHint::String],
            ParamTypeHint::Bool,
        ),
    ];
    let mut quote = method(
        DOM_XPATH,
        "quote",
        fn_xpath_quote,
        &["str"],
        1,
        vec![None],
        vec![None],
        vec![ParamTypeHint::String],
        ParamTypeHint::String,
    );
    quote.is_static = true;
    methods.push(quote);
    for owner in [DOM_ELEMENT, DOM_FRAGMENT] {
        for (name, handler) in [
            ("append", fn_element_append as InternalFunctionHandler),
            ("prepend", fn_element_prepend),
        ] {
            let mut declaration = method(
                owner,
                name,
                handler,
                &["nodes"],
                0,
                vec![None],
                vec![None],
                vec![ParamTypeHint::None],
                ParamTypeHint::Void,
            );
            declaration.variadic = true;
            methods.push(declaration);
        }
    }

    let mut functions = Vec::with_capacity(methods.len());
    for declaration in methods {
        register_method(eg, &mut functions, declaration);
    }
    functions
}

fn fn_dom_import_simplexml(
    _ed: *mut ExecuteData,
    rv: *mut Value,
    _eg: &mut ExecutorGlobals,
) -> Result<(), VmError> {
    super::write_return_value(rv, Value::bool(false));
    Ok(())
}

pub(super) fn register_functions(eg: &mut ExecutorGlobals) -> Vec<Box<InternalFunction>> {
    let mut functions = Vec::with_capacity(2);
    for name in ["dom_import_simplexml", "Dom\\import_simplexml"] {
        let mut function = Box::new(
            make_internal_function(fn_dom_import_simplexml, 1, 1, vec!["node".to_string()])
                .with_static_parameter_names(&["node"]),
        );
        function.handler_validates_types = true;
        function.common.sig.param_type_hints = vec![ParamTypeHint::ClassName("object".into())];
        function.common.sig.return_type_hint = if name.contains('\\') {
            ParamTypeHint::Union(vec![
                ParamTypeHint::ClassName("Dom\\Attr".into()),
                ParamTypeHint::ClassName("Dom\\Element".into()),
            ])
        } else {
            ParamTypeHint::Union(vec![
                ParamTypeHint::ClassName(DOM_ATTR.into()),
                ParamTypeHint::ClassName(DOM_ELEMENT.into()),
            ])
        };
        let pointer = &function.common as *const FunctionCommon;
        eg.register_function(name, pointer)
            .expect("DOM function registers once per request");
        eg.register_internal_function_reflection_metadata(pointer, vec![None], "dom");
        functions.push(function);
    }
    functions
}
