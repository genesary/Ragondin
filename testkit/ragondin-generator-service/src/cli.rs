//! The command line and the environment (ADR-C33 § 3).
//!
//! Two required flags, read from `std::env::args` without a parser crate, and
//! one optional environment variable. None of them is an experiment variable.

use std::env::VarError;
use std::fmt;
use std::net::SocketAddr;

use reqwest::header::HeaderValue;
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
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CliError {
    /// A required flag is absent.
    Missing(&'static str),
    /// A flag is the last argument, with no value after it.
    NoValue(&'static str),
    /// A flag given twice.
    Repeated(&'static str),
    /// An argument that is not one of the two flags.
    Unexpected(String),
    /// The base URL is not one the service can relay to.
    BaseUrl {
        /// The value given.
        value: String,
        /// What is wrong with it.
        reason: String,
    },
    /// The listen address is not a socket address.
    Listen {
        /// The value given.
        value: String,
        /// What is wrong with it.
        reason: String,
    },
    /// The API key cannot be sent in a header. The value is never shown.
    ApiKey(&'static str),
}

impl fmt::Display for CliError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Missing(flag) => write!(f, "{flag} is required"),
            Self::NoValue(flag) => write!(f, "{flag} needs a value"),
            Self::Repeated(flag) => write!(f, "{flag} is given more than once"),
            Self::Unexpected(arg) => write!(f, "unexpected argument {arg:?}"),
            Self::BaseUrl { value, reason } => write!(f, "--base-url {value:?}: {reason}"),
            Self::Listen { value, reason } => write!(f, "--listen {value:?}: {reason}"),
            Self::ApiKey(reason) => write!(f, "{API_KEY_VAR}: {reason}"),
        }
    }
}

impl std::error::Error for CliError {}

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
        Ok(key) => {
            HeaderValue::from_str(&format!("Bearer {key}"))
                .map_err(|_| CliError::ApiKey("not a valid HTTP header value"))?;
            Ok(Some(key))
        }
    }
}
