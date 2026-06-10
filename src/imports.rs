//! Extract a Rust file's declared dependencies as a flat list of import paths.
//!
//! [`extract_imports`] turns a Rust source string into the fully-qualified paths
//! of its `use` declarations — the second half of context gathering, the file's
//! declared dependencies. Only `use` declarations are considered; a grouped
//! `use a::{b, c}` expands deterministically into one concrete path per member.
//! A file with no imports yields an empty list, while input that cannot be parsed
//! becomes [`MendError::Parse`] rather than a panic.

use syn::{Item, UseTree};

use crate::{MendError, Result};

/// Extract the fully-qualified path of every `use` declaration from Rust `source`.
///
/// Returns a flat list with one string per concrete import path, in source order.
/// A grouped `use a::{b, c}` expands to `a::b` and `a::c`; each returned string is
/// a single concrete path, never a raw group. A file with no imports yields an
/// empty list; source that fails to parse yields [`MendError::Parse`].
pub fn extract_imports(source: &str) -> Result<Vec<String>> {
    let file = syn::parse_file(source).map_err(|e| MendError::Parse(e.to_string()))?;
    let mut paths = Vec::new();
    for item in &file.items {
        if let Item::Use(u) = item {
            collect_paths(&u.tree, String::new(), &mut paths);
        }
    }
    Ok(paths)
}

/// Walk a `use` tree, appending one fully-qualified path per leaf to `out`.
///
/// `prefix` is the path accumulated from enclosing `Path`/`Group` nodes. A `Path`
/// node extends the prefix and recurses; a `Name`/`Rename` leaf emits the prefix
/// joined with its identifier; a `Group` recurses into each member with the same
/// prefix; a glob contributes no concrete name.
fn collect_paths(tree: &UseTree, prefix: String, out: &mut Vec<String>) {
    match tree {
        UseTree::Path(p) => collect_paths(&p.tree, join(prefix, &p.ident.to_string()), out),
        UseTree::Name(n) => out.push(join(prefix, &n.ident.to_string())),
        UseTree::Rename(r) => out.push(join(prefix, &r.ident.to_string())),
        UseTree::Group(g) => {
            for member in &g.items {
                collect_paths(member, prefix.clone(), out);
            }
        }
        UseTree::Glob(_) => {}
    }
}

/// Append `segment` to a `::`-separated `prefix`, or return it bare if the prefix
/// is empty (the first segment of a path).
fn join(prefix: String, segment: &str) -> String {
    if prefix.is_empty() {
        segment.to_string()
    } else {
        format!("{prefix}::{segment}")
    }
}
