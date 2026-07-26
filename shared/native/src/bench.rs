//! What a desktop front end needs from the mobile prover.
//!
//! The Android app reaches the prover through JNI and an iOS app through
//! [`crate::ffi`]; a command-line build needs neither, only the same three
//! steps in Rust. Nothing here is specific to a platform — it exists so the
//! `mina` binary does not have to reach into private items.

use mina_runtime::{ProofResponse, ProveCircuitRequest};

use crate::CompiledToken;

/// Backend versions and the revisions this binary was built from, one line.
pub fn describe_build() -> String {
    serde_json::to_string(&crate::backend().info())
        .unwrap_or_else(|error| format!("backend info unavailable: {error}"))
}

/// Compiles the eleven `FungibleToken` methods, restoring the embedded cache
/// and seeding the embedded SRS payloads. Cached per process, like the app.
pub fn compiled_program() -> Result<CompiledToken, String> {
    crate::compiled_token()
}

/// Proves the transfer branch over a solved witness.
pub fn prove(compiled: &CompiledToken, witness: Vec<String>) -> Result<ProofResponse, String> {
    crate::backend()
        .prove_circuit(ProveCircuitRequest {
            circuit_id: compiled.transfer_circuit_id,
            witness,
        })
        .map_err(|error| error.to_string())
}

/// The full transfer the app performs, including submission: same request and
/// response JSON, timings included.
pub fn transfer(request_json: &str) -> String {
    crate::transfer(request_json)
}
