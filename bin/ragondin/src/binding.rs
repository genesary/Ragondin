//! `--remote <family>/<name>=<uri>`: binding an implementation name to the
//! address of the service that answers under it (ADR-C32 § 2).
//!
//! A node names a `Remote` component exactly as it names a `Local` one, by its
//! `impl:` (or, for a `dense` node's embedder, `embedder:`) value, and the
//! address never enters the configuration: it is given here, on the command
//! line, so the laptop run and the cluster run of one configuration are one
//! `run_id` (ADR-C32 § 1). Everything in this module reads text and needs no
//! backend, so every build runs the same checks on the same arguments; a build
//! without the `remote` feature then refuses whatever passes them, naming the
//! feature.

use std::fmt;

use anyhow::{bail, Result};
use ragondin_experiments::RunBinding;
use ragondin_pipeline::{LogicalNode, LogicalPipeline, ParamValue};

use crate::wiring;

/// A family `--remote` binds a name in.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Family {
    /// A retriever node.
    Retriever,
    /// A fusion node.
    Fusion,
    /// A reranker node.
    Reranker,
    /// A context builder node.
    ContextBuilder,
    /// A generator node.
    Generator,
    /// The embedder a `dense` node names with `embedder:`.
    Embedder,
}

impl Family {
    /// Every family, in the order a refusal lists them.
    pub const ALL: [Self; 6] = [
        Self::Retriever,
        Self::Fusion,
        Self::Reranker,
        Self::ContextBuilder,
        Self::Generator,
        Self::Embedder,
    ];

    /// The text `--remote` names the family by.
    pub fn name(self) -> &'static str {
        match self {
            Self::Retriever => "retriever",
            Self::Fusion => "fusion",
            Self::Reranker => "reranker",
            Self::ContextBuilder => "context_builder",
            Self::Generator => "generator",
            Self::Embedder => "embedder",
        }
    }
}

impl fmt::Display for Family {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.name())
    }
}

/// One `--remote` argument, parsed.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Binding {
    /// The family the name is bound in.
    pub family: Family,
    /// The implementation name a node uses.
    pub name: String,
    /// The service's address, as written.
    pub uri: String,
}

/// Every `--remote` argument of one `bench`, in the order given.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Bindings {
    entries: Vec<Binding>,
}

impl Bindings {
    /// Parses the `--remote` arguments, refusing each of ADR-C32 § 2's five
    /// parsing refusals as an error naming the argument: a missing `/` or `=`
    /// or an empty part, a family that is not one of [`Family::ALL`], a URI
    /// that is not `http://<host>[:<port>]`, a family and name bound twice,
    /// and a name this composition root gives a `Local` component of that
    /// family in any build. A build without the `remote` feature then refuses
    /// any argument that passed them, naming the feature.
    ///
    /// Called before the configuration is loaded: an argument is refused on its
    /// text alone. The sixth refusal, a binding no node uses, needs the
    /// pipeline, and is [`refuse_unused`](Self::refuse_unused).
    pub fn parse(arguments: &[String]) -> Result<Self> {
        let mut entries: Vec<Binding> = Vec::with_capacity(arguments.len());
        for argument in arguments {
            let binding = parse_one(argument)?;
            if entries
                .iter()
                .any(|seen| seen.family == binding.family && seen.name == binding.name)
            {
                bail!(
                    "`--remote {argument}`: `{}/{}` is bound twice, and a name answers at one \
                     address",
                    binding.family,
                    binding.name
                );
            }
            entries.push(binding);
        }

        #[cfg(not(feature = "remote"))]
        if let Some(argument) = arguments.first() {
            bail!(
                "`--remote {argument}`: this build cannot construct a `Remote` component; \
                 rebuild with the `remote` feature"
            );
        }

        Ok(Self { entries })
    }

    /// The bindings as the run records them: family, name and URI as
    /// written, in the order given, outside the run's identity.
    pub fn record(&self) -> Vec<RunBinding> {
        self.entries
            .iter()
            .map(|binding| RunBinding {
                family: binding.family.name().to_owned(),
                name: binding.name.clone(),
                uri: binding.uri.clone(),
            })
            .collect()
    }

    /// Whether `name` is bound in `family`.
    pub fn binds(&self, family: Family, name: &str) -> bool {
        self.get(family, name).is_some()
    }

    /// The binding of `name` in `family`, if there is one.
    pub fn get(&self, family: Family, name: &str) -> Option<&Binding> {
        self.entries
            .iter()
            .find(|binding| binding.family == family && binding.name == name)
    }

    /// Every binding, in the order given. Read only where a binding can be
    /// constructed.
    #[cfg(feature = "remote")]
    pub fn iter(&self) -> impl Iterator<Item = &Binding> {
        self.entries.iter()
    }

    /// Refuses a binding no node of `pipeline` uses (ADR-C32 § 2's sixth
    /// refusal): for a node family, no node of that family has that `impl:`;
    /// for `embedder`, no `dense` node has that `embedder:`. A binding nothing
    /// uses is a typo, and recording it on the run as provenance would be
    /// false.
    pub fn refuse_unused(&self, pipeline: &LogicalPipeline) -> Result<()> {
        for binding in &self.entries {
            if !pipeline
                .nodes()
                .iter()
                .any(|node| uses(node, binding.family, &binding.name))
            {
                bail!(
                    "`--remote {}/{}={}`: no node of the pipeline uses `{}/{}`",
                    binding.family,
                    binding.name,
                    binding.uri,
                    binding.family,
                    binding.name
                );
            }
        }
        Ok(())
    }
}

/// Whether `node` names `name` in `family`.
fn uses(node: &LogicalNode, family: Family, name: &str) -> bool {
    match (family, node) {
        (Family::Retriever, LogicalNode::Retriever(node)) => node.implementation == name,
        (Family::Fusion, LogicalNode::Fusion(node)) => node.implementation == name,
        (Family::Reranker, LogicalNode::Reranker(node)) => node.implementation == name,
        (Family::ContextBuilder, LogicalNode::ContextBuilder(node)) => node.implementation == name,
        (Family::Generator, LogicalNode::Generator(node)) => node.implementation == name,
        (Family::Embedder, LogicalNode::Retriever(node)) => {
            node.implementation == wiring::DENSE
                && matches!(
                    node.params.get("embedder"),
                    Some(ParamValue::String(embedder)) if embedder == name
                )
        }
        _ => false,
    }
}

/// One argument, through every refusal that reads it alone.
fn parse_one(argument: &str) -> Result<Binding> {
    let Some((family, rest)) = argument.split_once('/') else {
        bail!("`--remote {argument}` has no `/`: the form is `<family>/<name>=<uri>`");
    };
    let Some((name, uri)) = rest.split_once('=') else {
        bail!(
            "`--remote {argument}` has no `=` after its `/`: the form is `<family>/<name>=<uri>`"
        );
    };
    for (part, value) in [("family", family), ("name", name), ("URI", uri)] {
        if value.is_empty() {
            bail!("`--remote {argument}` has an empty {part}: the form is `<family>/<name>=<uri>`");
        }
    }

    let Some(family) = Family::ALL.into_iter().find(|known| known.name() == family) else {
        let known: Vec<String> = Family::ALL
            .iter()
            .map(|family| format!("`{family}`"))
            .collect();
        bail!(
            "`--remote {argument}`: `{family}` is not a family a name can be bound in; the \
             families are {}",
            known.join(", ")
        );
    };

    if let Err(why) = check_uri(uri) {
        bail!(
            "`--remote {argument}`: `{uri}` {why}; a binding's address is `http://<host>` or \
             `http://<host>:<port>`"
        );
    }

    if wiring::is_local(family, name) {
        bail!(
            "`--remote {argument}`: `{name}` is the name this composition root gives a `Local` \
             {family}, and a binding would silently replace it"
        );
    }

    Ok(Binding {
        family,
        name: name.to_owned(),
        uri: uri.to_owned(),
    })
}

/// Why `uri` is not `http://<host>` or `http://<host>:<port>`, if it is not.
///
/// The scheme is `http` alone: `https` is refused with the rest, since TLS is
/// not decided (ADR-C32 § 3). No path, not even `/`, no query, no fragment and
/// no user information: the address is a host and a port, and anything else
/// written there would be recorded on the run as if it meant something.
fn check_uri(uri: &str) -> Result<(), &'static str> {
    let Some(authority) = uri.strip_prefix("http://") else {
        return Err("is not an `http://` URI");
    };
    if authority.contains(['/', '?', '#']) {
        return Err("carries a path, a query or a fragment");
    }
    if authority.contains('@') {
        return Err("carries user information");
    }

    // A bracketed IPv6 literal holds colons of its own, so the port is what
    // follows its closing bracket rather than the first colon.
    let (host, port) = match authority.strip_prefix('[') {
        Some(bracketed) => match bracketed.split_once(']') {
            Some((literal, rest)) => (
                literal,
                match rest {
                    "" => None,
                    _ => Some(rest.strip_prefix(':').ok_or("is not a host and a port")?),
                },
            ),
            None => return Err("opens an IPv6 literal it does not close"),
        },
        None => match authority.split_once(':') {
            Some((host, port)) => (host, Some(port)),
            None => (authority, None),
        },
    };
    if host.is_empty() {
        return Err("names no host");
    }
    // Checked here, on the argument, rather than left to the channel: the
    // channel is built after the configuration is loaded, and would name the
    // binding rather than the argument. A bracketed host is an IPv6 address;
    // any other is a name or an IPv4 address, spelt in the characters DNS
    // allows. No percent-encoding: a host that needs it names nothing a
    // channel can reach.
    let bracketed = authority.starts_with('[');
    if bracketed {
        if host.parse::<std::net::Ipv6Addr>().is_err() {
            return Err("brackets something that is not an IPv6 address");
        }
    } else if !host
        .bytes()
        .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-' || byte == b'.')
    {
        return Err("has a host that is not a name or an address");
    }
    if let Some(port) = port {
        if port.is_empty() || !port.bytes().all(|byte| byte.is_ascii_digit()) {
            return Err("has a port that is not a number");
        }
        if port.parse::<u16>().is_err() {
            return Err("has a port out of range");
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use ragondin_pipeline::{validate, RawPipeline};

    use super::*;

    fn parse(arguments: &[&str]) -> Result<Bindings> {
        Bindings::parse(
            &arguments
                .iter()
                .map(|argument| (*argument).to_owned())
                .collect::<Vec<_>>(),
        )
    }

    /// The error's whole chain, as `bench` prints it.
    fn refusal(arguments: &[&str]) -> String {
        format!(
            "{:#}",
            parse(arguments).expect_err("the argument is refused")
        )
    }

    /// Every one of ADR-C32 § 2's parsing refusals names the argument.
    fn assert_names(error: &str, argument: &str) {
        assert!(
            error.contains(argument),
            "the refusal names `{argument}`: {error}"
        );
    }

    #[cfg(feature = "remote")]
    #[test]
    fn a_well_formed_binding_is_split_at_its_first_slash_and_the_first_equals_after_it() {
        let bindings = parse(&["embedder/bge=http://localhost:50051"]).expect("well formed");

        assert_eq!(
            bindings.iter().collect::<Vec<_>>(),
            [&Binding {
                family: Family::Embedder,
                name: "bge".to_owned(),
                uri: "http://localhost:50051".to_owned(),
            }]
        );
    }

    #[cfg(feature = "remote")]
    #[test]
    fn every_family_is_accepted_and_one_name_in_two_families_is_two_bindings() {
        for family in Family::ALL {
            parse(&[&format!("{family}/bge=http://host")]).expect("a family");
        }
        let bindings =
            parse(&["reranker/bge=http://a", "embedder/bge=http://b"]).expect("two bindings");

        assert_eq!(bindings.iter().count(), 2);
    }

    #[test]
    fn an_argument_missing_its_slash_or_its_equals_or_a_part_is_refused() {
        for argument in [
            "embedder",
            "embedder=http://host",
            "embedder/bge",
            "=http://host",
            "/bge=http://host",
            "embedder/=http://host",
            "embedder/bge=",
        ] {
            assert_names(&refusal(&[argument]), argument);
        }
    }

    #[test]
    fn a_family_that_is_not_one_of_the_six_is_refused_naming_those_that_are() {
        let error = refusal(&["store/qdrant=http://host"]);

        assert_names(&error, "store/qdrant=http://host");
        for family in Family::ALL {
            assert!(error.contains(&format!("`{family}`")), "{error}");
        }
    }

    #[test]
    fn a_uri_that_is_not_http_host_and_optional_port_is_refused() {
        for uri in [
            "https://host",
            "host:50051",
            "grpc://host",
            "http://",
            "http://host/",
            "http://host/v1",
            "http://host?x=1",
            "http://host#top",
            "http://host:",
            "http://host:port",
            "http://host:99999",
            "http://user@host",
            "http://[zz]:80",
            "http://a b",
            "http://a%zz",
            "http://[::1",
            "http://host.:",
        ] {
            let argument = format!("embedder/bge={uri}");
            let error = refusal(&[&argument]);
            assert_names(&error, &argument);
            // The URI's own refusal, in every build — not the lean build's
            // refusal of every binding, which would name the argument too.
            assert!(error.contains("`http://<host>:<port>`"), "{error}");
        }
    }

    #[cfg(feature = "remote")]
    #[test]
    fn a_uri_may_name_an_ip_address_and_a_port() {
        for uri in [
            "http://127.0.0.1:50051",
            "http://[::1]:50051",
            "http://vllm",
        ] {
            parse(&[&format!("generator/vllm={uri}")]).expect("an http host and port");
        }
    }

    #[test]
    fn the_same_family_and_name_bound_twice_is_refused_whatever_the_uris() {
        let error = refusal(&["embedder/bge=http://a", "embedder/bge=http://b"]);

        assert_names(&error, "embedder/bge");
    }

    #[test]
    fn a_name_this_composition_root_gives_a_local_component_is_refused_in_any_build() {
        // Whatever the features: `bm25` is refused by a build that cannot
        // construct BM25 too, so one command line means one thing everywhere.
        for argument in [
            "retriever/bm25=http://host",
            "retriever/dense=http://host",
            "fusion/rrf=http://host",
            "reranker/cross_encoder=http://host",
            "context_builder/concat=http://host",
            "generator/stub_generator=http://host",
            "embedder/onnx=http://host",
        ] {
            let error = refusal(&[argument]);
            assert_names(&error, argument);
            assert!(error.contains("`Local`"), "{error}");
        }
    }

    #[cfg(not(feature = "remote"))]
    #[test]
    fn a_build_without_the_remote_feature_refuses_a_well_formed_binding_naming_the_feature() {
        let error = refusal(&["generator/vllm=http://localhost:8000"]);

        assert_names(&error, "generator/vllm=http://localhost:8000");
        assert!(error.contains("`remote` feature"), "{error}");
    }

    #[cfg(not(feature = "remote"))]
    #[test]
    fn a_build_without_the_remote_feature_still_runs_the_parsing_checks_first() {
        let error = refusal(&["store/qdrant=http://host"]);

        assert!(!error.contains("`remote` feature"), "{error}");
    }

    fn pipeline(nodes: &str) -> LogicalPipeline {
        let yaml = format!("pipeline:\n  inputs: [question]\n  nodes:\n{nodes}");
        let raw: RawPipeline = serde_yaml::from_str(&yaml).expect("the fixture parses");
        validate(raw).expect("the fixture validates")
    }

    #[cfg(feature = "remote")]
    fn hybrid() -> LogicalPipeline {
        pipeline(
            "    - id: vectors\n      component: retriever\n      impl: dense\n      \
             inputs: [question]\n      params: { top_k: 10, embedder: bge, served_model: m }\n\
             \x20   - id: reranked\n      component: reranker\n      impl: bge-reranker\n      \
             inputs: [question, vectors]\n      params: { top_k: 5, served_model: r }\n",
        )
    }

    #[cfg(feature = "remote")]
    #[test]
    fn bindings_the_pipeline_uses_are_accepted() {
        parse(&["embedder/bge=http://a", "reranker/bge-reranker=http://b"])
            .expect("well formed")
            .refuse_unused(&hybrid())
            .expect("both are used");
    }

    #[cfg(feature = "remote")]
    #[test]
    fn a_binding_no_node_uses_is_refused_naming_it() {
        // `reranker/bge` names a family no node of which has that `impl:`, and
        // `embedder/bge-reranker` a name no `dense` node gives `embedder:`.
        for argument in ["reranker/bge=http://a", "embedder/bge-reranker=http://a"] {
            let error = parse(&[argument])
                .expect("well formed")
                .refuse_unused(&hybrid())
                .expect_err("a binding nothing uses is a typo");

            assert!(format!("{error:#}").contains(argument), "{error:#}");
        }
    }

    #[cfg(feature = "remote")]
    #[test]
    fn a_node_family_binding_is_used_only_by_a_node_of_that_family() {
        // `bge-reranker` is a reranker's name here; bound as a generator, it
        // is used by nothing.
        let error = parse(&["generator/bge-reranker=http://a"])
            .expect("well formed")
            .refuse_unused(&hybrid())
            .expect_err("no generator node is called `bge-reranker`");

        assert!(
            format!("{error:#}").contains("generator/bge-reranker"),
            "{error:#}"
        );
    }

    #[test]
    fn no_binding_is_no_refusal() {
        let bindings = parse(&[]).expect("nothing to parse");

        bindings
            .refuse_unused(&pipeline(
                "    - id: lexical\n      component: retriever\n      impl: bm25\n      \
                 inputs: [question]\n      params: { top_k: 10 }\n",
            ))
            .expect("nothing is bound");
    }
}
