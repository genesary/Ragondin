//! `ragondin-generator-service`: the reference `Remote` generator service
//! (ADR-C33). See the library's documentation and `ARCHITECTURE.md`.

use std::process::ExitCode;

use ragondin_generator_service::cli::{self, API_KEY_VAR, USAGE};
use ragondin_generator_service::relay::Relay;
use ragondin_proto::v1::generator_server::GeneratorServer;
use tokio::net::TcpListener;
use tonic::transport::server::TcpIncoming;
use tonic::transport::Server;

#[tokio::main]
async fn main() -> ExitCode {
    let refused = |error: &dyn std::fmt::Display| {
        eprintln!("ragondin-generator-service: {error}\n{USAGE}");
        ExitCode::from(2)
    };
    let config = match cli::parse(std::env::args().skip(1)) {
        Ok(config) => config,
        Err(e) => return refused(&e),
    };
    let api_key = match cli::api_key(std::env::var(API_KEY_VAR)) {
        Ok(key) => key,
        Err(e) => return refused(&e),
    };
    match serve(config, api_key).await {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("ragondin-generator-service: {e}");
            ExitCode::FAILURE
        }
    }
}

async fn serve(config: cli::Config, api_key: Option<String>) -> Result<(), String> {
    let relay = Relay::new(config.base_url, api_key)
        .map_err(|e| format!("cannot build the HTTP client: {e}"))?;
    let listener = TcpListener::bind(config.listen)
        .await
        .map_err(|e| format!("cannot bind {}: {e}", config.listen))?;
    let bound = listener
        .local_addr()
        .map_err(|e| format!("cannot read the bound address: {e}"))?;
    // The one line this program writes to stdout: a test or the calibration
    // reads it to learn the port (ADR-C33 § 3).
    println!("listening on {bound}");
    let incoming = TcpIncoming::from_listener(listener, true, None)
        .map_err(|e| format!("cannot accept on {bound}: {e}"))?;
    Server::builder()
        .add_service(GeneratorServer::new(relay))
        .serve_with_incoming(incoming)
        .await
        .map_err(|e| format!("the gRPC server stopped: {e}"))
}
