//! Desktop front end for the mobile prover: same crate, same embedded assets,
//! same witness solver, no Android and no Xcode.
//!
//! Two modes. Without arguments it runs the offline benchmark — compile the
//! eleven `FungibleToken` methods from the embedded cache, solve the transfer
//! witness, prove it — and prints what each stage cost. That needs no keys and
//! no network, so it is the quickest way to read a machine's proving budget.
//!
//! With `--transfer <file>` it performs the real thing through the very
//! function the Android app calls, and prints the response verbatim.
//!
//! ```sh
//! cargo run --release --bin mina                    # timings only
//! cargo run --release --bin mina -- --transfer request.json
//! ```
//!
//! The request file holds what the app's form collects:
//!
//! ```json
//! {
//!   "senderPrivateKey": "EKE...",
//!   "receiver": "B62q...",
//!   "amount": "1000000000",
//!   "tokenAddress": "B62q...",
//!   "graphqlUrl": "https://mina-devnet-graphql.aurowallet.com/graphql",
//!   "fundReceiver": true
//! }
//! ```

use std::{env, fs, process::ExitCode, time::Instant};

use mina_token_mobile::{bench, witness};

fn main() -> ExitCode {
    let mut arguments = env::args().skip(1);
    match arguments.next().as_deref() {
        None => benchmark(),
        Some("--transfer") => match arguments.next() {
            Some(path) => transfer(&path),
            None => {
                eprintln!("--transfer needs the path to a request JSON file");
                ExitCode::FAILURE
            }
        },
        Some("--help" | "-h") => {
            println!("mina                      time compile, witness and proving");
            println!("mina --transfer <file>    prove and submit the request in <file>");
            ExitCode::SUCCESS
        }
        Some(other) => {
            eprintln!("unknown argument {other}; try --help");
            ExitCode::FAILURE
        }
    }
}

/// Compile, solve, prove — the three stages a transfer pays locally, timed
/// separately. The app reports the last two together, so this is also how the
/// two numbers are told apart.
fn benchmark() -> ExitCode {
    println!("{}", bench::describe_build());

    let started = Instant::now();
    let compiled = match bench::compiled_program() {
        Ok(compiled) => compiled,
        Err(message) => {
            eprintln!("compile failed: {message}");
            return ExitCode::FAILURE;
        }
    };
    let compile_ms = started.elapsed().as_millis();

    let started = Instant::now();
    let witness = match witness::sample_transfer_witness(compiled.verification_key_hash) {
        Ok(witness) => witness,
        Err(message) => {
            eprintln!("witness solving failed: {message}");
            return ExitCode::FAILURE;
        }
    };
    let witness_ms = started.elapsed().as_millis();

    let started = Instant::now();
    let proof = match bench::prove(&compiled, witness) {
        Ok(proof) => proof,
        Err(message) => {
            eprintln!("proving failed: {message}");
            return ExitCode::FAILURE;
        }
    };
    let proving_ms = started.elapsed().as_millis();

    println!("compile : {compile_ms} ms");
    println!("witness : {witness_ms} ms");
    println!("proving : {proving_ms} ms");
    println!("total   : {} ms", compile_ms + witness_ms + proving_ms);
    println!(
        "proof   : {} bytes of transaction proof",
        proof.transaction_proof.map(|p| p.len()).unwrap_or(0)
    );
    ExitCode::SUCCESS
}

/// The real transfer, through the same entry point as the app.
fn transfer(path: &str) -> ExitCode {
    let request = match fs::read_to_string(path) {
        Ok(request) => request,
        Err(error) => {
            eprintln!("cannot read {path}: {error}");
            return ExitCode::FAILURE;
        }
    };
    println!("{}", bench::transfer(&request));
    ExitCode::SUCCESS
}
