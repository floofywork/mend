//! Extract a Rust file's public API surface as an ordered list of signatures.
//!
//! [`extract_structure`] turns a Rust source string into the signatures of its
//! top-level public `fn`, `struct`, `enum`, and `trait` items — the first half
//! of context gathering, a structural summary of the target. Only the public
//! surface is ever included; bodies are dropped, keeping signatures only. Empty
//! input is valid and yields an empty list, while input that cannot be parsed
//! becomes [`MendError::Parse`] rather than a panic.

use quote::ToTokens;
use syn::{Generics, Ident, Item, Visibility};

use crate::{MendError, Result};

/// Extract the signatures of every top-level public item from Rust `source`.
///
/// Returns an ordered list with one signature string per public `fn`, `struct`,
/// `enum`, or `trait`, in source order. Items not marked `pub` are excluded and
/// item bodies are never included. An empty file yields an empty list; source
/// that fails to parse yields [`MendError::Parse`].
pub fn extract_structure(source: &str) -> Result<Vec<String>> {
    let file = syn::parse_file(source).map_err(|e| MendError::Parse(e.to_string()))?;
    Ok(file.items.iter().filter_map(item_signature).collect())
}

/// The signature of `item` if it is a public `fn`/`struct`/`enum`/`trait`, else
/// `None`. A function keeps its full declaration (and so its return type) but
/// not its body; the other kinds reduce to their declaration header.
fn item_signature(item: &Item) -> Option<String> {
    match item {
        Item::Fn(f) if is_public(&f.vis) => Some(f.sig.to_token_stream().to_string()),
        Item::Struct(s) if is_public(&s.vis) => Some(header("struct", &s.ident, &s.generics)),
        Item::Enum(e) if is_public(&e.vis) => Some(header("enum", &e.ident, &e.generics)),
        Item::Trait(t) if is_public(&t.vis) => Some(header("trait", &t.ident, &t.generics)),
        _ => None,
    }
}

/// Whether `vis` marks an item as fully public (`pub`).
fn is_public(vis: &Visibility) -> bool {
    matches!(vis, Visibility::Public(_))
}

/// A declaration header — `keyword ident generics` — with no body.
fn header(keyword: &str, ident: &Ident, generics: &Generics) -> String {
    format!("{} {}{}", keyword, ident, generics.to_token_stream())
}
