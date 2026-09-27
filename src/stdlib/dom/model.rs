use std::rc::Rc;

pub(super) type NodeId = usize;

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct Attribute {
    pub(super) qualified_name: Vec<u8>,
    pub(super) prefix: Vec<u8>,
    pub(super) local_name: Vec<u8>,
    pub(super) namespace: Option<Vec<u8>>,
    pub(super) value: Vec<u8>,
    pub(super) is_id: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) enum NodeKind {
    Document,
    Element {
        qualified_name: Vec<u8>,
        prefix: Vec<u8>,
        local_name: Vec<u8>,
        namespace: Option<Vec<u8>>,
        attributes: Vec<Attribute>,
    },
    Text(Vec<u8>),
    Cdata(Vec<u8>),
    Comment(Vec<u8>),
    ProcessingInstruction {
        target: Vec<u8>,
        data: Vec<u8>,
    },
    Fragment,
    DocumentType {
        name: Vec<u8>,
        public_id: Vec<u8>,
        system_id: Vec<u8>,
        internal_subset: Option<Vec<u8>>,
    },
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct Node {
    pub(super) parent: Option<NodeId>,
    pub(super) children: Vec<NodeId>,
    pub(super) kind: NodeKind,
}

#[derive(Clone, Debug)]
pub(super) struct Tree {
    pub(super) nodes: Vec<Node>,
    pub(super) has_document_owner: bool,
    pub(super) version: Vec<u8>,
    pub(super) encoding: Option<Vec<u8>>,
    pub(super) standalone: bool,
    pub(super) document_uri: Option<Vec<u8>>,
}

impl Default for Tree {
    fn default() -> Self {
        Self::document(b"1.0", None)
    }
}

impl Tree {
    pub(super) fn document(version: &[u8], encoding: Option<&[u8]>) -> Self {
        Self {
            nodes: vec![Node {
                parent: None,
                children: Vec::new(),
                kind: NodeKind::Document,
            }],
            has_document_owner: true,
            version: version.to_vec(),
            encoding: encoding.map(<[u8]>::to_vec),
            standalone: false,
            document_uri: None,
        }
    }

    pub(super) fn detached() -> Self {
        let mut tree = Self::document(b"1.0", None);
        tree.has_document_owner = false;
        tree
    }

    pub(super) fn add_node(&mut self, kind: NodeKind) -> NodeId {
        let id = self.nodes.len();
        self.nodes.push(Node {
            parent: None,
            children: Vec::new(),
            kind,
        });
        id
    }

    pub(super) fn append_child(&mut self, parent: NodeId, child: NodeId) -> bool {
        if parent >= self.nodes.len() || child >= self.nodes.len() || parent == child {
            return false;
        }
        if self.is_ancestor(child, parent) {
            return false;
        }
        self.detach(child);
        self.nodes[child].parent = Some(parent);
        self.nodes[parent].children.push(child);
        true
    }

    pub(super) fn insert_before(
        &mut self,
        parent: NodeId,
        child: NodeId,
        reference: Option<NodeId>,
    ) -> bool {
        if parent >= self.nodes.len() || child >= self.nodes.len() || parent == child {
            return false;
        }
        if self.is_ancestor(child, parent) {
            return false;
        }
        let position = match reference {
            Some(reference) => {
                let Some(position) = self.nodes[parent]
                    .children
                    .iter()
                    .position(|candidate| *candidate == reference)
                else {
                    return false;
                };
                position
            }
            None => self.nodes[parent].children.len(),
        };
        self.detach(child);
        let position = position.min(self.nodes[parent].children.len());
        self.nodes[child].parent = Some(parent);
        self.nodes[parent].children.insert(position, child);
        true
    }

    pub(super) fn remove_child(&mut self, parent: NodeId, child: NodeId) -> bool {
        let Some(position) = self.nodes[parent]
            .children
            .iter()
            .position(|candidate| *candidate == child)
        else {
            return false;
        };
        self.nodes[parent].children.remove(position);
        self.nodes[child].parent = None;
        true
    }

    pub(super) fn replace_child(
        &mut self,
        parent: NodeId,
        replacement: NodeId,
        previous: NodeId,
    ) -> bool {
        let Some(position) = self.nodes[parent]
            .children
            .iter()
            .position(|candidate| *candidate == previous)
        else {
            return false;
        };
        if replacement == parent || self.is_ancestor(replacement, parent) {
            return false;
        }
        self.detach(replacement);
        self.nodes[parent].children[position] = replacement;
        self.nodes[replacement].parent = Some(parent);
        self.nodes[previous].parent = None;
        true
    }

    pub(super) fn detach(&mut self, node: NodeId) {
        let Some(parent) = self.nodes.get(node).and_then(|node| node.parent) else {
            return;
        };
        if let Some(position) = self.nodes[parent]
            .children
            .iter()
            .position(|candidate| *candidate == node)
        {
            self.nodes[parent].children.remove(position);
        }
        self.nodes[node].parent = None;
    }

    pub(super) fn is_ancestor(&self, ancestor: NodeId, mut node: NodeId) -> bool {
        while let Some(parent) = self.nodes.get(node).and_then(|node| node.parent) {
            if parent == ancestor {
                return true;
            }
            node = parent;
        }
        false
    }

    pub(super) fn document_element(&self) -> Option<NodeId> {
        self.nodes[0]
            .children
            .iter()
            .copied()
            .find(|id| matches!(self.nodes[*id].kind, NodeKind::Element { .. }))
    }

    pub(super) fn document_type(&self) -> Option<NodeId> {
        self.nodes[0]
            .children
            .iter()
            .copied()
            .find(|id| matches!(self.nodes[*id].kind, NodeKind::DocumentType { .. }))
    }

    pub(super) fn owner_document(&self, node: NodeId) -> Option<NodeId> {
        (self.has_document_owner && node != 0).then_some(0)
    }

    pub(super) fn node_name(&self, node: NodeId) -> Vec<u8> {
        match &self.nodes[node].kind {
            NodeKind::Document => b"#document".to_vec(),
            NodeKind::Element { qualified_name, .. } => qualified_name.clone(),
            NodeKind::Text(_) => b"#text".to_vec(),
            NodeKind::Cdata(_) => b"#cdata-section".to_vec(),
            NodeKind::Comment(_) => b"#comment".to_vec(),
            NodeKind::ProcessingInstruction { target, .. } => target.clone(),
            NodeKind::Fragment => b"#document-fragment".to_vec(),
            NodeKind::DocumentType { name, .. } => name.clone(),
        }
    }

    pub(super) fn node_type(&self, node: NodeId) -> i64 {
        match self.nodes[node].kind {
            NodeKind::Document => 9,
            NodeKind::Element { .. } => 1,
            NodeKind::Text(_) => 3,
            NodeKind::Cdata(_) => 4,
            NodeKind::Comment(_) => 8,
            NodeKind::ProcessingInstruction { .. } => 7,
            NodeKind::Fragment => 11,
            NodeKind::DocumentType { .. } => 10,
        }
    }

    pub(super) fn node_value(&self, node: NodeId) -> Option<Vec<u8>> {
        match &self.nodes[node].kind {
            NodeKind::Text(value) | NodeKind::Cdata(value) | NodeKind::Comment(value) => {
                Some(value.clone())
            }
            NodeKind::ProcessingInstruction { data, .. } => Some(data.clone()),
            _ => None,
        }
    }

    pub(super) fn qualified_name(&self, node: NodeId) -> Option<&[u8]> {
        match &self.nodes[node].kind {
            NodeKind::Element { qualified_name, .. } => Some(qualified_name),
            _ => None,
        }
    }

    pub(super) fn local_name(&self, node: NodeId) -> Option<&[u8]> {
        match &self.nodes[node].kind {
            NodeKind::Element { local_name, .. } => Some(local_name),
            _ => None,
        }
    }

    pub(super) fn namespace(&self, node: NodeId) -> Option<&[u8]> {
        match &self.nodes[node].kind {
            NodeKind::Element { namespace, .. } => namespace.as_deref(),
            _ => None,
        }
    }

    pub(super) fn prefix(&self, node: NodeId) -> &[u8] {
        match &self.nodes[node].kind {
            NodeKind::Element { prefix, .. } => prefix,
            _ => b"",
        }
    }

    pub(super) fn attributes(&self, node: NodeId) -> Option<&[Attribute]> {
        match &self.nodes[node].kind {
            NodeKind::Element { attributes, .. } => Some(attributes),
            _ => None,
        }
    }

    pub(super) fn attributes_mut(&mut self, node: NodeId) -> Option<&mut Vec<Attribute>> {
        match &mut self.nodes[node].kind {
            NodeKind::Element { attributes, .. } => Some(attributes),
            _ => None,
        }
    }

    pub(super) fn text_content(&self, node: NodeId) -> Vec<u8> {
        let mut output = Vec::new();
        self.append_text_content(node, &mut output);
        output
    }

    fn append_text_content(&self, node: NodeId, output: &mut Vec<u8>) {
        match &self.nodes[node].kind {
            NodeKind::Text(value) | NodeKind::Cdata(value) => output.extend_from_slice(value),
            NodeKind::Element { .. } | NodeKind::Document | NodeKind::Fragment => {
                for child in &self.nodes[node].children {
                    self.append_text_content(*child, output);
                }
            }
            NodeKind::ProcessingInstruction { data, .. } => output.extend_from_slice(data),
            NodeKind::Comment(_) | NodeKind::DocumentType { .. } => {}
        }
    }

    pub(super) fn descendants_named(&self, node: NodeId, qualified_name: &[u8]) -> Vec<NodeId> {
        let mut result = Vec::new();
        self.collect_descendants_named(node, qualified_name, &mut result);
        result
    }

    fn collect_descendants_named(
        &self,
        node: NodeId,
        qualified_name: &[u8],
        result: &mut Vec<NodeId>,
    ) {
        for child in &self.nodes[node].children {
            if let NodeKind::Element {
                qualified_name: name,
                ..
            } = &self.nodes[*child].kind
                && (qualified_name == b"*" || name == qualified_name)
            {
                result.push(*child);
            }
            self.collect_descendants_named(*child, qualified_name, result);
        }
    }

    pub(super) fn descendants_named_ns(
        &self,
        node: NodeId,
        namespace: Option<&[u8]>,
        local_name: &[u8],
    ) -> Vec<NodeId> {
        let mut result = Vec::new();
        self.collect_descendants_named_ns(node, namespace, local_name, &mut result);
        result
    }

    fn collect_descendants_named_ns(
        &self,
        node: NodeId,
        namespace: Option<&[u8]>,
        local_name: &[u8],
        result: &mut Vec<NodeId>,
    ) {
        for child in &self.nodes[node].children {
            if let NodeKind::Element {
                local_name: candidate,
                namespace: candidate_namespace,
                ..
            } = &self.nodes[*child].kind
                && (local_name == b"*" || candidate == local_name)
                && (namespace == Some(b"*") || candidate_namespace.as_deref() == namespace)
            {
                result.push(*child);
            }
            self.collect_descendants_named_ns(*child, namespace, local_name, result);
        }
    }

    pub(super) fn element_children(&self, node: NodeId) -> Vec<NodeId> {
        self.nodes[node]
            .children
            .iter()
            .copied()
            .filter(|child| matches!(self.nodes[*child].kind, NodeKind::Element { .. }))
            .collect()
    }

    pub(super) fn sibling(&self, node: NodeId, next: bool, elements_only: bool) -> Option<NodeId> {
        let parent = self.nodes[node].parent?;
        let siblings = &self.nodes[parent].children;
        let position = siblings.iter().position(|candidate| *candidate == node)?;
        let candidates: Box<dyn Iterator<Item = &NodeId>> = if next {
            Box::new(siblings[position + 1..].iter())
        } else {
            Box::new(siblings[..position].iter().rev())
        };
        candidates
            .filter(|candidate| {
                !elements_only || matches!(self.nodes[**candidate].kind, NodeKind::Element { .. })
            })
            .copied()
            .next()
    }

    pub(super) fn normalize(&mut self, node: NodeId) {
        let children = self.nodes[node].children.clone();
        for child in &children {
            self.normalize(*child);
        }
        let mut index = 0;
        while index + 1 < self.nodes[node].children.len() {
            let left = self.nodes[node].children[index];
            let right = self.nodes[node].children[index + 1];
            let right_value = match &self.nodes[right].kind {
                NodeKind::Text(value) => Some(value.clone()),
                _ => None,
            };
            if let (NodeKind::Text(left_value), Some(right_value)) =
                (&mut self.nodes[left].kind, right_value)
            {
                left_value.extend_from_slice(&right_value);
                self.nodes[node].children.remove(index + 1);
                self.nodes[right].parent = None;
            } else {
                index += 1;
            }
        }
    }

    pub(super) fn clone_subtree_into(
        source: &Rc<std::cell::RefCell<Tree>>,
        source_node: NodeId,
        target: &mut Tree,
        deep: bool,
    ) -> NodeId {
        let source = source.borrow();
        fn copy(source: &Tree, source_node: NodeId, target: &mut Tree, deep: bool) -> NodeId {
            let id = target.add_node(source.nodes[source_node].kind.clone());
            if deep {
                for child in &source.nodes[source_node].children {
                    let child = copy(source, *child, target, true);
                    target.append_child(id, child);
                }
            }
            id
        }
        copy(&source, source_node, target, deep)
    }

    pub(super) fn serialize_document(&self, pretty: bool, options: i64) -> Vec<u8> {
        let mut output = Vec::new();
        output.extend_from_slice(b"<?xml version=\"");
        output.extend_from_slice(&self.version);
        output.push(b'"');
        if let Some(encoding) = self
            .encoding
            .as_deref()
            .filter(|encoding| !encoding.is_empty())
        {
            output.extend_from_slice(b" encoding=\"");
            output.extend_from_slice(encoding);
            output.push(b'"');
        }
        if self.standalone {
            output.extend_from_slice(b" standalone=\"yes\"");
        }
        output.extend_from_slice(b"?>\n");
        for child in &self.nodes[0].children {
            self.serialize_node(*child, &mut output, pretty, 0, options, false, false, true);
        }
        output
    }

    pub(super) fn serialize_node_only(&self, node: NodeId, pretty: bool, options: i64) -> Vec<u8> {
        let mut output = Vec::new();
        self.serialize_node(node, &mut output, pretty, 0, options, false, false, false);
        output
    }

    pub(super) fn canonicalize(&self, node: NodeId, with_comments: bool) -> Vec<u8> {
        let mut output = Vec::new();
        self.serialize_node(node, &mut output, false, 0, 0, true, with_comments, false);
        output
    }

    fn serialize_node(
        &self,
        node: NodeId,
        output: &mut Vec<u8>,
        pretty: bool,
        depth: usize,
        options: i64,
        canonical: bool,
        with_comments: bool,
        top_level: bool,
    ) {
        match &self.nodes[node].kind {
            NodeKind::Document => {
                for child in &self.nodes[node].children {
                    self.serialize_node(
                        *child,
                        output,
                        pretty,
                        depth,
                        options,
                        canonical,
                        with_comments,
                        true,
                    );
                }
            }
            NodeKind::Element {
                qualified_name,
                attributes,
                ..
            } => {
                if pretty && !top_level {
                    output.push(b'\n');
                    output.extend(std::iter::repeat_n(b' ', depth * 2));
                }
                output.push(b'<');
                output.extend_from_slice(qualified_name);
                let mut ordered = attributes.iter().collect::<Vec<_>>();
                if canonical {
                    ordered.sort_unstable_by(|left, right| {
                        let left_namespace = left.qualified_name == b"xmlns"
                            || left.qualified_name.starts_with(b"xmlns:");
                        let right_namespace = right.qualified_name == b"xmlns"
                            || right.qualified_name.starts_with(b"xmlns:");
                        right_namespace
                            .cmp(&left_namespace)
                            .then_with(|| left.qualified_name.cmp(&right.qualified_name))
                    });
                }
                for attribute in ordered {
                    output.push(b' ');
                    output.extend_from_slice(&attribute.qualified_name);
                    output.extend_from_slice(b"=\"");
                    escape_attribute(output, &attribute.value, canonical);
                    output.push(b'"');
                }
                if self.nodes[node].children.is_empty() && !canonical && options & 4 == 0 {
                    output.extend_from_slice(b"/>");
                } else {
                    output.push(b'>');
                    let element_children = self.nodes[node]
                        .children
                        .iter()
                        .any(|child| matches!(self.nodes[*child].kind, NodeKind::Element { .. }));
                    for child in &self.nodes[node].children {
                        self.serialize_node(
                            *child,
                            output,
                            pretty && element_children,
                            depth + 1,
                            options,
                            canonical,
                            with_comments,
                            false,
                        );
                    }
                    if pretty && element_children {
                        output.push(b'\n');
                        output.extend(std::iter::repeat_n(b' ', depth * 2));
                    }
                    output.extend_from_slice(b"</");
                    output.extend_from_slice(qualified_name);
                    output.push(b'>');
                }
                if top_level && !output.ends_with(b"\n") {
                    output.push(b'\n');
                }
            }
            NodeKind::Text(value) => escape_text(output, value),
            NodeKind::Cdata(value) => {
                if canonical {
                    escape_text(output, value);
                } else {
                    output.extend_from_slice(b"<![CDATA[");
                    output.extend_from_slice(value);
                    output.extend_from_slice(b"]]>");
                }
            }
            NodeKind::Comment(value) => {
                if !canonical || with_comments {
                    output.extend_from_slice(b"<!--");
                    output.extend_from_slice(value);
                    output.extend_from_slice(b"-->");
                    if top_level {
                        output.push(b'\n');
                    }
                }
            }
            NodeKind::ProcessingInstruction { target, data } => {
                output.extend_from_slice(b"<?");
                output.extend_from_slice(target);
                if !data.is_empty() {
                    output.push(b' ');
                    output.extend_from_slice(data);
                }
                output.extend_from_slice(b"?>");
                if top_level {
                    output.push(b'\n');
                }
            }
            NodeKind::Fragment => {
                for child in &self.nodes[node].children {
                    self.serialize_node(
                        *child,
                        output,
                        pretty,
                        depth,
                        options,
                        canonical,
                        with_comments,
                        top_level,
                    );
                }
            }
            NodeKind::DocumentType {
                name,
                public_id,
                system_id,
                internal_subset,
            } => {
                output.extend_from_slice(b"<!DOCTYPE ");
                output.extend_from_slice(name);
                if !public_id.is_empty() {
                    output.extend_from_slice(b" PUBLIC \"");
                    output.extend_from_slice(public_id);
                    output.push(b'"');
                    if !system_id.is_empty() {
                        output.extend_from_slice(b" \"");
                        output.extend_from_slice(system_id);
                        output.push(b'"');
                    }
                } else if !system_id.is_empty() {
                    output.extend_from_slice(b" SYSTEM \"");
                    output.extend_from_slice(system_id);
                    output.push(b'"');
                }
                if let Some(subset) = internal_subset {
                    output.extend_from_slice(b" [");
                    output.extend_from_slice(subset);
                    output.push(b']');
                }
                output.push(b'>');
                if top_level {
                    output.push(b'\n');
                }
            }
        }
    }
}

pub(super) fn split_name(name: &[u8]) -> (Vec<u8>, Vec<u8>) {
    match name.iter().position(|byte| *byte == b':') {
        Some(position) => (name[..position].to_vec(), name[position + 1..].to_vec()),
        None => (Vec::new(), name.to_vec()),
    }
}

pub(super) fn valid_xml_name(name: &[u8]) -> bool {
    let Some(first) = name.first() else {
        return false;
    };
    if !(first.is_ascii_alphabetic() || matches!(first, b'_' | b':') || *first >= 0x80) {
        return false;
    }
    name[1..].iter().all(|byte| {
        byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b':' | b'-' | b'.') || *byte >= 0x80
    })
}

pub(super) fn escape_text(output: &mut Vec<u8>, value: &[u8]) {
    for byte in value {
        match byte {
            b'&' => output.extend_from_slice(b"&amp;"),
            b'<' => output.extend_from_slice(b"&lt;"),
            b'>' => output.extend_from_slice(b"&gt;"),
            _ => output.push(*byte),
        }
    }
}

fn escape_attribute(output: &mut Vec<u8>, value: &[u8], canonical: bool) {
    for byte in value {
        match byte {
            b'&' => output.extend_from_slice(b"&amp;"),
            b'<' => output.extend_from_slice(b"&lt;"),
            b'"' => output.extend_from_slice(b"&quot;"),
            b'\t' if canonical => output.extend_from_slice(b"&#x9;"),
            b'\n' if canonical => output.extend_from_slice(b"&#xA;"),
            b'\r' if canonical => output.extend_from_slice(b"&#xD;"),
            _ => output.push(*byte),
        }
    }
}
