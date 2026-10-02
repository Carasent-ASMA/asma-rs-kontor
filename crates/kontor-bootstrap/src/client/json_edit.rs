//! A byte-preserving JSON/JSONC editor.
//!
//! Client configuration files are operator-owned: bootstrap adds or replaces
//! exactly one `kontor` member and must leave every other byte — comments,
//! indentation, sibling values, key order — untouched. A parse-and-reprint
//! round trip cannot promise that, so this module parses the document into
//! spans and performs the smallest possible splice.
//!
//! JSONC (comments and trailing commas) is accepted only when the caller
//! declares it; a strict `.json` file with a comment is refused rather than
//! guessed at.

use serde_json::Value;

/// Which dialect a document is read as.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Dialect {
    /// Strict JSON: no comments, no trailing commas.
    Json,
    /// JSON with comments and trailing commas.
    Jsonc,
}

/// Why a document was refused.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum JsonEditError {
    /// A comment appeared in a strict JSON document.
    #[error("a comment is not valid in a strict JSON document")]
    StrictComment,
    /// A trailing comma appeared in a strict JSON document.
    #[error("a trailing comma is not valid in a strict JSON document")]
    StrictTrailingComma,
    /// The document is not well-formed.
    #[error("the document is not well-formed JSON")]
    Malformed,
    /// A key appears more than once in one object.
    #[error("an object declares the same key more than once")]
    DuplicateKey,
    /// The addressed value is not of the shape the edit needs.
    #[error("the addressed value is not an object")]
    NotAnObject,
    /// The member to add already exists.
    #[error("the member already exists")]
    MemberExists,
}

#[derive(Debug, Clone)]
enum Node {
    Object {
        open: usize,
        close: usize,
        members: Vec<Member>,
    },
    Array {
        open: usize,
        close: usize,
        #[allow(dead_code)]
        elements: Vec<Node>,
    },
    Scalar {
        start: usize,
        end: usize,
    },
}

#[derive(Debug, Clone)]
struct Member {
    key: String,
    key_start: usize,
    value: Node,
}

impl Node {
    fn span(&self) -> (usize, usize) {
        match self {
            Node::Object { open, close, .. } => (*open, close + 1),
            Node::Array { open, close, .. } => (*open, close + 1),
            Node::Scalar { start, end } => (*start, *end),
        }
    }
}

/// One parsed document that can be edited and re-emitted.
#[derive(Debug, Clone)]
pub struct JsonDocument {
    text: String,
    root: Node,
    dialect: Dialect,
}

impl JsonDocument {
    /// Parse one document in the declared dialect.
    ///
    /// # Errors
    /// Malformed input, a duplicate key, or a comment/trailing comma in a
    /// strict document.
    pub fn parse(text: impl Into<String>, dialect: Dialect) -> Result<Self, JsonEditError> {
        let text = text.into();
        let mut parser = Parser {
            bytes: text.as_bytes(),
            pos: 0,
            dialect,
        };
        let root = parser.parse_value()?;
        parser.skip_trivia()?;
        if parser.pos != parser.bytes.len() {
            return Err(JsonEditError::Malformed);
        }
        Ok(Self {
            text,
            root,
            dialect,
        })
    }

    /// The current text.
    #[must_use]
    pub fn text(&self) -> &str {
        &self.text
    }

    /// The raw text of one value, if the path resolves.
    #[must_use]
    pub fn value_text(&self, path: &[&str]) -> Option<&str> {
        let node = self.lookup(path)?;
        let (start, end) = node.span();
        Some(&self.text[start..end])
    }

    /// Whether a member exists at `path` under the root.
    #[must_use]
    pub fn has_member(&self, path: &[&str]) -> bool {
        self.lookup(path).is_some()
    }

    /// The parsed value at `path`, if it resolves and parses.
    #[must_use]
    pub fn value(&self, path: &[&str]) -> Option<Value> {
        serde_json::from_str(self.value_text(path)?).ok()
    }

    /// Add `name: value` inside the object at `path`, preserving every other
    /// byte.
    ///
    /// `value` must already be a valid JSON document fragment (adapters pass a
    /// [`serde_json::to_string`] result).
    ///
    /// # Errors
    /// The parent path is missing or not an object, or the member exists.
    pub fn insert_member(
        &mut self,
        path: &[&str],
        name: &str,
        value: &str,
    ) -> Result<(), JsonEditError> {
        let (open, close, members) = match self.lookup(path) {
            Some(Node::Object {
                open,
                close,
                members,
            }) => (*open, *close, members.clone()),
            Some(_) => return Err(JsonEditError::NotAnObject),
            None => return Err(JsonEditError::NotAnObject),
        };
        if members.iter().any(|member| member.key == name) {
            return Err(JsonEditError::MemberExists);
        }
        let escaped_name = serde_json::to_string(name).map_err(|_| JsonEditError::Malformed)?;
        let unit = indent_unit(&self.text);
        if members.is_empty() {
            let gap = &self.text[open + 1..close];
            if gap.contains("//") || gap.contains("/*") {
                let insertion = format!("{escaped_name}: {value},");
                self.text.insert_str(open + 1, &insertion);
            } else {
                let outer = line_indent(&self.text, open);
                let indent = format!("{outer}{unit}");
                let replacement = format!("{{\n{indent}{escaped_name}: {value}\n{outer}}}");
                self.text.replace_range(open..close + 1, &replacement);
            }
        } else {
            let last = members.last().expect("non-empty");
            let member_indent = indent_before(&self.text, last.key_start);
            let indent = if member_indent.is_empty() {
                unit
            } else {
                member_indent
            };
            let (_, value_end) = last.value.span();
            let after = skip_trivia_from(&self.text, value_end, self.dialect)?;
            if self.text[after..].starts_with(',') {
                let replacement = format!(",\n{indent}{escaped_name}: {value},");
                self.text.replace_range(after..after + 1, &replacement);
            } else {
                let insertion = format!(",\n{indent}{escaped_name}: {value}");
                self.text.insert_str(value_end, &insertion);
            }
        }
        self.reparse()
    }

    /// Replace the value at `path` with `value`, preserving every other byte.
    ///
    /// # Errors
    /// The path does not resolve.
    pub fn replace_value(&mut self, path: &[&str], value: &str) -> Result<(), JsonEditError> {
        let node = self.lookup(path).ok_or(JsonEditError::NotAnObject)?;
        let (start, end) = node.span();
        self.text.replace_range(start..end, value);
        self.reparse()
    }

    fn lookup(&self, path: &[&str]) -> Option<&Node> {
        let mut node = &self.root;
        for segment in path {
            let Node::Object { members, .. } = node else {
                return None;
            };
            node = &members.iter().find(|member| member.key == *segment)?.value;
        }
        Some(node)
    }

    fn reparse(&mut self) -> Result<(), JsonEditError> {
        *self = Self::parse(std::mem::take(&mut self.text), self.dialect)?;
        Ok(())
    }
}

fn line_indent(text: &str, offset: usize) -> String {
    let line_start = text[..offset].rfind('\n').map_or(0, |index| index + 1);
    text[line_start..offset]
        .chars()
        .take_while(|character| *character == ' ' || *character == '\t')
        .collect()
}

fn indent_before(text: &str, offset: usize) -> String {
    let line_start = text[..offset].rfind('\n').map_or(0, |index| index + 1);
    let candidate = &text[line_start..offset];
    if candidate
        .chars()
        .all(|character| character == ' ' || character == '\t')
    {
        candidate.to_owned()
    } else {
        String::new()
    }
}

fn indent_unit(text: &str) -> String {
    for line in text.lines() {
        let whitespace: String = line
            .chars()
            .take_while(|character| *character == ' ' || *character == '\t')
            .collect();
        if !whitespace.is_empty() && whitespace.len() < line.len() {
            return whitespace;
        }
    }
    "  ".to_owned()
}

fn skip_trivia_from(
    text: &str,
    mut offset: usize,
    dialect: Dialect,
) -> Result<usize, JsonEditError> {
    let bytes = text.as_bytes();
    loop {
        while offset < bytes.len() && matches!(bytes[offset], b' ' | b'\t' | b'\n' | b'\r') {
            offset += 1;
        }
        if offset + 1 < bytes.len() && bytes[offset] == b'/' {
            match bytes[offset + 1] {
                b'/' => {
                    if dialect == Dialect::Json {
                        return Err(JsonEditError::StrictComment);
                    }
                    while offset < bytes.len() && bytes[offset] != b'\n' {
                        offset += 1;
                    }
                }
                b'*' => {
                    if dialect == Dialect::Json {
                        return Err(JsonEditError::StrictComment);
                    }
                    offset += 2;
                    while offset + 1 < bytes.len()
                        && !(bytes[offset] == b'*' && bytes[offset + 1] == b'/')
                    {
                        offset += 1;
                    }
                    if offset + 1 >= bytes.len() {
                        return Err(JsonEditError::Malformed);
                    }
                    offset += 2;
                }
                _ => return Ok(offset),
            }
        } else {
            return Ok(offset);
        }
    }
}

struct Parser<'a> {
    bytes: &'a [u8],
    pos: usize,
    dialect: Dialect,
}

impl Parser<'_> {
    fn parse_value(&mut self) -> Result<Node, JsonEditError> {
        self.skip_trivia()?;
        let start = self.pos;
        match self.peek() {
            Some(b'{') => self.parse_object(),
            Some(b'[') => self.parse_array(),
            Some(b'"') => {
                self.parse_string()?;
                Ok(Node::Scalar {
                    start,
                    end: self.pos,
                })
            }
            Some(b't') | Some(b'f') | Some(b'n') => {
                self.parse_literal()?;
                Ok(Node::Scalar {
                    start,
                    end: self.pos,
                })
            }
            Some(byte) if byte == b'-' || byte.is_ascii_digit() => {
                self.parse_number()?;
                Ok(Node::Scalar {
                    start,
                    end: self.pos,
                })
            }
            _ => Err(JsonEditError::Malformed),
        }
    }

    fn parse_object(&mut self) -> Result<Node, JsonEditError> {
        let open = self.pos;
        self.pos += 1;
        let mut members = Vec::new();
        let mut just_comma = false;
        loop {
            self.skip_trivia()?;
            if self.peek() == Some(b'}') {
                if just_comma && self.dialect == Dialect::Json {
                    return Err(JsonEditError::StrictTrailingComma);
                }
                let close = self.pos;
                self.pos += 1;
                return Ok(Node::Object {
                    open,
                    close,
                    members,
                });
            }
            let key_start = self.pos;
            if self.peek() != Some(b'"') {
                return Err(JsonEditError::Malformed);
            }
            let key = self.parse_string()?;
            if members.iter().any(|member| member.key == key) {
                return Err(JsonEditError::DuplicateKey);
            }
            self.skip_trivia()?;
            if self.peek() != Some(b':') {
                return Err(JsonEditError::Malformed);
            }
            self.pos += 1;
            let value = self.parse_value()?;
            members.push(Member {
                key,
                key_start,
                value,
            });
            self.skip_trivia()?;
            match self.peek() {
                Some(b',') => {
                    self.pos += 1;
                    just_comma = true;
                }
                Some(b'}') => {
                    let close = self.pos;
                    self.pos += 1;
                    return Ok(Node::Object {
                        open,
                        close,
                        members,
                    });
                }
                _ => return Err(JsonEditError::Malformed),
            }
        }
    }

    fn parse_array(&mut self) -> Result<Node, JsonEditError> {
        let open = self.pos;
        self.pos += 1;
        let mut elements = Vec::new();
        let mut just_comma = false;
        loop {
            self.skip_trivia()?;
            if self.peek() == Some(b']') {
                if just_comma && self.dialect == Dialect::Json {
                    return Err(JsonEditError::StrictTrailingComma);
                }
                let close = self.pos;
                self.pos += 1;
                return Ok(Node::Array {
                    open,
                    close,
                    elements,
                });
            }
            elements.push(self.parse_value()?);
            self.skip_trivia()?;
            match self.peek() {
                Some(b',') => {
                    self.pos += 1;
                    just_comma = true;
                }
                Some(b']') => {
                    let close = self.pos;
                    self.pos += 1;
                    return Ok(Node::Array {
                        open,
                        close,
                        elements,
                    });
                }
                _ => return Err(JsonEditError::Malformed),
            }
        }
    }

    fn parse_string(&mut self) -> Result<String, JsonEditError> {
        self.pos += 1;
        let mut decoded = String::new();
        loop {
            let Some(byte) = self.peek() else {
                return Err(JsonEditError::Malformed);
            };
            match byte {
                b'"' => {
                    self.pos += 1;
                    return Ok(decoded);
                }
                b'\\' => {
                    self.pos += 1;
                    let Some(escape) = self.peek() else {
                        return Err(JsonEditError::Malformed);
                    };
                    self.pos += 1;
                    match escape {
                        b'"' | b'\\' | b'/' => decoded.push(escape as char),
                        b'b' => decoded.push('\u{8}'),
                        b'f' => decoded.push('\u{c}'),
                        b'n' => decoded.push('\n'),
                        b'r' => decoded.push('\r'),
                        b't' => decoded.push('\t'),
                        b'u' => {
                            let first = self.parse_hex4()?;
                            if (0xD800..=0xDBFF).contains(&first) {
                                if self.peek() != Some(b'\\') {
                                    return Err(JsonEditError::Malformed);
                                }
                                self.pos += 1;
                                if self.peek() != Some(b'u') {
                                    return Err(JsonEditError::Malformed);
                                }
                                self.pos += 1;
                                let second = self.parse_hex4()?;
                                if !(0xDC00..=0xDFFF).contains(&second) {
                                    return Err(JsonEditError::Malformed);
                                }
                                let combined = 0x10000
                                    + ((u32::from(first) - 0xD800) << 10)
                                    + (u32::from(second) - 0xDC00);
                                decoded.push(
                                    char::from_u32(combined).ok_or(JsonEditError::Malformed)?,
                                );
                            } else {
                                decoded.push(
                                    char::from_u32(u32::from(first))
                                        .ok_or(JsonEditError::Malformed)?,
                                );
                            }
                        }
                        _ => return Err(JsonEditError::Malformed),
                    }
                }
                0x00..=0x1f => return Err(JsonEditError::Malformed),
                _ => {
                    let rest = std::str::from_utf8(&self.bytes[self.pos..])
                        .map_err(|_| JsonEditError::Malformed)?;
                    let character = rest.chars().next().ok_or(JsonEditError::Malformed)?;
                    decoded.push(character);
                    self.pos += character.len_utf8();
                }
            }
        }
    }

    fn parse_hex4(&mut self) -> Result<u16, JsonEditError> {
        if self.pos + 4 > self.bytes.len() {
            return Err(JsonEditError::Malformed);
        }
        let digits = std::str::from_utf8(&self.bytes[self.pos..self.pos + 4])
            .map_err(|_| JsonEditError::Malformed)?;
        let value = u16::from_str_radix(digits, 16).map_err(|_| JsonEditError::Malformed)?;
        self.pos += 4;
        Ok(value)
    }

    fn parse_literal(&mut self) -> Result<(), JsonEditError> {
        for literal in [b"true".as_slice(), b"false", b"null"] {
            if self.bytes[self.pos..].starts_with(literal) {
                self.pos += literal.len();
                return Ok(());
            }
        }
        Err(JsonEditError::Malformed)
    }

    fn parse_number(&mut self) -> Result<(), JsonEditError> {
        let start = self.pos;
        if self.peek() == Some(b'-') {
            self.pos += 1;
        }
        match self.peek() {
            Some(b'0') => self.pos += 1,
            Some(byte) if byte.is_ascii_digit() => {
                while self.peek().is_some_and(|byte| byte.is_ascii_digit()) {
                    self.pos += 1;
                }
            }
            _ => return Err(JsonEditError::Malformed),
        }
        if self.peek() == Some(b'.') {
            self.pos += 1;
            if !self.peek().is_some_and(|byte| byte.is_ascii_digit()) {
                return Err(JsonEditError::Malformed);
            }
            while self.peek().is_some_and(|byte| byte.is_ascii_digit()) {
                self.pos += 1;
            }
        }
        if matches!(self.peek(), Some(b'e' | b'E')) {
            self.pos += 1;
            if matches!(self.peek(), Some(b'+' | b'-')) {
                self.pos += 1;
            }
            if !self.peek().is_some_and(|byte| byte.is_ascii_digit()) {
                return Err(JsonEditError::Malformed);
            }
            while self.peek().is_some_and(|byte| byte.is_ascii_digit()) {
                self.pos += 1;
            }
        }
        if self.pos == start {
            return Err(JsonEditError::Malformed);
        }
        Ok(())
    }

    fn skip_trivia(&mut self) -> Result<(), JsonEditError> {
        loop {
            while self
                .peek()
                .is_some_and(|byte| matches!(byte, b' ' | b'\t' | b'\n' | b'\r'))
            {
                self.pos += 1;
            }
            if self.peek() == Some(b'/') {
                match self.bytes.get(self.pos + 1) {
                    Some(b'/') => {
                        if self.dialect == Dialect::Json {
                            return Err(JsonEditError::StrictComment);
                        }
                        while self.peek().is_some_and(|byte| byte != b'\n') {
                            self.pos += 1;
                        }
                    }
                    Some(b'*') => {
                        if self.dialect == Dialect::Json {
                            return Err(JsonEditError::StrictComment);
                        }
                        self.pos += 2;
                        while self.pos + 1 < self.bytes.len()
                            && !(self.bytes[self.pos] == b'*' && self.bytes[self.pos + 1] == b'/')
                        {
                            self.pos += 1;
                        }
                        if self.pos + 1 >= self.bytes.len() {
                            return Err(JsonEditError::Malformed);
                        }
                        self.pos += 2;
                    }
                    _ => return Ok(()),
                }
            } else {
                return Ok(());
            }
        }
    }

    fn peek(&self) -> Option<u8> {
        self.bytes.get(self.pos).copied()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strict_and_jsonc_are_two_dialects() {
        let with_comment = "{\n  // note\n  \"a\": 1\n}";
        assert_eq!(
            JsonDocument::parse(with_comment, Dialect::Json).unwrap_err(),
            JsonEditError::StrictComment
        );
        assert!(JsonDocument::parse(with_comment, Dialect::Jsonc).is_ok());

        let trailing = "{\"a\": 1,}";
        assert_eq!(
            JsonDocument::parse(trailing, Dialect::Json).unwrap_err(),
            JsonEditError::StrictTrailingComma
        );
        assert!(JsonDocument::parse(trailing, Dialect::Jsonc).is_ok());
    }

    #[test]
    fn a_duplicate_key_is_refused_in_both_dialects() {
        assert_eq!(
            JsonDocument::parse(r#"{"a":1,"a":2}"#, Dialect::Json).unwrap_err(),
            JsonEditError::DuplicateKey
        );
        assert_eq!(
            JsonDocument::parse("{\"a\":1,//x\n\"a\":2}", Dialect::Jsonc).unwrap_err(),
            JsonEditError::DuplicateKey
        );
    }

    #[test]
    fn inserting_into_an_empty_object_keeps_it_well_formed() {
        let mut document = JsonDocument::parse("{}", Dialect::Json).expect("parse");
        document
            .insert_member(&[], "kontor", r#"{"command":"/x"}"#)
            .expect("insert");
        assert_eq!(document.text(), "{\n  \"kontor\": {\"command\":\"/x\"}\n}");
    }

    #[test]
    fn inserting_preserves_every_other_byte_exactly() {
        let original =
            "{\n  // keep me\n  \"alpha\": [1, 2, {\"deep\": true}],\n  \"beta\": \"v\"\n}\n";
        let mut document = JsonDocument::parse(original, Dialect::Jsonc).expect("parse");
        document
            .insert_member(&[], "kontor", r#"{"x":1}"#)
            .expect("insert");
        assert_eq!(
            document.text(),
            "{\n  // keep me\n  \"alpha\": [1, 2, {\"deep\": true}],\n  \"beta\": \"v\",\n  \"kontor\": {\"x\":1}\n}\n"
        );
    }

    #[test]
    fn inserting_into_a_nested_object_creates_nothing_but_the_leaf() {
        let original = "{\n\t\"mcp\": {\n\t\t\"other\": {\"a\": 1}\n\t}\n}";
        let mut document = JsonDocument::parse(original, Dialect::Json).expect("parse");
        document
            .insert_member(&["mcp"], "servers", "{}")
            .expect("insert servers");
        document
            .insert_member(&["mcp", "servers"], "kontor", r#"{"type":"local"}"#)
            .expect("insert kontor");
        assert_eq!(
            document.text(),
            "{\n\t\"mcp\": {\n\t\t\"other\": {\"a\": 1},\n\t\t\"servers\": {\n\t\t\t\"kontor\": {\"type\":\"local\"}\n\t\t}\n\t}\n}"
        );
    }

    #[test]
    fn a_jsonc_trailing_comma_is_edited_in_place() {
        let original = "{\n  \"a\": 1,\n}";
        let mut document = JsonDocument::parse(original, Dialect::Jsonc).expect("parse");
        document
            .insert_member(&[], "kontor", "{\"x\":1}")
            .expect("insert");
        assert_eq!(
            document.text(),
            "{\n  \"a\": 1,\n  \"kontor\": {\"x\":1},\n}"
        );
        assert!(JsonDocument::parse(document.text(), Dialect::Jsonc).is_ok());
    }

    #[test]
    fn replacing_a_value_preserves_the_surroundings() {
        let original = "{\n  \"kontor\": { \"command\": \"/old\" }, // tail\n  \"keep\": 2\n}";
        let mut document = JsonDocument::parse(original, Dialect::Jsonc).expect("parse");
        document
            .replace_value(&["kontor"], r#"{"command":"/new"}"#)
            .expect("replace");
        assert_eq!(
            document.text(),
            "{\n  \"kontor\": {\"command\":\"/new\"}, // tail\n  \"keep\": 2\n}"
        );
        assert_eq!(
            document.value(&["kontor"]).expect("value")["command"],
            "/new"
        );
    }

    #[test]
    fn escaped_keys_are_compared_decoded() {
        let document =
            JsonDocument::parse(r#"{"m\u0063p": {"a": 1}}"#, Dialect::Json).expect("parse");
        assert!(document.has_member(&["mcp"]));
        assert_eq!(document.value(&["mcp"]).expect("mcp")["a"], 1);
    }

    #[test]
    fn malformed_documents_are_refused() {
        for bad in [
            "",
            "{",
            "{\"a\" 1}",
            "[1,]",
            "{\"a\": 01}",
            "\"unterminated",
        ] {
            let result = JsonDocument::parse(bad, Dialect::Json);
            assert!(result.is_err(), "{bad:?} must not parse");
        }
    }
}
