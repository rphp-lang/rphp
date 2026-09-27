use super::model::{Attribute, NodeId, NodeKind, Tree, split_name, valid_xml_name};

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct ParseError {
    pub(super) message: String,
    pub(super) line: usize,
    pub(super) column: usize,
}

#[derive(Clone, Default)]
struct NamespaceFrame {
    bindings: Vec<(Vec<u8>, Vec<u8>)>,
}

pub(super) fn parse_document(source: &[u8], preserve_whitespace: bool) -> Result<Tree, ParseError> {
    Parser::new(source, preserve_whitespace).parse()
}

struct Parser<'a> {
    source: &'a [u8],
    offset: usize,
    line: usize,
    line_start: usize,
    preserve_whitespace: bool,
    tree: Tree,
    stack: Vec<NodeId>,
    namespaces: Vec<NamespaceFrame>,
    root_seen: bool,
}

impl<'a> Parser<'a> {
    fn new(source: &'a [u8], preserve_whitespace: bool) -> Self {
        let offset = source
            .starts_with(&[0xef, 0xbb, 0xbf])
            .then_some(3)
            .unwrap_or(0);
        Self {
            source,
            offset,
            line: 1,
            line_start: offset,
            preserve_whitespace,
            tree: Tree::default(),
            stack: vec![0],
            namespaces: vec![NamespaceFrame::default()],
            root_seen: false,
        }
    }

    fn parse(mut self) -> Result<Tree, ParseError> {
        while self.offset < self.source.len() {
            if self.starts_with(b"<?xml") && !self.root_seen {
                self.parse_declaration()?;
            } else if self.starts_with(b"<!--") {
                self.parse_comment()?;
            } else if self.starts_with(b"<![CDATA[") {
                self.parse_cdata()?;
            } else if self.starts_with_case_insensitive(b"<!DOCTYPE") {
                self.parse_doctype()?;
            } else if self.starts_with(b"<?") {
                self.parse_processing_instruction()?;
            } else if self.starts_with(b"</") {
                self.parse_end_tag()?;
            } else if self.peek() == Some(b'<') {
                self.parse_start_tag()?;
            } else {
                self.parse_text()?;
            }
        }
        if self.stack.len() != 1 {
            return Err(self.error("Premature end of data in tag"));
        }
        if !self.root_seen {
            return Err(self.error("Start tag expected, '<' not found"));
        }
        Ok(self.tree)
    }

    fn parse_declaration(&mut self) -> Result<(), ParseError> {
        let end = self.find_from(self.offset + 5, b"?>")?;
        let attributes = self.parse_attributes_slice(self.offset + 5, end)?;
        if let Some(version) = find_attribute(&attributes, b"version") {
            self.tree.version = version.to_vec();
        }
        self.tree.encoding = find_attribute(&attributes, b"encoding").map(<[u8]>::to_vec);
        self.tree.standalone =
            find_attribute(&attributes, b"standalone").is_some_and(|value| value == b"yes");
        self.advance_to(end + 2);
        Ok(())
    }

    fn parse_comment(&mut self) -> Result<(), ParseError> {
        let end = self.find_from(self.offset + 4, b"-->")?;
        let value = self.source[self.offset + 4..end].to_vec();
        if value.windows(2).any(|window| window == b"--") {
            return Err(self.error("Double hyphen within comment"));
        }
        let node = self.tree.add_node(NodeKind::Comment(value));
        self.append(node)?;
        self.advance_to(end + 3);
        Ok(())
    }

    fn parse_cdata(&mut self) -> Result<(), ParseError> {
        if self.stack.len() == 1 {
            return Err(self.error("CData section not allowed outside document element"));
        }
        let end = self.find_from(self.offset + 9, b"]]>")?;
        let node = self
            .tree
            .add_node(NodeKind::Cdata(self.source[self.offset + 9..end].to_vec()));
        self.append(node)?;
        self.advance_to(end + 3);
        Ok(())
    }

    fn parse_doctype(&mut self) -> Result<(), ParseError> {
        let start = self.offset;
        let mut cursor = self.offset + 9;
        let mut quote = None;
        let mut subset_depth = 0usize;
        while cursor < self.source.len() {
            let byte = self.source[cursor];
            if let Some(delimiter) = quote {
                if byte == delimiter {
                    quote = None;
                }
            } else {
                match byte {
                    b'\'' | b'"' => quote = Some(byte),
                    b'[' => subset_depth += 1,
                    b']' => subset_depth = subset_depth.saturating_sub(1),
                    b'>' if subset_depth == 0 => break,
                    _ => {}
                }
            }
            cursor += 1;
        }
        if cursor == self.source.len() {
            return Err(self.error("DOCTYPE improperly terminated"));
        }
        let declaration = trim_ascii(&self.source[start + 9..cursor]);
        let name_end = declaration
            .iter()
            .position(u8::is_ascii_whitespace)
            .unwrap_or(declaration.len());
        let name = declaration[..name_end].to_vec();
        if !valid_xml_name(&name) {
            return Err(self.error("Invalid XML document type name"));
        }
        let internal_subset = declaration
            .iter()
            .position(|byte| *byte == b'[')
            .and_then(|start| {
                declaration
                    .iter()
                    .rposition(|byte| *byte == b']')
                    .filter(|end| *end > start)
                    .map(|end| trim_ascii(&declaration[start + 1..end]).to_vec())
            });
        let node = self.tree.add_node(NodeKind::DocumentType {
            name,
            public_id: Vec::new(),
            system_id: Vec::new(),
            internal_subset,
        });
        self.append(node)?;
        self.advance_to(cursor + 1);
        Ok(())
    }

    fn parse_processing_instruction(&mut self) -> Result<(), ParseError> {
        let end = self.find_from(self.offset + 2, b"?>")?;
        let body = trim_ascii(&self.source[self.offset + 2..end]);
        let target_end = body
            .iter()
            .position(u8::is_ascii_whitespace)
            .unwrap_or(body.len());
        let target = body[..target_end].to_vec();
        if !valid_xml_name(&target) {
            return Err(self.error("Invalid processing instruction target"));
        }
        let data = trim_ascii(&body[target_end..]).to_vec();
        let node = self
            .tree
            .add_node(NodeKind::ProcessingInstruction { target, data });
        self.append(node)?;
        self.advance_to(end + 2);
        Ok(())
    }

    fn parse_start_tag(&mut self) -> Result<(), ParseError> {
        let tag_start = self.offset;
        self.bump();
        let name_start = self.offset;
        while self.peek().is_some_and(is_name_byte) {
            self.bump();
        }
        let qualified_name = self.source[name_start..self.offset].to_vec();
        if !valid_xml_name(&qualified_name) {
            return Err(self.error("Invalid element name"));
        }
        let mut raw_attributes = Vec::new();
        let mut self_closing = false;
        loop {
            self.skip_space();
            match self.peek() {
                Some(b'>') => {
                    self.bump();
                    break;
                }
                Some(b'/') if self.source.get(self.offset + 1) == Some(&b'>') => {
                    self.advance_to(self.offset + 2);
                    self_closing = true;
                    break;
                }
                None => return Err(self.error("Couldn't find end of Start Tag")),
                _ => raw_attributes.push(self.parse_attribute()?),
            }
        }
        let mut frame = self.namespaces.last().cloned().unwrap_or_default();
        for (name, value) in &raw_attributes {
            if name == b"xmlns" {
                replace_binding(&mut frame.bindings, Vec::new(), value.clone());
            } else if let Some(prefix) = name.strip_prefix(b"xmlns:") {
                replace_binding(&mut frame.bindings, prefix.to_vec(), value.clone());
            }
        }
        let (prefix, local_name) = split_name(&qualified_name);
        let namespace = resolve_namespace(&frame, &prefix);
        let attributes = raw_attributes
            .into_iter()
            .map(|(qualified_name, value)| {
                let (prefix, local_name) = split_name(&qualified_name);
                let namespace = if qualified_name == b"xmlns" || prefix == b"xmlns" {
                    Some(b"http://www.w3.org/2000/xmlns/".to_vec())
                } else if prefix.is_empty() {
                    None
                } else {
                    resolve_namespace(&frame, &prefix)
                };
                Attribute {
                    is_id: qualified_name.eq_ignore_ascii_case(b"id"),
                    qualified_name,
                    prefix,
                    local_name,
                    namespace,
                    value,
                }
            })
            .collect();
        if self.stack.len() == 1 {
            if self.root_seen {
                return Err(self.error("Extra content at the end of the document"));
            }
            self.root_seen = true;
        }
        let node = self.tree.add_node(NodeKind::Element {
            qualified_name,
            prefix,
            local_name,
            namespace,
            attributes,
        });
        self.append(node)?;
        if !self_closing {
            self.stack.push(node);
            self.namespaces.push(frame);
        }
        if self.offset <= tag_start {
            return Err(self.error("Parser made no progress"));
        }
        Ok(())
    }

    fn parse_end_tag(&mut self) -> Result<(), ParseError> {
        self.advance_to(self.offset + 2);
        let name_start = self.offset;
        while self.peek().is_some_and(is_name_byte) {
            self.bump();
        }
        let name = self.source[name_start..self.offset].to_vec();
        self.skip_space();
        if self.peek() != Some(b'>') {
            return Err(self.error("End tag not finished"));
        }
        self.bump();
        if self.stack.len() == 1 {
            return Err(self.error("Opening and ending tag mismatch"));
        }
        let current = *self.stack.last().unwrap();
        if self.tree.qualified_name(current) != Some(name.as_slice()) {
            return Err(self.error("Opening and ending tag mismatch"));
        }
        self.stack.pop();
        self.namespaces.pop();
        Ok(())
    }

    fn parse_text(&mut self) -> Result<(), ParseError> {
        let start = self.offset;
        while self.peek().is_some_and(|byte| byte != b'<') {
            self.bump();
        }
        let raw = &self.source[start..self.offset];
        if self.stack.len() == 1 {
            if raw.iter().all(u8::is_ascii_whitespace) {
                return Ok(());
            }
            return Err(self.error("Extra content at the end of the document"));
        }
        if !self.preserve_whitespace && raw.iter().all(u8::is_ascii_whitespace) {
            return Ok(());
        }
        let value = decode_entities(raw).map_err(|message| self.error(message))?;
        if !value.is_empty() {
            let node = self.tree.add_node(NodeKind::Text(value));
            self.append(node)?;
        }
        Ok(())
    }

    fn parse_attribute(&mut self) -> Result<(Vec<u8>, Vec<u8>), ParseError> {
        let name_start = self.offset;
        while self.peek().is_some_and(is_name_byte) {
            self.bump();
        }
        let name = self.source[name_start..self.offset].to_vec();
        if !valid_xml_name(&name) {
            return Err(self.error("Invalid attribute name"));
        }
        self.skip_space();
        if self.peek() != Some(b'=') {
            return Err(self.error("Specification mandates value for attribute"));
        }
        self.bump();
        self.skip_space();
        let Some(quote @ (b'\'' | b'"')) = self.peek() else {
            return Err(self.error("AttValue: quote expected"));
        };
        self.bump();
        let value_start = self.offset;
        while self.peek().is_some_and(|byte| byte != quote) {
            if self.peek() == Some(b'<') {
                return Err(self.error("Unescaped '<' not allowed in attributes values"));
            }
            self.bump();
        }
        if self.peek().is_none() {
            return Err(self.error("Unfinished attribute value"));
        }
        let value = decode_entities(&self.source[value_start..self.offset])
            .map_err(|message| self.error(message))?;
        self.bump();
        Ok((name, value))
    }

    fn parse_attributes_slice(
        &self,
        mut start: usize,
        end: usize,
    ) -> Result<Vec<(Vec<u8>, Vec<u8>)>, ParseError> {
        let mut attributes = Vec::new();
        while start < end {
            while start < end && self.source[start].is_ascii_whitespace() {
                start += 1;
            }
            if start == end {
                break;
            }
            let name_start = start;
            while start < end && is_name_byte(self.source[start]) {
                start += 1;
            }
            let name = self.source[name_start..start].to_vec();
            while start < end && self.source[start].is_ascii_whitespace() {
                start += 1;
            }
            if self.source.get(start) != Some(&b'=') {
                return Err(self.error("Malformed XML declaration"));
            }
            start += 1;
            while start < end && self.source[start].is_ascii_whitespace() {
                start += 1;
            }
            let Some(quote @ (b'\'' | b'"')) = self.source.get(start).copied() else {
                return Err(self.error("Malformed XML declaration"));
            };
            start += 1;
            let value_start = start;
            while start < end && self.source[start] != quote {
                start += 1;
            }
            if start == end {
                return Err(self.error("Malformed XML declaration"));
            }
            attributes.push((name, self.source[value_start..start].to_vec()));
            start += 1;
        }
        Ok(attributes)
    }

    fn append(&mut self, node: NodeId) -> Result<(), ParseError> {
        let parent = *self.stack.last().unwrap();
        if self.tree.append_child(parent, node) {
            Ok(())
        } else {
            Err(self.error("Hierarchy request error"))
        }
    }

    fn starts_with(&self, needle: &[u8]) -> bool {
        self.source[self.offset..].starts_with(needle)
    }

    fn starts_with_case_insensitive(&self, needle: &[u8]) -> bool {
        self.source
            .get(self.offset..self.offset.saturating_add(needle.len()))
            .is_some_and(|candidate| candidate.eq_ignore_ascii_case(needle))
    }

    fn find_from(&self, start: usize, needle: &[u8]) -> Result<usize, ParseError> {
        self.source[start..]
            .windows(needle.len())
            .position(|window| window == needle)
            .map(|position| start + position)
            .ok_or_else(|| self.error("Premature end of data"))
    }

    fn peek(&self) -> Option<u8> {
        self.source.get(self.offset).copied()
    }

    fn bump(&mut self) {
        if self.peek() == Some(b'\n') {
            self.line += 1;
            self.line_start = self.offset + 1;
        }
        self.offset += 1;
    }

    fn advance_to(&mut self, target: usize) {
        while self.offset < target {
            self.bump();
        }
    }

    fn skip_space(&mut self) {
        while self.peek().is_some_and(|byte| byte.is_ascii_whitespace()) {
            self.bump();
        }
    }

    fn error(&self, message: impl Into<String>) -> ParseError {
        ParseError {
            message: message.into(),
            line: self.line,
            column: self.offset.saturating_sub(self.line_start) + 1,
        }
    }
}

fn is_name_byte(byte: u8) -> bool {
    byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b':' | b'-' | b'.') || byte >= 0x80
}

fn trim_ascii(mut value: &[u8]) -> &[u8] {
    while value.first().is_some_and(u8::is_ascii_whitespace) {
        value = &value[1..];
    }
    while value.last().is_some_and(u8::is_ascii_whitespace) {
        value = &value[..value.len() - 1];
    }
    value
}

fn find_attribute<'a>(attributes: &'a [(Vec<u8>, Vec<u8>)], name: &[u8]) -> Option<&'a [u8]> {
    attributes
        .iter()
        .find(|(candidate, _)| candidate.as_slice() == name)
        .map(|(_, value)| value.as_slice())
}

fn replace_binding(bindings: &mut Vec<(Vec<u8>, Vec<u8>)>, prefix: Vec<u8>, value: Vec<u8>) {
    if let Some((_, bound)) = bindings
        .iter_mut()
        .find(|(candidate, _)| *candidate == prefix)
    {
        *bound = value;
    } else {
        bindings.push((prefix, value));
    }
}

fn resolve_namespace(frame: &NamespaceFrame, prefix: &[u8]) -> Option<Vec<u8>> {
    frame
        .bindings
        .iter()
        .rev()
        .find(|(candidate, _)| candidate == prefix)
        .map(|(_, value)| value.clone())
        .filter(|value| !value.is_empty())
}

fn decode_entities(value: &[u8]) -> Result<Vec<u8>, &'static str> {
    let mut output = Vec::with_capacity(value.len());
    let mut index = 0;
    while index < value.len() {
        if value[index] != b'&' {
            output.push(value[index]);
            index += 1;
            continue;
        }
        let Some(relative_end) = value[index + 1..].iter().position(|byte| *byte == b';') else {
            return Err("EntityRef: expecting ';'");
        };
        let end = index + 1 + relative_end;
        let entity = &value[index + 1..end];
        match entity {
            b"amp" => output.push(b'&'),
            b"lt" => output.push(b'<'),
            b"gt" => output.push(b'>'),
            b"quot" => output.push(b'"'),
            b"apos" => output.push(b'\''),
            _ if entity.starts_with(b"#x") => {
                let text = std::str::from_utf8(&entity[2..])
                    .map_err(|_| "CharRef: invalid hexadecimal value")?;
                let codepoint = u32::from_str_radix(text, 16)
                    .map_err(|_| "CharRef: invalid hexadecimal value")?;
                push_codepoint(&mut output, codepoint)?;
            }
            _ if entity.starts_with(b"#") => {
                let text = std::str::from_utf8(&entity[1..])
                    .map_err(|_| "CharRef: invalid decimal value")?;
                let codepoint = text
                    .parse::<u32>()
                    .map_err(|_| "CharRef: invalid decimal value")?;
                push_codepoint(&mut output, codepoint)?;
            }
            _ => return Err("Entity not defined"),
        }
        index = end + 1;
    }
    Ok(output)
}

fn push_codepoint(output: &mut Vec<u8>, codepoint: u32) -> Result<(), &'static str> {
    let Some(character) = char::from_u32(codepoint) else {
        return Err("CharRef: invalid xmlChar value");
    };
    let mut buffer = [0u8; 4];
    output.extend_from_slice(character.encode_utf8(&mut buffer).as_bytes());
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_namespaces_entities_comments_and_cdata() {
        let tree = parse_document(
            br#"<?xml version="1.1" encoding="UTF-8"?><r xmlns="urn:x" id="a"><a>one &amp; two</a><!--c--><![CDATA[<x>]]></r>"#,
            true,
        )
        .unwrap();
        let root = tree.document_element().unwrap();
        assert_eq!(tree.local_name(root), Some(b"r".as_slice()));
        assert_eq!(tree.namespace(root), Some(b"urn:x".as_slice()));
        assert_eq!(tree.text_content(root), b"one & two<x>");
        assert_eq!(tree.version, b"1.1");
        assert_eq!(tree.encoding.as_deref(), Some(b"UTF-8".as_slice()));
    }

    #[test]
    fn rejects_mismatched_elements() {
        let error = parse_document(b"<a><b></a>", true).unwrap_err();
        assert_eq!(error.message, "Opening and ending tag mismatch");
    }
}
