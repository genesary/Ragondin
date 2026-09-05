//! # ragondin-config
//!
//! Configuration handling: a `ConfigSource` (either a `LocalFile` or a `Stream`
//! from the delivery service), the config schema, and the
//! parse → validate → compile path that turns a configuration into a
//! `LogicalPipeline`.
//!
//! The wire/config format is **separate** from the in-memory representation and
//! **versioned independently** (INV-9): never derive it from the internal IR
//! types, or the first refactor breaks every stored configuration.
//!
//! The schema and compilation land in a later issue; this is the compiling
//! skeleton.

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
