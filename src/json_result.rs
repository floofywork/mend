//! The machine-readable output mode: serialise a mend result to one JSON object
//! and parse it back.
//!
//! A [`JsonResult`] bundles everything a caller needs to reconstruct what mend
//! did to a file: the `path`, the `original` text, the `proposed` text, the list
//! of [`ParsedEdit`]s, and an `applied` flag recording whether the proposed text
//! was written to disk. [`emit_json`] turns that struct into one self-contained
//! JSON object; [`parse_json`] turns the object back into the struct. The two are
//! exact inverses — emitting then parsing reconstructs the original value field
//! for field, and re-emitting the parsed value reproduces byte-identical JSON —
//! so the JSON fully captures the result. This is fenced off from the human diff
//! path ([`crate::render_diff`]): no colour, no human-readable diff, and no
//! partial or streaming output — one object, all at once.

use crate::{MendError, ParsedEdit, Result};
use std::iter::Peekable;
use std::str::Chars;

/// A mend result rendered for machine consumption.
///
/// Every field the JSON carries: the file `path`, its `original` and `proposed`
/// text, the `edits` that bridge them, and an `applied` flag stating whether the
/// proposed text was written. [`emit_json`] and [`parse_json`] round-trip this
/// struct exactly, so it is the canonical shape the JSON reconstructs.
#[derive(Debug, PartialEq)]
pub struct JsonResult {
    /// The path of the file the result concerns.
    pub path: String,
    /// The file's text before the edits.
    pub original: String,
    /// The file's text after the edits.
    pub proposed: String,
    /// The edits that turn `original` into `proposed`, in order.
    pub edits: Vec<ParsedEdit>,
    /// Whether `proposed` was written back to disk.
    pub applied: bool,
}

/// Serialise `result` to one JSON object.
///
/// The object carries every field — `path`, `original`, `proposed`, the `edits`
/// list, and the `applied` boolean — with text values JSON-escaped. The layout is
/// fixed and whitespace-free, so the output is deterministic: re-emitting a parsed
/// result reproduces the identical string. Symmetric with [`parse_json`].
pub fn emit_json(result: &JsonResult) -> String {
    let edits: Vec<String> = result.edits.iter().map(emit_edit).collect();

    let mut out = String::new();
    out.push_str("{\"path\":\"");
    out.push_str(&escape(&result.path));
    out.push_str("\",\"original\":\"");
    out.push_str(&escape(&result.original));
    out.push_str("\",\"proposed\":\"");
    out.push_str(&escape(&result.proposed));
    out.push_str("\",\"edits\":[");
    out.push_str(&edits.join(","));
    out.push_str("],\"applied\":");
    out.push_str(&result.applied.to_string());
    out.push('}');
    out
}

/// Serialise a single [`ParsedEdit`] to a JSON object, tagged by its shape.
fn emit_edit(edit: &ParsedEdit) -> String {
    match edit {
        ParsedEdit::SearchReplace { search, replace } => format!(
            "{{\"type\":\"search_replace\",\"search\":\"{}\",\"replace\":\"{}\"}}",
            escape(search),
            escape(replace)
        ),
        ParsedEdit::WholeFile { body } => {
            format!("{{\"type\":\"whole_file\",\"body\":\"{}\"}}", escape(body))
        }
    }
}

/// Escape a string for embedding inside a JSON string literal: backslash, double
/// quote, and newline become their two-character escape sequences. The backslash
/// is escaped first so the escapes introduced for the quote and newline are not
/// themselves doubled.
fn escape(s: &str) -> String {
    s.replace('\\', "\\\\")
        .replace('"', "\\\"")
        .replace('\n', "\\n")
}

/// Parse the JSON produced by [`emit_json`] back into a [`JsonResult`].
///
/// The object is read field by field; recognised keys populate the struct and any
/// unknown key is ignored. The reconstruction is exact: parsing the output of
/// [`emit_json`] yields a value equal to the original. A string that is not the
/// expected one-object shape is [`MendError::Parse`].
pub fn parse_json(json: &str) -> Result<JsonResult> {
    let mut it = json.chars().peekable();
    let pairs = match parse_value(&mut it)? {
        Json::Obj(pairs) => pairs,
        _ => return Err(parse_err()),
    };

    let mut path = String::new();
    let mut original = String::new();
    let mut proposed = String::new();
    let mut edits = Vec::new();
    let mut applied = false;
    for (key, val) in pairs {
        match key.as_str() {
            "path" => path = as_string(val)?,
            "original" => original = as_string(val)?,
            "proposed" => proposed = as_string(val)?,
            "edits" => edits = as_edits(val)?,
            "applied" => applied = as_bool(val)?,
            _ => {}
        }
    }

    Ok(JsonResult {
        path,
        original,
        proposed,
        edits,
        applied,
    })
}

/// A parsed JSON value, limited to the shapes [`emit_json`] produces.
enum Json {
    Str(String),
    Bool(bool),
    Arr(Vec<Json>),
    Obj(Vec<(String, Json)>),
}

/// The single parse failure this module raises.
fn parse_err() -> MendError {
    MendError::Parse("malformed JSON result".to_string())
}

/// Parse one JSON value, dispatching on its opening character.
fn parse_value(it: &mut Peekable<Chars<'_>>) -> Result<Json> {
    match it.peek().copied() {
        Some('"') => Ok(Json::Str(parse_string(it)?)),
        Some('[') => Ok(Json::Arr(parse_array(it)?)),
        Some('{') => Ok(Json::Obj(parse_object(it)?)),
        Some('t') => {
            it.next();
            it.next();
            it.next();
            it.next();
            Ok(Json::Bool(true))
        }
        Some('f') => {
            it.next();
            it.next();
            it.next();
            it.next();
            it.next();
            Ok(Json::Bool(false))
        }
        _ => Err(parse_err()),
    }
}

/// Parse a JSON string literal, consuming the opening quote through the closing
/// quote and unescaping the body.
fn parse_string(it: &mut Peekable<Chars<'_>>) -> Result<String> {
    it.next(); // opening quote
    let mut raw = String::new();
    loop {
        match it.next() {
            Some('"') => return Ok(unescape(&raw)),
            Some('\\') => {
                // Keep the escape pair verbatim so an escaped quote does not look
                // like the terminator; `unescape` resolves it afterwards.
                raw.push('\\');
                match it.next() {
                    Some(c) => raw.push(c),
                    None => return Err(parse_err()),
                }
            }
            Some(c) => raw.push(c),
            None => return Err(parse_err()),
        }
    }
}

/// Parse a non-empty JSON array of values.
fn parse_array(it: &mut Peekable<Chars<'_>>) -> Result<Vec<Json>> {
    it.next(); // opening bracket
    let mut items = Vec::new();
    loop {
        items.push(parse_value(it)?);
        match it.next() {
            Some(',') => {}
            _ => return Ok(items), // closing bracket
        }
    }
}

/// Parse a non-empty JSON object into its key/value pairs, in order.
fn parse_object(it: &mut Peekable<Chars<'_>>) -> Result<Vec<(String, Json)>> {
    it.next(); // opening brace
    let mut pairs = Vec::new();
    loop {
        let key = parse_string(it)?;
        it.next(); // colon
        let val = parse_value(it)?;
        pairs.push((key, val));
        match it.next() {
            Some(',') => {}
            _ => return Ok(pairs), // closing brace
        }
    }
}

/// Reverse [`escape`]: turn the two-character escape sequences back into the
/// characters they stand for.
fn unescape(s: &str) -> String {
    let mut out = String::new();
    let mut it = s.chars();
    while let Some(c) = it.next() {
        match c {
            '\\' => match it.next() {
                Some('n') => out.push('\n'),
                Some(other) => out.push(other),
                None => out.push('\\'),
            },
            other => out.push(other),
        }
    }
    out
}

/// Expect a JSON string value.
fn as_string(v: Json) -> Result<String> {
    match v {
        Json::Str(s) => Ok(s),
        _ => Err(parse_err()),
    }
}

/// Expect a JSON boolean value.
fn as_bool(v: Json) -> Result<bool> {
    match v {
        Json::Bool(b) => Ok(b),
        _ => Err(parse_err()),
    }
}

/// Expect a JSON array of edit objects.
fn as_edits(v: Json) -> Result<Vec<ParsedEdit>> {
    let items = match v {
        Json::Arr(items) => items,
        _ => return Err(parse_err()),
    };
    let mut edits = Vec::new();
    for item in items {
        edits.push(as_edit(item)?);
    }
    Ok(edits)
}

/// Reconstruct one [`ParsedEdit`] from its JSON object, dispatching on `type`.
fn as_edit(v: Json) -> Result<ParsedEdit> {
    let pairs = match v {
        Json::Obj(pairs) => pairs,
        _ => return Err(parse_err()),
    };
    let mut kind = String::new();
    let mut search = String::new();
    let mut replace = String::new();
    let mut body = String::new();
    for (key, val) in pairs {
        match key.as_str() {
            "type" => kind = as_string(val)?,
            "search" => search = as_string(val)?,
            "replace" => replace = as_string(val)?,
            "body" => body = as_string(val)?,
            _ => {}
        }
    }
    match kind.as_str() {
        "search_replace" => Ok(ParsedEdit::SearchReplace { search, replace }),
        "whole_file" => Ok(ParsedEdit::WholeFile { body }),
        _ => Err(parse_err()),
    }
}
