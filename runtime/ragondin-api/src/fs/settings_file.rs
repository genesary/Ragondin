//! `workspace.toml`'s schema, checked over a `toml_edit` document, and the
//! per-key edits that change the file in place (ADR-C38).
//!
//! ```toml
//! datasets = "/data/benchmarks"   # optional; relative to the workspace
//!
//! [services]
//! "generator/qwen" = "http://127.0.0.1:8080"
//! ```
//!
//! **Reading.** The text is parsed as a [`Document`], which keeps each item's
//! span, and then checked: a top-level `datasets` string, and one standard
//! `[services]` table whose entries are `"<family>/<name>" = <string>`, the
//! key split at its first `/`. Everything else is refused with the line it
//! starts on — another key or table, a sub-table, an array of tables, an
//! inline table, a dotted key, a value that is not a string, a service key
//! without its `/` or with an empty family or name — and so is a parse
//! error, which `toml_edit` reports with its span. A key or a value is read by
//! what it means, never by how it is spelled: a basic, literal or multi-line
//! string is the same string, so the file `toml_edit` writes is always read
//! back.
//!
//! **Writing.** [`edit`] checks the text first, so a refused file is never
//! edited, and then applies one [`Edit`] to the document parsed from it,
//! changing that key and nothing else. Whether an edit is needed at all is
//! decided on the settings, before this module is called
//! (`FsSettings`): an operation that changes no setting never reaches it, so
//! the file is not rewritten. What becomes of the comments around a key an
//! edit touches is `ARCHITECTURE.md` § `workspace.toml`, read and edited in
//! place: none is dropped.

use toml_edit::{Document, DocumentMut, Item, RawString, Table, Value};

/// The settings as the file states them, before the datasets directory is
/// resolved against the workspace.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct SettingsFile {
    /// `datasets`, as written.
    pub(crate) datasets: Option<String>,
    /// `(family, name, uri)`, in the file's order.
    pub(crate) services: Vec<(String, String, String)>,
}

/// Why a file is outside the schema, and on which line (from 1).
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct GrammarError {
    pub(crate) line: usize,
    pub(crate) reason: String,
}

/// One per-key change to the file.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Edit {
    /// `[services]."<family>/<name>" = <uri>`: the address replaced when the
    /// key is there, the key appended to `[services]` when it is not.
    Bind {
        family: String,
        name: String,
        uri: String,
    },
    /// `[services]."<family>/<name>"` removed.
    Unbind { family: String, name: String },
    /// `datasets = <directory>`, as it is to be written.
    SetDatasets(String),
    /// `datasets` removed.
    ClearDatasets,
}

/// The comment a fresh workspace's file holds, and nothing else.
const HEADER: &str = "\
# The workspace's deployment settings, read by `ragondin ui`. Never hashed
# into a run: an address here stays out of every pipeline and run identity.
#
# datasets = \"datasets\"                       # where benchmarks are read;
#                                             # <workspace>/datasets when absent
# [services]
# \"generator/qwen\" = \"http://127.0.0.1:8080\"  # `--remote` bindings
";

const DATASETS: &str = "datasets";
const SERVICES: &str = "services";

/// The file a fresh workspace starts with: the header, and nothing set.
pub(crate) fn empty() -> String {
    HEADER.to_owned()
}

/// Reads `text`.
pub(crate) fn parse(text: &str) -> Result<SettingsFile, GrammarError> {
    check(text, &document(text)?)
}

/// Applies `edit` to `text`, which is checked first, and returns the file
/// with that one key changed.
pub(crate) fn edit(text: &str, edit: &Edit) -> Result<String, GrammarError> {
    let document = document(text)?;
    check(text, &document)?;
    let mut document = document.into_mut();
    match edit {
        Edit::Bind { family, name, uri } => bind(&mut document, &service_key(family, name), uri),
        Edit::Unbind { family, name } => unbind(&mut document, &service_key(family, name)),
        Edit::SetDatasets(datasets) => set_datasets(&mut document, datasets),
        Edit::ClearDatasets => clear_datasets(&mut document),
    }
    Ok(document.to_string())
}

/// `text` parsed, spans kept; a parse error names the line its span starts
/// on.
fn document(text: &str) -> Result<Document<&str>, GrammarError> {
    Document::parse(text).map_err(|error| GrammarError {
        line: error.span().map_or(1, |span| line_at(text, span.start)),
        reason: error.message().to_owned(),
    })
}

/// The key a binding is stated under.
fn service_key(family: &str, name: &str) -> String {
    format!("{family}/{name}")
}

/// The line, from 1, that byte `offset` of `text` is on.
fn line_at(text: &str, offset: usize) -> usize {
    let end = offset.min(text.len());
    text.as_bytes()[..end]
        .iter()
        .filter(|byte| **byte == b'\n')
        .count()
        + 1
}

/// The schema, over a parsed document.
fn check(text: &str, document: &Document<&str>) -> Result<SettingsFile, GrammarError> {
    let mut file = SettingsFile::default();
    let root = document.as_table();
    for (key, item) in root.iter() {
        let refuse = |reason: String| GrammarError {
            line: line_of(text, root, key, item),
            reason,
        };
        match (key, item) {
            (_, Item::Table(table)) if table.is_dotted() => {
                return Err(refuse(format!(
                    "the dotted key `{key}.…`: only `datasets` and the `[services]` table are \
                     read, each written whole"
                )))
            }
            (_, Item::ArrayOfTables(_)) => {
                return Err(refuse(format!(
                    "`[[{key}]]`, an array of tables: only the `[services]` table is read"
                )))
            }
            (_, Item::Value(Value::InlineTable(_))) => {
                return Err(refuse(format!(
                    "`{key}` is an inline table: only the `[services]` table is read, written \
                     as `[services]` on its own line"
                )))
            }
            (DATASETS, Item::Value(value)) => match value.as_str() {
                Some(datasets) => file.datasets = Some(datasets.to_owned()),
                None => {
                    return Err(refuse(format!(
                        "`datasets` is a {}, not a string",
                        value.type_name()
                    )))
                }
            },
            (SERVICES, Item::Table(table)) => {
                file.services = services(text, table)?;
            }
            (_, Item::Table(_)) => {
                return Err(refuse(format!(
                    "the table `[{key}]`: only `[services]` is read"
                )))
            }
            _ => {
                return Err(refuse(format!(
                    "the key `{key}`: only `datasets` and the `[services]` table are read"
                )))
            }
        }
    }
    Ok(file)
}

/// `[services]`'s bindings, in the file's order.
fn services(text: &str, table: &Table) -> Result<Vec<(String, String, String)>, GrammarError> {
    let mut services = Vec::new();
    for (key, item) in table.iter() {
        let refuse = |reason: String| GrammarError {
            line: line_of(text, table, key, item),
            reason,
        };
        let uri = match item {
            Item::Table(sub) if sub.is_dotted() => {
                return Err(refuse(format!(
                    "the dotted key `\"{key}\".…`: a binding is `\"<family>/<name>\" = <string>`"
                )))
            }
            Item::Table(_) => {
                return Err(refuse(format!(
                    "the sub-table `[services.\"{key}\"]`: a binding is `\"<family>/<name>\" = \
                     <string>`"
                )))
            }
            Item::ArrayOfTables(_) => {
                return Err(refuse(format!(
                    "`[[services.\"{key}\"]]`, an array of tables: a binding is \
                     `\"<family>/<name>\" = <string>`"
                )))
            }
            Item::Value(Value::InlineTable(_)) => {
                return Err(refuse(format!(
                    "`{key}` is an inline table: a binding is `\"<family>/<name>\" = <string>`"
                )))
            }
            Item::Value(value) => match value.as_str() {
                Some(uri) => uri.to_owned(),
                None => {
                    return Err(refuse(format!(
                        "`{key}` is a {}, not a string",
                        value.type_name()
                    )))
                }
            },
            Item::None => continue,
        };
        // At the first `/`, as ADR-C32 § 2 splits a binding: a name may hold
        // a `/`, a family may not.
        let Some((family, name)) = key.split_once('/') else {
            return Err(refuse(format!(
                "the service key `{key}` is not `\"<family>/<name>\"`"
            )));
        };
        if family.is_empty() {
            return Err(refuse(format!(
                "the service key `{key}` has an empty family"
            )));
        }
        if name.is_empty() {
            return Err(refuse(format!("the service key `{key}` has an empty name")));
        }
        services.push((family.to_owned(), name.to_owned(), uri));
    }
    Ok(services)
}

/// The line `key` of `table` starts on, from its span — the item's when the
/// key has none.
fn line_of(text: &str, table: &Table, key: &str, item: &Item) -> usize {
    table
        .key(key)
        .and_then(|key| key.span())
        .or_else(|| item.span())
        .map_or(1, |span| line_at(text, span.start))
}

/// A decor part as text: an unset one is empty.
fn raw(part: Option<&RawString>) -> &str {
    part.and_then(RawString::as_str).unwrap_or("")
}

/// `value`'s trailing comment, as a line of its own, or nothing.
fn trailing_comment(value: &Value) -> String {
    let suffix = raw(value.decor().suffix()).trim_start_matches([' ', '\t']);
    if suffix.starts_with('#') {
        format!("{}\n", suffix.trim_end_matches([' ', '\t']))
    } else {
        String::new()
    }
}

/// `replacement` in place of `item`'s value, its decor — the trailing
/// comment — kept.
fn replace_value(item: &mut Item, replacement: &str) {
    let decor = item.as_value().map(|value| value.decor().clone());
    let mut value = Value::from(replacement);
    if let Some(decor) = decor {
        *value.decor_mut() = decor;
    }
    *item = Item::Value(value);
}

/// Takes the document's trailing comments, leaving none.
fn take_trailing(document: &mut DocumentMut) -> String {
    let trailing = document.trailing().as_str().unwrap_or("").to_owned();
    document.set_trailing("");
    trailing
}

fn bind(document: &mut DocumentMut, key: &str, uri: &str) {
    let has_services = document.as_table().get(SERVICES).is_some();
    if !has_services {
        // The comments after the last item — a fresh file's header — stay
        // above the table created after them, a blank line between.
        let trailing = take_trailing(document);
        let separator = if trailing.is_empty() && document.as_table().is_empty() {
            ""
        } else {
            "\n"
        };
        let mut table = Table::new();
        table
            .decor_mut()
            .set_prefix(format!("{trailing}{separator}"));
        document.as_table_mut().insert(SERVICES, Item::Table(table));
    }
    if let Some(item) = services_mut(document).and_then(|table| table.get_mut(key)) {
        replace_value(item, uri);
        return;
    }
    // `[services]` is the document's last table, so a key appended to it is
    // the document's last item: the comments that ended the file stay above
    // it, and the file's bytes so far are unchanged.
    let trailing = take_trailing(document);
    if let Some(table) = services_mut(document) {
        table.insert(key, Item::Value(Value::from(uri)));
        if let Some(mut inserted) = table.key_mut(key) {
            inserted.leaf_decor_mut().set_prefix(trailing);
        }
    }
}

/// `[services]`, when the document has it.
fn services_mut(document: &mut DocumentMut) -> Option<&mut Table> {
    document
        .as_table_mut()
        .get_mut(SERVICES)
        .and_then(Item::as_table_mut)
}

fn unbind(document: &mut DocumentMut, key: &str) {
    let Some(table) = services_mut(document) else {
        return;
    };
    let Some(index) = table.iter().position(|(bound, _)| bound == key) else {
        return;
    };
    let Some((removed, item)) = table.remove_entry(key) else {
        return;
    };
    let comments = format!(
        "{}{}",
        raw(removed.leaf_decor().prefix()),
        item.as_value().map(trailing_comment).unwrap_or_default()
    );
    // Onto the key that followed it, or — the last one — onto the end of the
    // file, `[services]` being its last table.
    let followed = match table.iter_mut().nth(index) {
        Some((mut next, _)) => {
            let prefix = format!("{comments}{}", raw(next.leaf_decor().prefix()));
            next.leaf_decor_mut().set_prefix(prefix);
            true
        }
        None => false,
    };
    if !followed {
        prepend_trailing(document, &comments);
    }
}

/// `comments` placed at the end of the file, before the comments already
/// there.
fn prepend_trailing(document: &mut DocumentMut, comments: &str) {
    let trailing = take_trailing(document);
    document.set_trailing(format!("{comments}{trailing}"));
}

fn set_datasets(document: &mut DocumentMut, datasets: &str) {
    if let Some(item) = document.as_table_mut().get_mut(DATASETS) {
        replace_value(item, datasets);
        return;
    }
    // `datasets` is the document's first item: the comments that opened the
    // file stay above it, and what was first keeps a blank line.
    let services_prefix = services_mut(document).map(|table| {
        let prefix = raw(table.decor().prefix()).to_owned();
        table.decor_mut().set_prefix("\n");
        prefix
    });
    let prefix = match services_prefix {
        Some(prefix) => prefix,
        None => {
            let trailing = take_trailing(document);
            if trailing.is_empty() {
                trailing
            } else {
                format!("{trailing}\n")
            }
        }
    };
    let root = document.as_table_mut();
    root.insert(DATASETS, Item::Value(Value::from(datasets)));
    if let Some(mut inserted) = root.key_mut(DATASETS) {
        inserted.leaf_decor_mut().set_prefix(prefix);
    }
}

fn clear_datasets(document: &mut DocumentMut) {
    let Some((removed, item)) = document.as_table_mut().remove_entry(DATASETS) else {
        return;
    };
    let comments = format!(
        "{}{}",
        raw(removed.leaf_decor().prefix()),
        item.as_value().map(trailing_comment).unwrap_or_default()
    );
    // Onto `[services]`, which follows it, or onto the end of the file.
    if let Some(table) = services_mut(document) {
        let prefix = format!("{comments}{}", raw(table.decor().prefix()));
        table.decor_mut().set_prefix(prefix);
    } else {
        prepend_trailing(document, &comments);
    }
}
