//! The prompt template grammar of ADR-C31 § 2, rendered on receipt.

use std::fmt;

/// Why a template is malformed. The service refuses the call as
/// `INVALID_ARGUMENT`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TemplateError {
    /// A name between braces other than `query` or `context`.
    UnknownPlaceholder {
        /// The byte offset of the opening brace.
        at: usize,
        /// The name found between the braces.
        name: String,
    },
    /// A `{` that no `}` closes.
    Unclosed {
        /// The byte offset of the opening brace.
        at: usize,
    },
    /// A `}` that is neither half of `}}` nor the end of a placeholder.
    LoneClose {
        /// The byte offset of the brace.
        at: usize,
    },
}

impl fmt::Display for TemplateError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::UnknownPlaceholder { at, name } => write!(
                f,
                "malformed template: unknown placeholder {{{name}}} at byte {at}; the placeholders are {{query}} and {{context}}"
            ),
            Self::Unclosed { at } => {
                write!(f, "malformed template: the {{ at byte {at} is never closed")
            }
            Self::LoneClose { at } => write!(
                f,
                "malformed template: lone }} at byte {at}; a literal }} is written }}}}"
            ),
        }
    }
}

impl std::error::Error for TemplateError {}

/// Renders `template` with `query` and `context`, under ADR-C31 § 2.
///
/// `{query}` and `{context}` are replaced by the two texts; `{{` renders a
/// literal `{` and `}}` a literal `}`; any other brace is malformed. One pass,
/// left to right, taking `{{` or `}}` before looking for a placeholder at each
/// position, so `{{query}}` renders as `{query}`. Substituted text is appended
/// and never scanned again.
pub fn render(template: &str, query: &str, context: &str) -> Result<String, TemplateError> {
    let mut out = String::with_capacity(template.len() + query.len() + context.len());
    let mut rest = template;
    // Byte offset of `rest` within `template`, for the error's position.
    let mut at = 0;
    while let Some(i) = rest.find(['{', '}']) {
        out.push_str(&rest[..i]);
        at += i;
        rest = &rest[i..];
        let consumed = if let Some(after) = rest.strip_prefix("{{") {
            out.push('{');
            rest.len() - after.len()
        } else if let Some(after) = rest.strip_prefix("}}") {
            out.push('}');
            rest.len() - after.len()
        } else if rest.starts_with('}') {
            return Err(TemplateError::LoneClose { at });
        } else {
            let close = rest.find('}').ok_or(TemplateError::Unclosed { at })?;
            match &rest[1..close] {
                "query" => out.push_str(query),
                "context" => out.push_str(context),
                name => {
                    return Err(TemplateError::UnknownPlaceholder {
                        at,
                        name: name.to_owned(),
                    })
                }
            }
            close + 1
        };
        at += consumed;
        rest = &rest[consumed..];
    }
    out.push_str(rest);
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ok(template: &str) -> String {
        render(template, "Q", "C").expect("well-formed template")
    }

    #[test]
    fn text_without_braces_renders_as_itself() {
        assert_eq!(ok("Answer briefly."), "Answer briefly.");
    }

    #[test]
    fn both_placeholders_are_replaced_any_number_of_times() {
        assert_eq!(ok("{context}\n\nQ: {query} ({query})"), "C\n\nQ: Q (Q)");
    }

    #[test]
    fn doubled_braces_render_as_literal_braces() {
        assert_eq!(ok("{{ json }}"), "{ json }");
    }

    #[test]
    fn a_doubled_brace_is_taken_before_a_placeholder() {
        assert_eq!(ok("{{query}}"), "{query}");
        assert_eq!(ok("{{{query}}}"), "{Q}");
    }

    #[test]
    fn substituted_text_is_never_scanned_again() {
        assert_eq!(
            render("{query}", "{context} }{", "C").unwrap(),
            "{context} }{"
        );
    }

    #[test]
    fn multibyte_text_is_kept_intact() {
        assert_eq!(ok("é{query}—{context}ü"), "éQ—Cü");
    }

    #[test]
    fn an_unknown_name_is_malformed() {
        assert_eq!(
            render("x{answer}", "Q", "C"),
            Err(TemplateError::UnknownPlaceholder {
                at: 1,
                name: "answer".into()
            })
        );
        assert!(matches!(
            render("{}", "Q", "C"),
            Err(TemplateError::UnknownPlaceholder { at: 0, .. })
        ));
    }

    #[test]
    fn an_unclosed_brace_is_malformed() {
        assert_eq!(
            render("ab{query", "Q", "C"),
            Err(TemplateError::Unclosed { at: 2 })
        );
    }

    #[test]
    fn a_lone_closing_brace_is_malformed() {
        assert_eq!(
            render("a}b", "Q", "C"),
            Err(TemplateError::LoneClose { at: 1 })
        );
        assert_eq!(
            render("{query}}", "Q", "C"),
            Err(TemplateError::LoneClose { at: 7 })
        );
    }
}
