//! The command line and the environment (ADR-C33 § 3).
//!
//! Two required flags, read from `std::env::args` without a parser crate, and
//! one optional environment variable. None of them is an experiment variable.

use std::env::VarError;
use std::net::SocketAddr;

use reqwest::Url;

/// The environment variable holding the inference server's API key.
pub const API_KEY_VAR: &str = "RAGONDIN_INFERENCE_API_KEY";

/// How the binary is invoked, printed after a refusal.
pub const USAGE: &str = "usage: ragondin-generator-service --base-url <http(s) URL, without /v1> --listen <socket address>\n\
                         environment: RAGONDIN_INFERENCE_API_KEY, optional, sent as a bearer token";

/// The service's whole configuration.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Config {
    /// The inference server's root, with one trailing `/` removed. The
    /// endpoints are this followed by `/v1/…`.
    pub base_url: String,
    /// The address the gRPC server binds.
    pub listen: SocketAddr,
}

/// Why a command line or the environment is refused at startup.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum CliError {
    /// A required flag is absent.
    #[error("{0} is required")]
    Missing(&'static str),
    /// A flag is the last argument, with no value after it.
    #[error("{0} needs a value")]
    NoValue(&'static str),
    /// A flag given twice.
    #[error("{0} is given more than once")]
    Repeated(&'static str),
    /// An argument that is not one of the two flags.
    #[error("unexpected argument {0:?}")]
    Unexpected(String),
    /// The base URL is not one the service can relay to.
    #[error("--base-url {value:?}: {reason}")]
    BaseUrl {
        /// The value given.
        value: String,
        /// What is wrong with it.
        reason: String,
    },
    /// The base URL carries a user or a password. The value is never shown.
    #[error(
        "--base-url carries a user or a password, which is refused and not shown; the key goes in {API_KEY_VAR}"
    )]
    BaseUrlCredentials,
    /// The listen address is not a socket address.
    #[error("--listen {value:?}: {reason}")]
    Listen {
        /// The value given.
        value: String,
        /// What is wrong with it.
        reason: String,
    },
    /// The API key is refused. The value is never shown.
    #[error("{API_KEY_VAR}: {0}")]
    ApiKey(&'static str),
}

const BASE_URL: &str = "--base-url";
const LISTEN: &str = "--listen";

/// Parses the arguments that follow the program name.
pub fn parse(args: impl IntoIterator<Item = String>) -> Result<Config, CliError> {
    let mut base_url = None;
    let mut listen = None;
    let mut args = args.into_iter();
    while let Some(arg) = args.next() {
        let (flag, slot) = match arg.as_str() {
            BASE_URL => (BASE_URL, &mut base_url),
            LISTEN => (LISTEN, &mut listen),
            _ => return Err(CliError::Unexpected(arg)),
        };
        let value = args.next().ok_or(CliError::NoValue(flag))?;
        if slot.replace(value).is_some() {
            return Err(CliError::Repeated(flag));
        }
    }
    let base_url = base_url.ok_or(CliError::Missing(BASE_URL))?;
    let listen = listen.ok_or(CliError::Missing(LISTEN))?;
    Ok(Config {
        base_url: check_base_url(base_url)?,
        listen: listen
            .parse()
            .map_err(|e: std::net::AddrParseError| CliError::Listen {
                reason: e.to_string(),
                value: listen,
            })?,
    })
}

fn check_base_url(value: String) -> Result<String, CliError> {
    let refuse = |reason: &str| CliError::BaseUrl {
        value: value.clone(),
        reason: reason.to_owned(),
    };
    let url = Url::parse(&value).map_err(|e| refuse(&format!("not an absolute URL ({e})")))?;
    // Checked first, and the value never shown: it holds a credential.
    if !url.username().is_empty() || url.password().is_some() {
        return Err(CliError::BaseUrlCredentials);
    }
    if !matches!(url.scheme(), "http" | "https") {
        return Err(refuse("the scheme must be http or https"));
    }
    if url.query().is_some() {
        return Err(refuse("a base URL carries no query"));
    }
    if url.fragment().is_some() {
        return Err(refuse("a base URL carries no fragment"));
    }
    let path = url.path();
    if path.strip_suffix('/').unwrap_or(path).ends_with("/v1") {
        return Err(refuse(
            "give the server's root without the /v1 segment, which the service adds",
        ));
    }
    Ok(value.strip_suffix('/').unwrap_or(&value).to_owned())
}

/// Reads the API key from the value of [`API_KEY_VAR`], as
/// `std::env::var` returns it. Unset and empty are the same: no key.
pub fn api_key(var: Result<String, VarError>) -> Result<Option<String>, CliError> {
    match var {
        Err(VarError::NotPresent) => Ok(None),
        Err(VarError::NotUnicode(_)) => Err(CliError::ApiKey("not valid UTF-8")),
        Ok(key) if key.is_empty() => Ok(None),
        // Printable ASCII other than `"` and `\` is exactly what neither a
        // JSON string nor a `{:?}`-quoted one escapes, so the key is found,
        // and redacted, verbatim in any error text that quotes it back. It is
        // also always a valid header value.
        Ok(key)
            if !key
                .bytes()
                .all(|b| matches!(b, b' '..=b'~') && b != b'"' && b != b'\\') =>
        {
            Err(CliError::ApiKey(
                "only printable ASCII other than \" and \\ is accepted",
            ))
        }
        Ok(key) => Ok(Some(key)),
    }
}
