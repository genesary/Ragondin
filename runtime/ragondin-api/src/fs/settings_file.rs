//! `workspace.toml`'s grammar: the subset of TOML the settings need, read and
//! written by hand.
//!
//! **Why by hand.** A TOML parser is not among the dependencies ADR-C36 § 6
//! admits, and that section makes any other entry a new decision. The
//! settings are two things — a datasets directory and `family/name → uri`
//! bindings — so the file needs one top-level key, one table and strings.
//! This module reads exactly that, and refuses everything else with the line
//! it is on, so a hand-edited file that leaves the subset is reported rather
//! than misread. What it reads is valid TOML, so any TOML reader agrees with
//! it on every file it accepts.
//!
//! ```toml
//! datasets = "/data/benchmarks"   # optional; relative to the workspace
//!
//! [services]
//! "generator/qwen" = "http://127.0.0.1:8080"
//! ```
//!
//! Read: blank lines and `#` comments, `datasets = <string>` before any
//! table, one `[services]` table of `"<family>/<name>" = <string>`, strings
//! basic (`"…"`, with TOML's escapes) or literal (`'…'`), and a trailing
//! comment after a value. Refused: any other key or table, a duplicate key
//! or table, a value that is not a one-line string, a service key without
//! its `/`, and anything left on a line after its value.

/// The settings as the file states them, before the datasets directory is
/// resolved against the workspace.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct SettingsFile {
    /// `datasets`, as written.
    pub(crate) datasets: Option<String>,
    /// `(family, name, uri)`, in the file's order.
    pub(crate) services: Vec<(String, String, String)>,
}

/// Why a file is outside the grammar, and on which line (from 1).
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct GrammarError {
    pub(crate) line: usize,
    pub(crate) reason: String,
}

/// The comment every file this module writes begins with.
const HEADER: &str = "\
# The workspace's deployment settings, read by `ragondin ui`. Never hashed
# into a run: an address here stays out of every pipeline and run identity.
#
# datasets = \"datasets\"                       # where benchmarks are read;
#                                             # <workspace>/datasets when absent
# [services]
# \"generator/qwen\" = \"http://127.0.0.1:8080\"  # `--remote` bindings
";

/// The file a fresh workspace starts with: the header, and nothing set.
pub(crate) fn empty() -> String {
    HEADER.to_owned()
}

/// Reads `text`.
pub(crate) fn parse(text: &str) -> Result<SettingsFile, GrammarError> {
    let mut file = SettingsFile::default();
    let mut in_services = false;
    let mut seen_services = false;
    for (index, raw) in text.lines().enumerate() {
        let line = index + 1;
        let refuse = |reason: String| GrammarError { line, reason };
        let content = raw.trim();
        if content.is_empty() || content.starts_with('#') {
            continue;
        }
        if let Some(header) = content.strip_prefix('[') {
            let (name, rest) = header
                .split_once(']')
                .ok_or_else(|| refuse("a table header that is not closed".to_owned()))?;
            if !is_blank_or_comment(rest) {
                return Err(refuse(format!("`{}` after a table header", rest.trim())));
            }
            if name.trim() != "services" {
                return Err(refuse(format!(
                    "the table `[{}]`: only `[services]` is read",
                    name.trim()
                )));
            }
            if seen_services {
                return Err(refuse("`[services]` is declared twice".to_owned()));
            }
            seen_services = true;
            in_services = true;
            continue;
        }
        let (key, rest) = parse_key(content).map_err(refuse)?;
        let rest = rest
            .trim_start()
            .strip_prefix('=')
            .ok_or_else(|| refuse(format!("the key `{key}` is not followed by `=`")))?;
        let (value, rest) = parse_string(rest.trim_start()).map_err(refuse)?;
        if !is_blank_or_comment(rest) {
            return Err(refuse(format!("`{}` after the value", rest.trim())));
        }
        if in_services {
            let Some((family, name)) = key.split_once('/') else {
                return Err(refuse(format!(
                    "the service key `{key}` is not `\"<family>/<name>\"`"
                )));
            };
            if family.is_empty() || name.is_empty() {
                return Err(refuse(format!(
                    "the service key `{key}` has an empty family or name"
                )));
            }
            if file
                .services
                .iter()
                .any(|(f, n, _)| f == family && n == name)
            {
                return Err(refuse(format!("`{key}` is bound twice")));
            }
            file.services
                .push((family.to_owned(), name.to_owned(), value));
        } else if key == "datasets" {
            if file.datasets.is_some() {
                return Err(refuse("`datasets` is set twice".to_owned()));
            }
            file.datasets = Some(value);
        } else {
            return Err(refuse(format!(
                "the key `{key}`: only `datasets` and the `[services]` table are read"
            )));
        }
    }
    Ok(file)
}

/// Writes `file`: the header, `datasets` when set, then `[services]` when
/// any is bound. What [`parse`] reads back is `file`.
pub(crate) fn render(file: &SettingsFile) -> String {
    let mut text = empty();
    if let Some(datasets) = &file.datasets {
        text.push_str(&format!("\ndatasets = {}\n", quote(datasets)));
    }
    if !file.services.is_empty() {
        text.push_str("\n[services]\n");
        for (family, name, uri) in &file.services {
            text.push_str(&format!(
                "{} = {}\n",
                quote(&format!("{family}/{name}")),
                quote(uri)
            ));
        }
    }
    text
}

fn is_blank_or_comment(rest: &str) -> bool {
    let rest = rest.trim();
    rest.is_empty() || rest.starts_with('#')
}

/// A bare key (`[A-Za-z0-9_-]+`) or a quoted one, and what follows it.
fn parse_key(text: &str) -> Result<(String, &str), String> {
    if text.starts_with('"') || text.starts_with('\'') {
        return parse_string(text);
    }
    let end = text
        .find(|c: char| !(c.is_ascii_alphanumeric() || c == '_' || c == '-'))
        .unwrap_or(text.len());
    if end == 0 {
        return Err(format!("`{text}` is not a key"));
    }
    Ok((text[..end].to_owned(), &text[end..]))
}

/// A one-line basic or literal string at the start of `text`, unescaped,
/// and what follows it.
fn parse_string(text: &str) -> Result<(String, &str), String> {
    if text.starts_with("\"\"\"") || text.starts_with("'''") {
        return Err("a multi-line string: only one-line strings are read".to_owned());
    }
    if let Some(body) = text.strip_prefix('\'') {
        let end = body
            .find('\'')
            .ok_or_else(|| "a literal string that is not closed".to_owned())?;
        return Ok((body[..end].to_owned(), &body[end + 1..]));
    }
    let Some(body) = text.strip_prefix('"') else {
        return Err(format!("`{text}` is not a string; every value here is one"));
    };
    let mut value = String::new();
    let mut chars = body.char_indices();
    while let Some((at, c)) = chars.next() {
        match c {
            '"' => return Ok((value, &body[at + 1..])),
            '\\' => {
                let (_, escape) = chars
                    .next()
                    .ok_or_else(|| "a string that ends in an escape".to_owned())?;
                match escape {
                    'b' => value.push('\u{8}'),
                    't' => value.push('\t'),
                    'n' => value.push('\n'),
                    'f' => value.push('\u{c}'),
                    'r' => value.push('\r'),
                    '"' => value.push('"'),
                    '\\' => value.push('\\'),
                    'u' | 'U' => {
                        let width = if escape == 'u' { 4 } else { 8 };
                        let digits: String = (0..width)
                            .filter_map(|_| chars.next().map(|(_, digit)| digit))
                            .collect();
                        let decoded = u32::from_str_radix(&digits, 16)
                            .ok()
                            .filter(|_| digits.len() == width)
                            .and_then(char::from_u32)
                            .ok_or_else(|| format!("`\\{escape}{digits}` is not a character"))?;
                        value.push(decoded);
                    }
                    other => return Err(format!("`\\{other}` is not an escape TOML knows")),
                }
            }
            c if c.is_control() && c != '\t' => {
                return Err("a control character inside a string".to_owned())
            }
            c => value.push(c),
        }
    }
    Err("a string that is not closed".to_owned())
}

/// `value` as a basic string, escaped so [`parse_string`] reads it back.
fn quote(value: &str) -> String {
    let mut quoted = String::from("\"");
    for c in value.chars() {
        match c {
            '"' => quoted.push_str("\\\""),
            '\\' => quoted.push_str("\\\\"),
            '\t' => quoted.push_str("\\t"),
            '\n' => quoted.push_str("\\n"),
            '\r' => quoted.push_str("\\r"),
            c if c.is_control() => quoted.push_str(&format!("\\u{:04X}", c as u32)),
            c => quoted.push(c),
        }
    }
    quoted.push('"');
    quoted
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_empty_file_is_read_as_nothing_set() {
        assert_eq!(parse(&empty()), Ok(SettingsFile::default()));
    }

    #[test]
    fn what_is_rendered_is_read_back() {
        let file = SettingsFile {
            datasets: Some("/data/\"a\" \\ \u{1}\n".to_owned()),
            services: vec![
                (
                    "generator".into(),
                    "qwen".into(),
                    "http://[::1]:8080".into(),
                ),
                ("embedder".into(), "bge".into(), "http://host".into()),
            ],
        };

        assert_eq!(parse(&render(&file)), Ok(file));
    }

    #[test]
    fn literal_strings_comments_and_unicode_escapes_are_read() {
        let file = parse(
            "datasets = 'C:\\data' # windows\n[ services ] # bindings\n'generator/a' = \"http://h\\u00e9\"\n",
        )
        .expect("in the grammar");

        assert_eq!(file.datasets.as_deref(), Some("C:\\data"));
        assert_eq!(
            file.services,
            [("generator".into(), "a".into(), "http://hé".into())]
        );
    }
}
