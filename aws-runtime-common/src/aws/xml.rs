/// Minimal XML tree parser for AWS response XML.
/// No namespace handling, no attributes — just elements and text.

#[derive(Debug)]
pub enum XmlNode {
    Element { tag: String, children: Vec<XmlNode> },
    Text(String),
}

impl XmlNode {
    /// Parse an XML string into a tree. Returns the root element.
    pub fn parse(xml: &str) -> Result<XmlNode, String> {
        let mut stack: Vec<(String, Vec<XmlNode>)> = vec![("__root__".into(), vec![])];
        let mut pos = 0;
        let bytes = xml.as_bytes();

        while pos < bytes.len() {
            if bytes[pos] == b'<' {
                pos += 1;
                if pos >= bytes.len() { break; }

                if bytes[pos] == b'/' {
                    // Closing tag
                    pos += 1;
                    let tag_start = pos;
                    while pos < bytes.len() && bytes[pos] != b'>' { pos += 1; }
                    let tag = &xml[tag_start..pos];
                    pos += 1; // skip >

                    let (closed_tag, children) = stack.pop()
                        .ok_or_else(|| format!("unexpected closing tag: {tag}"))?;
                    if closed_tag != tag {
                        return Err(format!("mismatched tags: expected </{closed_tag}>, got </{tag}>"));
                    }
                    let node = XmlNode::Element { tag: closed_tag, children };
                    stack.last_mut()
                        .ok_or_else(|| "stack underflow".to_string())?
                        .1.push(node);
                } else if bytes[pos] == b'?' || bytes[pos] == b'!' {
                    // Processing instruction or comment/doctype — skip to >
                    if pos + 2 < bytes.len() && &bytes[pos..pos+3] == b"!--" {
                        // Comment: skip to -->
                        pos += 3;
                        while pos + 2 < bytes.len() && &bytes[pos..pos+3] != b"-->" { pos += 1; }
                        pos += 3;
                    } else {
                        while pos < bytes.len() && bytes[pos] != b'>' { pos += 1; }
                        pos += 1;
                    }
                } else {
                    // Opening tag — find tag name (stop at space, /, or >)
                    let tag_start = pos;
                    while pos < bytes.len() && !matches!(bytes[pos], b' ' | b'/' | b'>' | b'\t' | b'\n' | b'\r') {
                        pos += 1;
                    }
                    let tag = xml[tag_start..pos].to_string();

                    // Skip attributes (handle quoted values)
                    while pos < bytes.len() && bytes[pos] != b'>' && !(bytes[pos] == b'/' && pos + 1 < bytes.len() && bytes[pos + 1] == b'>') {
                        if bytes[pos] == b'"' {
                            pos += 1;
                            while pos < bytes.len() && bytes[pos] != b'"' { pos += 1; }
                        } else if bytes[pos] == b'\'' {
                            pos += 1;
                            while pos < bytes.len() && bytes[pos] != b'\'' { pos += 1; }
                        }
                        pos += 1;
                    }

                    if pos < bytes.len() && bytes[pos] == b'/' {
                        // Self-closing tag <foo/>
                        pos += 1; // skip /
                        if pos < bytes.len() && bytes[pos] == b'>' { pos += 1; }
                        let node = XmlNode::Element { tag, children: vec![] };
                        stack.last_mut()
                            .ok_or_else(|| "stack underflow".to_string())?
                            .1.push(node);
                    } else {
                        if pos < bytes.len() { pos += 1; } // skip >
                        stack.push((tag, vec![]));
                    }
                }
            } else {
                // Text content
                let text_start = pos;
                while pos < bytes.len() && bytes[pos] != b'<' { pos += 1; }
                let text = xml[text_start..pos].trim();
                if !text.is_empty() {
                    let decoded = xml_decode(text);
                    stack.last_mut()
                        .ok_or_else(|| "text outside element".to_string())?
                        .1.push(XmlNode::Text(decoded));
                }
            }
        }

        let (_, mut root_children) = stack.pop()
            .ok_or_else(|| "empty document".to_string())?;
        // Return the first (and typically only) root element
        if root_children.len() == 1 {
            Ok(root_children.remove(0))
        } else if root_children.is_empty() {
            Err("empty document".into())
        } else {
            // Wrap multiple root elements
            Ok(XmlNode::Element { tag: "__root__".into(), children: root_children })
        }
    }

    /// Get the tag name (empty string for text nodes).
    pub fn tag(&self) -> &str {
        match self {
            XmlNode::Element { tag, .. } => tag,
            XmlNode::Text(_) => "",
        }
    }

    /// Find the first child element with the given tag name.
    pub fn child(&self, name: &str) -> Option<&XmlNode> {
        match self {
            XmlNode::Element { children, .. } => {
                children.iter().find(|c| c.tag() == name)
            }
            _ => None,
        }
    }

    /// Find all child elements with the given tag name.
    pub fn children(&self, name: &str) -> Vec<&XmlNode> {
        match self {
            XmlNode::Element { children, .. } => {
                children.iter().filter(|c| c.tag() == name).collect()
            }
            _ => vec![],
        }
    }

    /// Get the concatenated text content of this element.
    pub fn text(&self) -> Option<String> {
        match self {
            XmlNode::Text(s) => Some(s.clone()),
            XmlNode::Element { children, .. } => {
                let mut out = String::new();
                for c in children {
                    if let XmlNode::Text(s) = c {
                        out.push_str(s);
                    }
                }
                if out.is_empty() { None } else { Some(out) }
            }
        }
    }
}

fn xml_decode(s: &str) -> String {
    s.replace("&amp;", "&")
     .replace("&lt;", "<")
     .replace("&gt;", ">")
     .replace("&quot;", "\"")
     .replace("&apos;", "'")
}

/// Trait for types that can be parsed from an XML element.
pub trait FromXml: Sized {
    fn from_xml(node: &XmlNode) -> Result<Self, String>;
}

// Primitive impls — parse from child text content
impl FromXml for String {
    fn from_xml(node: &XmlNode) -> Result<Self, String> {
        Ok(node.text().unwrap_or_default())
    }
}

impl FromXml for i32 {
    fn from_xml(node: &XmlNode) -> Result<Self, String> {
        node.text().unwrap_or_default().parse()
            .map_err(|e| format!("i32 parse: {e}"))
    }
}

impl FromXml for i64 {
    fn from_xml(node: &XmlNode) -> Result<Self, String> {
        node.text().unwrap_or_default().parse()
            .map_err(|e| format!("i64 parse: {e}"))
    }
}

impl FromXml for f32 {
    fn from_xml(node: &XmlNode) -> Result<Self, String> {
        node.text().unwrap_or_default().parse()
            .map_err(|e| format!("f32 parse: {e}"))
    }
}

impl FromXml for f64 {
    fn from_xml(node: &XmlNode) -> Result<Self, String> {
        node.text().unwrap_or_default().parse()
            .map_err(|e| format!("f64 parse: {e}"))
    }
}

impl FromXml for bool {
    fn from_xml(node: &XmlNode) -> Result<Self, String> {
        Ok(node.text().unwrap_or_default() == "true")
    }
}

impl<V: FromXml> FromXml for std::collections::HashMap<String, V> {
    fn from_xml(node: &XmlNode) -> Result<Self, String> {
        // AWS XML maps use <entry><key>K</key><value>V</value></entry> pattern
        let mut map = std::collections::HashMap::new();
        if let XmlNode::Element { children, .. } = node {
            for child in children {
                if let XmlNode::Element { children: entry_children, .. } = child {
                    let key = entry_children.iter()
                        .find(|c| c.tag() == "key")
                        .and_then(|c| c.text())
                        .unwrap_or_default();
                    if let Some(val_node) = entry_children.iter().find(|c| c.tag() == "value") {
                        map.insert(key, V::from_xml(val_node)?);
                    }
                }
            }
        }
        Ok(map)
    }
}

/// Escape text for inclusion in an XML element (used by generated
/// request-body serializers).
pub fn escape(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&apos;"),
            _ => out.push(c),
        }
    }
    out
}
