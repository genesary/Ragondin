//! The permissive wire schema.

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_absent_version_reads_as_the_version_that_predates_the_field() {
        let doc: RawPipeline = serde_json::from_str(r#"{"pipeline":{"nodes":[]}}"#).unwrap();
        assert_eq!(doc.version, SchemaVersion::default());
        assert_eq!(doc.version.get(), 1);
    }

    #[test]
    fn the_supported_version_is_accepted_when_stated() {
        let doc: RawPipeline =
            serde_json::from_str(r#"{"version":1,"pipeline":{"nodes":[]}}"#).unwrap();
        assert_eq!(doc.version.get(), 1);
    }

    #[test]
    fn an_unknown_version_is_a_typed_error() {
        let err = SchemaVersion::new(7).unwrap_err();
        assert_eq!(err.found(), 7);
        assert!(
            err.to_string().contains('7') && err.to_string().contains('1'),
            "the message must name both what was found and what is understood: {err}"
        );
    }

    #[test]
    fn an_unknown_version_stops_deserialization_rather_than_being_ignored() {
        let err = serde_json::from_str::<RawPipeline>(r#"{"version":7,"pipeline":{"nodes":[]}}"#)
            .expect_err("a version this build cannot read must not parse");
        assert!(
            err.to_string().contains('7'),
            "the parse error must carry the offending version: {err}"
        );
    }

    #[test]
    fn the_wire_form_of_a_param_is_not_the_internal_one() {
        // INV-9. This separation is not stylistic: `ParamValue` is externally
        // tagged, so it cannot read what a configuration actually contains.
        assert_eq!(
            serde_json::from_str::<RawParamValue>("50").unwrap(),
            RawParamValue::Int(50)
        );
        assert!(
            serde_json::from_str::<crate::ParamValue>("50").is_err(),
            "if ParamValue ever reads a bare scalar, this crate has two names for one \
             format and INV-9's separation has quietly collapsed"
        );
    }

    #[test]
    fn param_scalars_read_as_their_narrowest_kind() {
        let cases = [
            ("true", RawParamValue::Bool(true)),
            ("50", RawParamValue::Int(50)),
            ("-3", RawParamValue::Int(-3)),
            ("0.75", RawParamValue::Float(0.75)),
            (r#""cosine""#, RawParamValue::String("cosine".to_string())),
            (
                r#"[1,"a"]"#,
                RawParamValue::List(vec![
                    RawParamValue::Int(1),
                    RawParamValue::String("a".to_string()),
                ]),
            ),
        ];
        for (json, expected) in cases {
            assert_eq!(
                serde_json::from_str::<RawParamValue>(json).unwrap(),
                expected,
                "{json} read as the wrong kind"
            );
        }
    }

    #[test]
    fn a_node_may_omit_its_inputs_and_params() {
        let node: RawNode =
            serde_json::from_str(r#"{"id":"t","component":"query_transform","impl":"hyde"}"#)
                .unwrap();
        assert!(node.inputs.is_empty());
        assert!(node.params.is_empty());
        assert_eq!(node.implementation, "hyde");
    }

    #[test]
    fn an_unknown_node_field_is_tolerated() {
        // The permissive level tolerates what it does not know; rejecting is #9's
        // job. This is the opposite of the validated level, deliberately.
        let node: RawNode = serde_json::from_str(
            r#"{"id":"g","component":"retriever","impl":"bm25","next":"generate"}"#,
        )
        .expect("an unrecognised key must not stop the parse");
        assert_eq!(node.id, "g");
    }

    #[test]
    fn the_wire_form_round_trips() {
        let doc: RawPipeline = serde_json::from_str(
            r#"{"version":1,"pipeline":{"nodes":[{"id":"d","component":"retriever","impl":"bm25","inputs":["t"],"params":{"top_k":50,"alpha":0.5}}]}}"#,
        )
        .unwrap();
        let back: RawPipeline =
            serde_json::from_str(&serde_json::to_string(&doc).unwrap()).unwrap();
        assert_eq!(back, doc);
    }
}
