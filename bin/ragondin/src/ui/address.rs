//! Where `ragondin ui` listens: a loopback address, and nothing else.
//!
//! The server has no authentication: anyone who can open a connection to it
//! can read the workspace's runs and, once submission lands, launch jobs and
//! write files. So it listens on loopback only, and a request to bind anywhere
//! else is refused until an authentication layer exists (ADR-C36 § 1, as the
//! product owner decided on 2026-09-30). A machine elsewhere is reached
//! through an SSH tunnel, which keeps the loopback and brings its own
//! authentication; the refusal says how.

use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr};

use anyhow::{bail, Result};

/// The port `ragondin ui` listens on when `--port` is not given. Arbitrary on
/// purpose: not a round number, and none of the ports development servers
/// default to (3000, 5173, 8000, 8080), so it is unlikely to be taken on a
/// machine that also runs the UI's own dev server. `--port` overrides it.
pub const DEFAULT_PORT: u16 = 7341;

/// The two addresses `--bind` accepts, spelled as `--bind` spells them.
const LOOPBACK: [(&str, IpAddr); 2] = [
    ("127.0.0.1", IpAddr::V4(Ipv4Addr::LOCALHOST)),
    ("::1", IpAddr::V6(Ipv6Addr::LOCALHOST)),
];

/// The socket address `--bind` and `--port` name: `127.0.0.1` or `::1`,
/// `127.0.0.1` when `--bind` is absent, and [`DEFAULT_PORT`] when `--port`
/// is.
///
/// Decided on the argument's text, before anything opens a socket. Exactly
/// the two spellings are accepted: another address of `127.0.0.0/8` is
/// loopback too, but the served authority the `Host` check compares is the
/// one printed, and two spellings are all a user needs; `localhost` is a name,
/// not an address, and resolving it is the one thing this refuses to guess.
pub fn listen_address(bind: Option<&str>, port: Option<u16>) -> Result<SocketAddr> {
    let port = port.unwrap_or(DEFAULT_PORT);
    let bind = bind.unwrap_or(LOOPBACK[0].0);
    // Under `--port 0` no port is known yet, and `ssh -L 0:…` forwards none.
    let shown = match port {
        0 => "<port>".to_owned(),
        port => port.to_string(),
    };
    match LOOPBACK.iter().find(|(spelling, _)| *spelling == bind) {
        Some((_, ip)) => Ok(SocketAddr::new(*ip, port)),
        None => bail!(
            "`--bind {bind}` is refused: `ragondin ui` listens on loopback only (`127.0.0.1` \
             or `::1`), because it has no authentication yet, and anyone who could reach \
             another address could read the workspace and launch runs. To use it from another \
             machine, run it there on loopback and forward the port over SSH: \
             `ssh -L {shown}:127.0.0.1:{shown} <host>`, then open http://127.0.0.1:{shown}/ \
             here"
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn refusal(bind: &str, port: Option<u16>) -> String {
        format!(
            "{:#}",
            listen_address(Some(bind), port).expect_err("a non-loopback bind is refused")
        )
    }

    #[test]
    fn with_no_argument_it_listens_on_ipv4_loopback_at_the_default_port() {
        assert_eq!(
            listen_address(None, None).expect("the default"),
            SocketAddr::from(([127, 0, 0, 1], DEFAULT_PORT))
        );
    }

    #[test]
    fn both_loopback_forms_are_accepted_with_the_port_given() {
        assert_eq!(
            listen_address(Some("127.0.0.1"), Some(0)).expect("IPv4 loopback"),
            "127.0.0.1:0".parse::<SocketAddr>().expect("an address")
        );
        assert_eq!(
            listen_address(Some("::1"), Some(8080)).expect("IPv6 loopback"),
            "[::1]:8080".parse::<SocketAddr>().expect("an address")
        );
    }

    #[test]
    fn every_other_address_is_refused_naming_the_reason_and_the_ssh_tunnel() {
        for bind in [
            "0.0.0.0",
            "::",
            "192.168.1.10",
            "10.0.0.1",
            "fe80::1",
            "127.0.0.2",
            "localhost",
            "[::1]",
            "",
        ] {
            let error = refusal(bind, None);

            assert!(error.contains(&format!("`--bind {bind}`")), "{error}");
            assert!(error.contains("no authentication"), "{error}");
            assert!(
                error.contains(&format!(
                    "ssh -L {DEFAULT_PORT}:127.0.0.1:{DEFAULT_PORT} <host>"
                )),
                "{error}"
            );
        }
    }

    #[test]
    fn under_port_zero_the_tunnel_names_a_placeholder_rather_than_port_zero() {
        let error = refusal("0.0.0.0", Some(0));

        assert!(
            error.contains("ssh -L <port>:127.0.0.1:<port> <host>"),
            "{error}"
        );
        assert!(error.contains("http://127.0.0.1:<port>/"), "{error}");
        assert!(!error.contains(":0"), "{error}");
    }

    #[test]
    fn the_tunnel_the_refusal_suggests_forwards_the_port_asked_for() {
        assert!(refusal("0.0.0.0", Some(9000)).contains("ssh -L 9000:127.0.0.1:9000 <host>"));
    }
}
