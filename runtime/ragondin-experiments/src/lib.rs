//! # ragondin-experiments
//!
//! The native run store, the run registry, and the API the UI consumes. A run
//! is identified by its content-addressed tuple, so re-running an unchanged
//! `(config, dataset, index, models, engine)` need not re-execute.
//!
//! The store and its export adapters land in a later issue; this is the
//! compiling skeleton.

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
