use core::fmt::{self, Write};
use thiserror::Error;

/// A byte range and its one-based source location in a BSN document.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct BsnSpan {
    /// Inclusive byte offset.
    pub start: usize,
    /// Exclusive byte offset.
    pub end: usize,
    /// One-based line.
    pub line: usize,
    /// One-based column.
    pub column: usize,
}

/// A parsed `.bsn` document containing one root entity.
#[derive(Debug, Clone, PartialEq)]
pub struct BsnDocument {
    /// The root entity described by this file.
    pub root: BsnEntity,
}

impl BsnDocument {
    /// Parses a data-only BSN document.
    pub fn parse(source: &str) -> Result<Self, BsnParseError> {
        parse_bsn(source)
    }

    /// Writes this document using canonical indentation and punctuation.
    pub fn to_bsn_string(&self) -> String {
        format_bsn(self)
    }
}

impl fmt::Display for BsnDocument {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&format_bsn(self))
    }
}

/// One entity in a BSN hierarchy.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct BsnEntity {
    /// Optional `#Name` declaration.
    pub name: Option<String>,
    /// Optional cached `:"path.bsn"` scene included before local entries.
    pub cached_scene: Option<String>,
    /// Reflected components on this entity.
    pub components: Vec<BsnComponent>,
    /// Entities in the built-in `Children` relationship.
    pub children: Vec<BsnEntity>,
    /// Span covering this entity's entries.
    pub span: BsnSpan,
}

/// A reflected component constructor.
#[derive(Debug, Clone, PartialEq)]
pub struct BsnComponent {
    /// Registered full or unambiguous short type path.
    pub type_path: String,
    /// Unit, tuple, or named component body.
    pub body: BsnComponentBody,
    /// Source span covering the component.
    pub span: BsnSpan,
}

/// The supported component constructor shapes.
#[derive(Debug, Clone, PartialEq)]
pub enum BsnComponentBody {
    /// `Component`
    Unit,
    /// `Component(value, ...)`
    Tuple(Vec<BsnValue>),
    /// `Component { field: value, ... }`
    Struct(Vec<BsnStructField>),
}

impl BsnComponentBody {
    pub(crate) fn as_value(&self) -> BsnValue {
        match self {
            Self::Unit => BsnValue::Unit,
            Self::Tuple(values) => BsnValue::Tuple(values.clone()),
            Self::Struct(fields) => BsnValue::Struct {
                type_path: None,
                fields: fields.clone(),
            },
        }
    }
}

/// One named reflected field.
#[derive(Debug, Clone, PartialEq)]
pub struct BsnStructField {
    /// Rust field name.
    pub name: String,
    /// Field value.
    pub value: BsnValue,
    /// Source span covering the field.
    pub span: BsnSpan,
}

/// A runtime-safe reflected BSN value.
#[derive(Debug, Clone, PartialEq)]
pub enum BsnValue {
    /// `()`
    Unit,
    /// `true` / `false`
    Bool(bool),
    /// A Rust numeric literal, retained losslessly until type-directed deserialization.
    Number(String),
    /// A UTF-8 string literal.
    String(String),
    /// A character literal.
    Char(char),
    /// A unit constructor or enum variant path.
    Path(String),
    /// `(value, ...)`
    Tuple(Vec<BsnValue>),
    /// `[value, ...]`
    List(Vec<BsnValue>),
    /// `{ key: value, ... }`
    Map(Vec<(BsnValue, BsnValue)>),
    /// `Type { field: value, ... }`, or a component body when `type_path` is `None`.
    Struct {
        /// Optional constructor path.
        type_path: Option<String>,
        /// Named fields.
        fields: Vec<BsnStructField>,
    },
    /// `Type(value, ...)`
    Constructor {
        /// Constructor or enum variant path.
        type_path: String,
        /// Positional fields.
        fields: Vec<BsnValue>,
    },
}

/// A syntax error with a stable source location.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
#[error("{message} at {line}:{column}")]
pub struct BsnParseError {
    /// Human-readable error.
    pub message: String,
    /// One-based line.
    pub line: usize,
    /// One-based column.
    pub column: usize,
    /// Byte span associated with the error.
    pub span: BsnSpan,
}

impl BsnParseError {
    fn new(message: impl Into<String>, span: BsnSpan) -> Self {
        Self {
            message: message.into(),
            line: span.line,
            column: span.column,
            span,
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
enum TokenKind {
    Ident(String),
    Number(String),
    String(String),
    Char(char),
    Hash,
    Colon,
    ColonColon,
    Comma,
    LParen,
    RParen,
    LBrace,
    RBrace,
    LBracket,
    RBracket,
    Eof,
}

#[derive(Debug, Clone)]
struct Token {
    kind: TokenKind,
    span: BsnSpan,
}

struct Lexer<'a> {
    source: &'a str,
    offset: usize,
    line: usize,
    column: usize,
}

impl<'a> Lexer<'a> {
    fn new(source: &'a str) -> Self {
        Self {
            source,
            offset: 0,
            line: 1,
            column: 1,
        }
    }

    fn tokenize(mut self) -> Result<Vec<Token>, BsnParseError> {
        let mut tokens = Vec::new();
        loop {
            self.skip_trivia()?;
            let start = self.position();
            let Some(character) = self.peek() else {
                tokens.push(Token {
                    kind: TokenKind::Eof,
                    span: start,
                });
                return Ok(tokens);
            };
            let kind = match character {
                '#' => {
                    self.bump();
                    TokenKind::Hash
                }
                ':' => {
                    self.bump();
                    if self.peek() == Some(':') {
                        self.bump();
                        TokenKind::ColonColon
                    } else {
                        TokenKind::Colon
                    }
                }
                ',' => {
                    self.bump();
                    TokenKind::Comma
                }
                '(' => {
                    self.bump();
                    TokenKind::LParen
                }
                ')' => {
                    self.bump();
                    TokenKind::RParen
                }
                '{' => {
                    self.bump();
                    TokenKind::LBrace
                }
                '}' => {
                    self.bump();
                    TokenKind::RBrace
                }
                '[' => {
                    self.bump();
                    TokenKind::LBracket
                }
                ']' => {
                    self.bump();
                    TokenKind::RBracket
                }
                '"' => TokenKind::String(self.read_string('"')?),
                '\'' => TokenKind::Char(self.read_char()?),
                '-' if self.peek_second().is_some_and(|next| next.is_ascii_digit()) => {
                    TokenKind::Number(self.read_number())
                }
                value if value.is_ascii_digit() => TokenKind::Number(self.read_number()),
                value if is_ident_start(value) => TokenKind::Ident(self.read_ident()),
                '@' => {
                    return Err(BsnParseError::new(
                        "runtime BSN files do not support `@` scene components",
                        start,
                    ));
                }
                '|' => {
                    return Err(BsnParseError::new(
                        "runtime BSN files do not support closures",
                        start,
                    ));
                }
                '!' => {
                    return Err(BsnParseError::new(
                        "runtime BSN files do not support macro calls",
                        start,
                    ));
                }
                _ => {
                    return Err(BsnParseError::new(
                        format!(
                            "unsupported token `{character}`; runtime BSN accepts data values, not Rust expressions"
                        ),
                        start,
                    ));
                }
            };
            tokens.push(Token {
                kind,
                span: self.finish_span(start),
            });
        }
    }

    fn skip_trivia(&mut self) -> Result<(), BsnParseError> {
        loop {
            while self.peek().is_some_and(char::is_whitespace) {
                self.bump();
            }
            if self.remaining().starts_with("//") {
                while self.peek().is_some_and(|character| character != '\n') {
                    self.bump();
                }
                continue;
            }
            if self.remaining().starts_with("/*") {
                let start = self.position();
                self.bump();
                self.bump();
                let mut depth = 1usize;
                while depth > 0 {
                    if self.remaining().starts_with("/*") {
                        self.bump();
                        self.bump();
                        depth += 1;
                    } else if self.remaining().starts_with("*/") {
                        self.bump();
                        self.bump();
                        depth -= 1;
                    } else if self.bump().is_none() {
                        return Err(BsnParseError::new("unterminated block comment", start));
                    }
                }
                continue;
            }
            return Ok(());
        }
    }

    fn read_ident(&mut self) -> String {
        let start = self.offset;
        self.bump();
        while self.peek().is_some_and(is_ident_continue) {
            self.bump();
        }
        self.source[start..self.offset].to_string()
    }

    fn read_number(&mut self) -> String {
        let start = self.offset;
        if self.peek() == Some('-') {
            self.bump();
        }
        while self.peek().is_some_and(|character| {
            character.is_ascii_alphanumeric() || matches!(character, '.' | '_' | '+' | '-')
        }) {
            self.bump();
        }
        self.source[start..self.offset].to_string()
    }

    fn read_string(&mut self, delimiter: char) -> Result<String, BsnParseError> {
        let start = self.position();
        self.bump();
        let mut result = String::new();
        loop {
            match self.bump() {
                Some(character) if character == delimiter => return Ok(result),
                Some('\\') => result.push(self.read_escape(start)?),
                Some('\n' | '\r') => {
                    return Err(BsnParseError::new("newline in string literal", start));
                }
                Some(character) => result.push(character),
                None => return Err(BsnParseError::new("unterminated string literal", start)),
            }
        }
    }

    fn read_char(&mut self) -> Result<char, BsnParseError> {
        let start = self.position();
        self.bump();
        let value = match self.bump() {
            Some('\\') => self.read_escape(start)?,
            Some('\n' | '\r' | '\'') | None => {
                return Err(BsnParseError::new("invalid character literal", start));
            }
            Some(character) => character,
        };
        if self.bump() != Some('\'') {
            return Err(BsnParseError::new(
                "character literal must contain one character",
                start,
            ));
        }
        Ok(value)
    }

    fn read_escape(&mut self, start: BsnSpan) -> Result<char, BsnParseError> {
        match self.bump() {
            Some('n') => Ok('\n'),
            Some('r') => Ok('\r'),
            Some('t') => Ok('\t'),
            Some('0') => Ok('\0'),
            Some('\\') => Ok('\\'),
            Some('"') => Ok('"'),
            Some('\'') => Ok('\''),
            Some('u') if self.bump() == Some('{') => {
                let digits_start = self.offset;
                while self.peek().is_some_and(|character| character != '}') {
                    self.bump();
                }
                let digits = &self.source[digits_start..self.offset];
                if self.bump() != Some('}') {
                    return Err(BsnParseError::new("unterminated unicode escape", start));
                }
                let value = u32::from_str_radix(digits, 16)
                    .ok()
                    .and_then(char::from_u32)
                    .ok_or_else(|| BsnParseError::new("invalid unicode escape", start))?;
                Ok(value)
            }
            _ => Err(BsnParseError::new("unsupported escape sequence", start)),
        }
    }

    fn remaining(&self) -> &'a str {
        &self.source[self.offset..]
    }

    fn peek(&self) -> Option<char> {
        self.remaining().chars().next()
    }

    fn peek_second(&self) -> Option<char> {
        self.remaining().chars().nth(1)
    }

    fn bump(&mut self) -> Option<char> {
        let character = self.peek()?;
        self.offset += character.len_utf8();
        if character == '\n' {
            self.line += 1;
            self.column = 1;
        } else {
            self.column += 1;
        }
        Some(character)
    }

    fn position(&self) -> BsnSpan {
        BsnSpan {
            start: self.offset,
            end: self.offset,
            line: self.line,
            column: self.column,
        }
    }

    fn finish_span(&self, start: BsnSpan) -> BsnSpan {
        BsnSpan {
            end: self.offset,
            ..start
        }
    }
}

fn is_ident_start(character: char) -> bool {
    character == '_' || character.is_alphabetic()
}

fn is_ident_continue(character: char) -> bool {
    character == '_' || character.is_alphanumeric()
}

struct Parser {
    tokens: Vec<Token>,
    index: usize,
}

impl Parser {
    fn parse(mut self) -> Result<BsnDocument, BsnParseError> {
        let root = if self.at(&TokenKind::LParen) {
            self.bump();
            let root = self.parse_entity(&[TokenKind::RParen])?;
            self.expect(&TokenKind::RParen, "expected `)` after root entity")?;
            root
        } else {
            self.parse_entity(&[TokenKind::Eof])?
        };
        self.expect(&TokenKind::Eof, "unexpected trailing tokens")?;
        Ok(BsnDocument { root })
    }

    fn parse_entity(&mut self, stops: &[TokenKind]) -> Result<BsnEntity, BsnParseError> {
        let start = self.peek().span;
        let mut entity = BsnEntity::default();
        while !stops.iter().any(|stop| self.at(stop)) {
            if self.at(&TokenKind::Eof) {
                return Err(self.error("unexpected end of BSN entity"));
            }
            if self.at(&TokenKind::Hash) {
                let hash = self.bump().span;
                let (name, _) = self.expect_ident("expected an identifier after `#`")?;
                if entity.name.replace(name).is_some() {
                    return Err(BsnParseError::new(
                        "an entity can only declare one `#Name`",
                        hash,
                    ));
                }
                continue;
            }
            if self.at(&TokenKind::Colon) {
                let colon = self.bump().span;
                let path = match &self.bump().kind {
                    TokenKind::String(path) => path.clone(),
                    _ => {
                        return Err(BsnParseError::new(
                            "cached scenes use `:\"path.bsn\"`",
                            colon,
                        ));
                    }
                };
                if entity.cached_scene.replace(path).is_some() {
                    return Err(BsnParseError::new(
                        "an entity can include at most one cached scene",
                        colon,
                    ));
                }
                continue;
            }

            let component_start = self.peek().span;
            let type_path = self.parse_path("expected a component type path")?;
            if type_path == "Children" && self.at(&TokenKind::LBracket) {
                entity.children.extend(self.parse_children()?);
                continue;
            }
            let body = if self.at(&TokenKind::LParen) {
                BsnComponentBody::Tuple(
                    self.parse_value_list(TokenKind::LParen, TokenKind::RParen)?,
                )
            } else if self.at(&TokenKind::LBrace) {
                BsnComponentBody::Struct(self.parse_struct_fields()?)
            } else {
                BsnComponentBody::Unit
            };
            let end = self.previous_span().end;
            entity.components.push(BsnComponent {
                type_path,
                body,
                span: BsnSpan {
                    end,
                    ..component_start
                },
            });
        }
        entity.span = BsnSpan {
            end: self.peek().span.start,
            ..start
        };
        Ok(entity)
    }

    fn parse_children(&mut self) -> Result<Vec<BsnEntity>, BsnParseError> {
        self.expect(&TokenKind::LBracket, "expected `[` after `Children`")?;
        let mut children = Vec::new();
        while !self.at(&TokenKind::RBracket) {
            if self.at(&TokenKind::Eof) {
                return Err(self.error("unterminated `Children` list"));
            }
            let child = if self.at(&TokenKind::LParen) {
                self.bump();
                let child = self.parse_entity(&[TokenKind::RParen])?;
                self.expect(&TokenKind::RParen, "expected `)` after child entity")?;
                child
            } else {
                self.parse_entity(&[TokenKind::Comma, TokenKind::RBracket])?
            };
            children.push(child);
            if self.at(&TokenKind::Comma) {
                self.bump();
            } else if !self.at(&TokenKind::RBracket) {
                return Err(self.error("expected `,` between child entities"));
            }
        }
        self.bump();
        Ok(children)
    }

    fn parse_struct_fields(&mut self) -> Result<Vec<BsnStructField>, BsnParseError> {
        self.expect(&TokenKind::LBrace, "expected `{`")?;
        let mut fields = Vec::new();
        while !self.at(&TokenKind::RBrace) {
            let start = self.peek().span;
            let (name, _) = self.expect_ident("expected a field name")?;
            self.expect(
                &TokenKind::Colon,
                "runtime BSN does not support field shorthand",
            )?;
            let value = self.parse_value()?;
            fields.push(BsnStructField {
                name,
                value,
                span: BsnSpan {
                    end: self.previous_span().end,
                    ..start
                },
            });
            if self.at(&TokenKind::Comma) {
                self.bump();
            } else if !self.at(&TokenKind::RBrace) {
                return Err(self.error("expected `,` between fields"));
            }
        }
        self.bump();
        Ok(fields)
    }

    fn parse_value(&mut self) -> Result<BsnValue, BsnParseError> {
        let token = self.bump().clone();
        match token.kind {
            TokenKind::Number(value) => Ok(BsnValue::Number(value)),
            TokenKind::String(value) => Ok(BsnValue::String(value)),
            TokenKind::Char(value) => Ok(BsnValue::Char(value)),
            TokenKind::Ident(value) if value == "true" => Ok(BsnValue::Bool(true)),
            TokenKind::Ident(value) if value == "false" => Ok(BsnValue::Bool(false)),
            TokenKind::Ident(first) => {
                let path = self.parse_path_tail(first)?;
                if self.at(&TokenKind::LParen) {
                    Ok(BsnValue::Constructor {
                        type_path: path,
                        fields: self.parse_value_list(TokenKind::LParen, TokenKind::RParen)?,
                    })
                } else if self.at(&TokenKind::LBrace) {
                    Ok(BsnValue::Struct {
                        type_path: Some(path),
                        fields: self.parse_struct_fields()?,
                    })
                } else {
                    Ok(BsnValue::Path(path))
                }
            }
            TokenKind::LParen => {
                if self.at(&TokenKind::RParen) {
                    self.bump();
                    return Ok(BsnValue::Unit);
                }
                let mut values = Vec::new();
                values.push(self.parse_value()?);
                let had_comma = self.at(&TokenKind::Comma);
                while self.at(&TokenKind::Comma) {
                    self.bump();
                    if self.at(&TokenKind::RParen) {
                        break;
                    }
                    values.push(self.parse_value()?);
                }
                self.expect(&TokenKind::RParen, "expected `)` after tuple")?;
                if values.len() == 1 && !had_comma {
                    Ok(values.pop().unwrap())
                } else {
                    Ok(BsnValue::Tuple(values))
                }
            }
            TokenKind::LBracket => {
                let values = self.parse_value_list_after_open(TokenKind::RBracket)?;
                Ok(BsnValue::List(values))
            }
            TokenKind::LBrace => self.parse_map_after_open(),
            TokenKind::Hash => Err(BsnParseError::new(
                "named entity references in reflected values are not supported yet",
                token.span,
            )),
            _ => Err(BsnParseError::new(
                "expected a static BSN value",
                token.span,
            )),
        }
    }

    fn parse_map_after_open(&mut self) -> Result<BsnValue, BsnParseError> {
        let mut entries = Vec::new();
        while !self.at(&TokenKind::RBrace) {
            let key = self.parse_value()?;
            self.expect(&TokenKind::Colon, "expected `:` between map key and value")?;
            let value = self.parse_value()?;
            entries.push((key, value));
            if self.at(&TokenKind::Comma) {
                self.bump();
            } else if !self.at(&TokenKind::RBrace) {
                return Err(self.error("expected `,` between map entries"));
            }
        }
        self.bump();
        Ok(BsnValue::Map(entries))
    }

    fn parse_value_list(
        &mut self,
        open: TokenKind,
        close: TokenKind,
    ) -> Result<Vec<BsnValue>, BsnParseError> {
        self.expect(&open, "expected opening delimiter")?;
        self.parse_value_list_after_open(close)
    }

    fn parse_value_list_after_open(
        &mut self,
        close: TokenKind,
    ) -> Result<Vec<BsnValue>, BsnParseError> {
        let mut values = Vec::new();
        while !self.at(&close) {
            values.push(self.parse_value()?);
            if self.at(&TokenKind::Comma) {
                self.bump();
            } else if !self.at(&close) {
                return Err(self.error("expected `,` between values"));
            }
        }
        self.bump();
        Ok(values)
    }

    fn parse_path(&mut self, message: &str) -> Result<String, BsnParseError> {
        let (first, _) = self.expect_ident(message)?;
        self.parse_path_tail(first)
    }

    fn parse_path_tail(&mut self, mut path: String) -> Result<String, BsnParseError> {
        while self.at(&TokenKind::ColonColon) {
            self.bump();
            let (segment, _) = self.expect_ident("expected a path segment after `::`")?;
            path.push_str("::");
            path.push_str(&segment);
        }
        Ok(path)
    }

    fn expect_ident(&mut self, message: &str) -> Result<(String, BsnSpan), BsnParseError> {
        let token = self.bump().clone();
        match token.kind {
            TokenKind::Ident(value) => Ok((value, token.span)),
            _ => Err(BsnParseError::new(message, token.span)),
        }
    }

    fn expect(&mut self, kind: &TokenKind, message: &str) -> Result<Token, BsnParseError> {
        if self.at(kind) {
            Ok(self.bump().clone())
        } else {
            Err(self.error(message))
        }
    }

    fn at(&self, kind: &TokenKind) -> bool {
        core::mem::discriminant(&self.peek().kind) == core::mem::discriminant(kind)
    }

    fn peek(&self) -> &Token {
        &self.tokens[self.index]
    }

    fn bump(&mut self) -> &Token {
        let index = self.index;
        if !matches!(self.tokens[index].kind, TokenKind::Eof) {
            self.index += 1;
        }
        &self.tokens[index]
    }

    fn previous_span(&self) -> BsnSpan {
        self.tokens[self.index.saturating_sub(1)].span
    }

    fn error(&self, message: impl Into<String>) -> BsnParseError {
        BsnParseError::new(message, self.peek().span)
    }
}

/// Parses the static, runtime-safe subset of Bevy Scene Notation.
pub fn parse_bsn(source: &str) -> Result<BsnDocument, BsnParseError> {
    Parser {
        tokens: Lexer::new(source).tokenize()?,
        index: 0,
    }
    .parse()
}

/// Formats a BSN document using canonical four-space indentation.
pub fn format_bsn(document: &BsnDocument) -> String {
    let mut output = String::new();
    format_entity(&document.root, 0, &mut output);
    output
}

fn format_entity(entity: &BsnEntity, indent: usize, output: &mut String) {
    if let Some(cached) = &entity.cached_scene {
        write_indent(indent, output);
        let _ = writeln!(output, ":{}", quoted(cached));
    }
    if let Some(name) = &entity.name {
        write_indent(indent, output);
        let _ = writeln!(output, "#{name}");
    }
    for component in &entity.components {
        format_component(component, indent, output);
    }
    if !entity.children.is_empty() {
        write_indent(indent, output);
        output.push_str("Children [\n");
        for child in &entity.children {
            write_indent(indent + 1, output);
            output.push_str("(\n");
            format_entity(child, indent + 2, output);
            write_indent(indent + 1, output);
            output.push_str("),\n");
        }
        write_indent(indent, output);
        output.push_str("]\n");
    }
}

fn format_component(component: &BsnComponent, indent: usize, output: &mut String) {
    write_indent(indent, output);
    output.push_str(&component.type_path);
    match &component.body {
        BsnComponentBody::Unit => output.push('\n'),
        BsnComponentBody::Tuple(values) => {
            output.push('(');
            format_inline_values(values, output);
            output.push_str(")\n");
        }
        BsnComponentBody::Struct(fields) => {
            output.push_str(" {\n");
            format_fields(fields, indent + 1, output);
            write_indent(indent, output);
            output.push_str("}\n");
        }
    }
}

fn format_fields(fields: &[BsnStructField], indent: usize, output: &mut String) {
    for field in fields {
        write_indent(indent, output);
        let _ = write!(output, "{}: ", field.name);
        format_value(&field.value, indent, output);
        output.push_str(",\n");
    }
}

fn format_value(value: &BsnValue, indent: usize, output: &mut String) {
    match value {
        BsnValue::Unit => output.push_str("()"),
        BsnValue::Bool(value) => output.push_str(if *value { "true" } else { "false" }),
        BsnValue::Number(value) | BsnValue::Path(value) => output.push_str(value),
        BsnValue::String(value) => output.push_str(&quoted(value)),
        BsnValue::Char(value) => {
            output.push('\'');
            for escaped in value.escape_default() {
                output.push(escaped);
            }
            output.push('\'');
        }
        BsnValue::Tuple(values) => {
            output.push('(');
            format_inline_values(values, output);
            if values.len() == 1 {
                output.push(',');
            }
            output.push(')');
        }
        BsnValue::List(values) => {
            output.push('[');
            format_inline_values(values, output);
            output.push(']');
        }
        BsnValue::Map(entries) => {
            output.push('{');
            for (index, (key, value)) in entries.iter().enumerate() {
                if index > 0 {
                    output.push_str(", ");
                }
                format_value(key, indent, output);
                output.push_str(": ");
                format_value(value, indent, output);
            }
            output.push('}');
        }
        BsnValue::Struct { type_path, fields } => {
            if let Some(type_path) = type_path {
                output.push_str(type_path);
                output.push(' ');
            }
            output.push_str("{\n");
            format_fields(fields, indent + 1, output);
            write_indent(indent, output);
            output.push('}');
        }
        BsnValue::Constructor { type_path, fields } => {
            output.push_str(type_path);
            output.push('(');
            format_inline_values(fields, output);
            output.push(')');
        }
    }
}

fn format_inline_values(values: &[BsnValue], output: &mut String) {
    for (index, value) in values.iter().enumerate() {
        if index > 0 {
            output.push_str(", ");
        }
        format_value(value, 0, output);
    }
}

fn quoted(value: &str) -> String {
    format!("{value:?}")
}

fn write_indent(indent: usize, output: &mut String) {
    for _ in 0..indent {
        output.push_str("    ");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_and_canonically_formats_hierarchy() {
        let source = r#"
            #Root Transform { translation: Vec3(1.0, 2.0, 3.0) }
            Children [
                (#Camera Camera2d),
                #Label Label("hello"),
            ]
        "#;
        let document = parse_bsn(source).unwrap();
        let formatted = format_bsn(&document);
        assert_eq!(format_bsn(&parse_bsn(&formatted).unwrap()), formatted);
        assert!(formatted.contains("#Root\n"));
        assert!(formatted.contains("Children [\n"));
    }

    #[test]
    fn rejects_rust_expressions() {
        let error = parse_bsn("Transform { translation: { 1.0 + 2.0 } }").unwrap_err();
        assert!(error.message.contains("Rust expressions"));
        assert_eq!(error.line, 1);
    }

    #[test]
    fn supports_comments_and_cached_scenes() {
        let document = parse_bsn(":\"base.bsn\" /* nested /* comment */ */ #Root").unwrap();
        assert_eq!(document.root.cached_scene.as_deref(), Some("base.bsn"));
        assert_eq!(document.root.name.as_deref(), Some("Root"));
    }
}
