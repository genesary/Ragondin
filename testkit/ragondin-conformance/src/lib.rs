//! # ragondin-conformance
//!
//! The behavioural suite **every** component implementation must pass, whatever
//! its nature. It is what makes `Local`/`Remote` equivalence *real* rather than
//! asserted, and it is what operationally enforces "no privilege for built-ins"
//! (INV-7): a built-in and a third-party component plug into exactly the same
//! suite and earn exactly the same guarantee. Without it, INV-7 is a slogan.
//!
//! The suite itself lands in a later issue; this is the compiling skeleton.

#[cfg(test)]
mod tests {
    #[test]
    fn skeleton_links() {
        let name = env!("CARGO_PKG_NAME");
        assert!(
            name.starts_with("ragondin-"),
            "unexpected crate name: {name}"
        );
    }
}
