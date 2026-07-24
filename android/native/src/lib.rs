use std::{
    cell::Cell,
    panic::catch_unwind,
    sync::{Arc, OnceLock},
    time::Instant,
};

use ark_ff::UniformRand;
use jni::{
    errors::ThrowRuntimeExAndDefault,
    objects::{JClass, JString},
    EnvUnowned,
};
use ledger::{
    scan_state::{
        currency::{Amount, Fee, Nonce, Sgn, Signed},
        transaction_logic::{
            zkapp_command::{
                Account, AccountPreconditions, AccountUpdate, Actions, AuthorizationKind, Body,
                CallForest, Control, Events, FeePayer, FeePayerBody, MayUseToken, Numeric,
                OrIgnore, Preconditions, Tree, Update, WithStackHash, ZkAppCommand,
                ZkAppPreconditions,
            },
            Memo,
        },
    },
    AccountId, MutableFp, TokenId,
};
use mina_curves::pasta::Fp;
use mina_p2p_messages::v2::{
    MinaBaseZkappCommandTStableV1WireStableV1, PicklesProofProofsVerifiedMaxStableV2,
};
use mina_runtime::{
    Backend, BackendRequest, BackendResponse, NetworkId, ProveCircuitRequest, ResourceId,
    SignZkappCommandRequest, SignedZkappCommandResponse, Versioned,
};
use mina_signer::{CompressedPubKey, Keypair, SecKey, Signature};
use serde::{Deserialize, Serialize};

pub mod graphql_json;
pub mod network;
pub mod witness;

static BACKEND: OnceLock<Backend> = OnceLock::new();
static COMPILED_TOKEN: OnceLock<Result<CompiledToken, String>> = OnceLock::new();

#[derive(Clone, Copy)]
struct CompiledToken {
    transfer_circuit_id: ResourceId,
    verification_key_hash: Fp,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct TransferRequest {
    sender_private_key: String,
    receiver: String,
    amount: String,
    token_address: String,
    graphql_url: String,
    fund_receiver: bool,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct BalanceRequest {
    address: String,
    token_address: String,
    graphql_url: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct NativeResponse {
    status: &'static str,
    message: String,
    transaction_hash: Option<String>,
    timings_ms: Timings,
}

impl NativeResponse {
    fn error(message: String, timings_ms: Timings) -> Self {
        Self {
            status: "error",
            message,
            transaction_hash: None,
            timings_ms,
        }
    }
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct BalanceResponse {
    status: &'static str,
    balance: Option<String>,
    message: String,
}

#[derive(Debug, Serialize)]
struct Timings {
    compile: Option<u128>,
    proving: Option<u128>,
    signature: Option<u128>,
    submit: Option<u128>,
    total: u128,
}

impl Timings {
    fn pending(started: Instant) -> Self {
        Self {
            compile: None,
            proving: None,
            signature: None,
            submit: None,
            total: started.elapsed().as_millis(),
        }
    }
}

fn backend() -> &'static Backend {
    BACKEND.get_or_init(Backend::default)
}

fn compiled_token() -> Result<CompiledToken, String> {
    COMPILED_TOKEN
        .get_or_init(|| {
            let expected_hash = witness::verification_key_hash()?;
            let transfer_branch = witness::transfer_branch()?;
            let response = backend()
                .compile_program(witness::compile_request()?)
                .map_err(|error| error.to_string())?;
            let branch = response
                .branches
                .get(transfer_branch)
                .ok_or_else(|| "the compiled FungibleToken transfer branch is missing".to_owned())?;
            let compiled_hash = branch
                .verification_key_hash
                .as_deref()
                .ok_or_else(|| "the native compiler did not return a verification key".to_owned())?
                .parse::<Fp>()
                .map_err(|_| "the native compiler returned an invalid verification key hash".to_owned())?;
            if compiled_hash != expected_hash {
                return Err(format!(
                    "native/o1js FungibleToken verification key mismatch: expected {expected_hash}, got {compiled_hash}"
                ));
            }
            Ok(CompiledToken {
                transfer_circuit_id: branch.circuit_id,
                verification_key_hash: compiled_hash,
            })
        })
        .clone()
}

pub fn transfer_call_data(
    sender: &CompressedPubKey,
    receiver: &CompressedPubKey,
    amount: u64,
    blinding: Fp,
) -> Fp {
    let packed_arguments = (Fp::from(sender.is_odd as u64) * Fp::from(2u64)
        + Fp::from(receiver.is_odd as u64))
        * Fp::from(1u128 << 64)
        + Fp::from(amount);
    let method_name = Fp::from(26_689_851_412_640_264_820u128);
    poseidon::hash::hash_fields(&[
        Fp::from(5u64),
        sender.x,
        receiver.x,
        packed_arguments,
        Fp::from(0u64),
        method_name,
        blinding,
    ])
}

pub fn derive_token_id_base58(token_address: CompressedPubKey) -> String {
    let token_id = AccountId::new(token_address, TokenId::default()).derive_token_id();
    let token_id: mina_p2p_messages::v2::TokenIdKeyHash = token_id.into();
    token_id.to_string()
}

fn empty_account_update_body(
    public_key: CompressedPubKey,
    token_id: TokenId,
    balance_change: Signed<Amount>,
) -> Body {
    Body {
        public_key,
        token_id,
        update: Update::noop(),
        balance_change,
        increment_nonce: false,
        events: Events::empty(),
        actions: Actions::empty(),
        call_data: Fp::from(0u64),
        preconditions: Preconditions {
            network: ZkAppPreconditions::accept(),
            account: AccountPreconditions(Account::accept()),
            valid_while: Numeric::Ignore,
        },
        use_full_commitment: false,
        implicit_account_creation_fee: false,
        may_use_token: MayUseToken::No,
        authorization_kind: AuthorizationKind::NoneGiven,
    }
}

fn forest_node(
    account_update: AccountUpdate,
    calls: CallForest<AccountUpdate>,
) -> WithStackHash<AccountUpdate> {
    WithStackHash {
        elt: Tree {
            account_update,
            account_update_digest: MutableFp::empty(),
            calls,
        },
        stack_hash: MutableFp::empty(),
    }
}

pub fn build_unsigned_transfer_command(
    sender: CompressedPubKey,
    receiver: CompressedPubKey,
    token_address: CompressedPubKey,
    amount: u64,
    fee: u64,
    nonce: u32,
    fund_receiver: bool,
    verification_key_hash: Fp,
    blinding: Fp,
) -> ZkAppCommand {
    let token_id = AccountId::new(token_address.clone(), TokenId::default()).derive_token_id();

    let mut sender_token_body = empty_account_update_body(
        sender.clone(),
        token_id.clone(),
        Signed {
            magnitude: Amount::from_u64(amount),
            sgn: Sgn::Neg,
        },
    );
    sender_token_body.use_full_commitment = true;
    sender_token_body.may_use_token = MayUseToken::ParentsOwnToken;
    sender_token_body.authorization_kind = AuthorizationKind::Signature;

    let mut receiver_token_body = empty_account_update_body(
        receiver,
        token_id,
        Signed {
            magnitude: Amount::from_u64(amount),
            sgn: Sgn::Pos,
        },
    );
    receiver_token_body.may_use_token = MayUseToken::ParentsOwnToken;

    let token_calls = CallForest(vec![
        forest_node(
            AccountUpdate {
                body: sender_token_body,
                authorization: Control::Signature(Signature::dummy()),
            },
            CallForest::new(),
        ),
        forest_node(
            AccountUpdate {
                body: receiver_token_body,
                authorization: Control::NoneGiven,
            },
            CallForest::new(),
        ),
    ]);

    let mut token_body = empty_account_update_body(
        token_address,
        TokenId::default(),
        Signed {
            magnitude: Amount::from_u64(0),
            sgn: Sgn::Pos,
        },
    );
    token_body.call_data = transfer_call_data(
        &sender,
        &token_calls.0[1].elt.account_update.body.public_key,
        amount,
        blinding,
    );
    token_body.preconditions.account.0.state[3] = OrIgnore::Check(Fp::from(0u64));
    token_body.authorization_kind = AuthorizationKind::Proof(verification_key_hash);

    let mut roots = Vec::with_capacity(if fund_receiver { 2 } else { 1 });
    if fund_receiver {
        let mut funding_body = empty_account_update_body(
            sender.clone(),
            TokenId::default(),
            Signed {
                magnitude: Amount::from_u64(1_000_000_000),
                sgn: Sgn::Neg,
            },
        );
        funding_body.use_full_commitment = true;
        funding_body.authorization_kind = AuthorizationKind::Signature;
        roots.push(forest_node(
            AccountUpdate {
                body: funding_body,
                authorization: Control::Signature(Signature::dummy()),
            },
            CallForest::new(),
        ));
    }
    roots.push(forest_node(
        AccountUpdate {
            body: token_body,
            authorization: Control::NoneGiven,
        },
        token_calls,
    ));
    let account_updates = CallForest(roots);
    account_updates.ensure_hashed();

    ZkAppCommand {
        fee_payer: FeePayer {
            body: FeePayerBody {
                public_key: sender,
                fee: Fee::from_u64(fee),
                valid_until: None,
                nonce: Nonce::from_u32(nonce),
            },
            authorization: Signature::dummy(),
        },
        account_updates,
        memo: Memo::empty(),
    }
}

pub fn attach_transaction_proof(
    command: &mut ZkAppCommand,
    transaction_proof: &str,
) -> Result<(), String> {
    let proof: PicklesProofProofsVerifiedMaxStableV2 =
        serde_json::from_value(serde_json::Value::String(transaction_proof.to_owned()))
            .map_err(|error| format!("the Mina transaction proof is invalid: {error}"))?;
    attach_decoded_proof(command, Arc::new(proof))
}

fn attach_decoded_proof(
    command: &mut ZkAppCommand,
    proof: Arc<PicklesProofProofsVerifiedMaxStableV2>,
) -> Result<(), String> {
    let attached = Cell::new(0usize);
    command.account_updates = command.account_updates.map_to(|account_update| {
        if matches!(
            account_update.body.authorization_kind,
            AuthorizationKind::Proof(_)
        ) {
            attached.set(attached.get() + 1);
            let mut account_update = account_update.clone();
            account_update.authorization = Control::Proof(Arc::clone(&proof));
            return account_update;
        }
        account_update.clone()
    });
    let attached = attached.get();
    if attached != 1 {
        return Err(format!(
            "the transfer command must contain exactly one proved account update, found {attached}"
        ));
    }
    command.account_updates.ensure_hashed();
    Ok(())
}

pub fn sign_transfer_command(
    command: &ZkAppCommand,
    private_key: &str,
) -> Result<SignedZkappCommandResponse, String> {
    let command: MinaBaseZkappCommandTStableV1WireStableV1 = command.into();
    let response = backend()
        .execute(Versioned::current(BackendRequest::SignZkappCommand(
            SignZkappCommandRequest {
                private_key: private_key.to_owned(),
                network: NetworkId::Testnet,
                command,
            },
        )))
        .map_err(|error| error.to_string())?;
    match response.payload {
        BackendResponse::ZkappCommandSigned(response) => Ok(response),
        _ => Err("the native backend returned an unexpected signing response".to_owned()),
    }
}

fn response_json(response: NativeResponse) -> String {
    serde_json::to_string_pretty(&response)
        .unwrap_or_else(|error| format!(r#"{{"status":"error","message":"{error}"}}"#))
}

fn validate_request(request: &TransferRequest) -> Result<(), String> {
    if !request.sender_private_key.starts_with("EK") {
        return Err("the sender private key must start with EK".to_owned());
    }
    if !request.receiver.starts_with("B62") {
        return Err("the receiver address must start with B62".to_owned());
    }
    if !request.token_address.starts_with("B62") {
        return Err("the token contract address must start with B62".to_owned());
    }
    let amount = request
        .amount
        .parse::<u64>()
        .map_err(|_| "the amount must be an integer in the token's smallest unit".to_owned())?;
    if amount == 0 {
        return Err("the amount must be greater than zero".to_owned());
    }
    if !request.graphql_url.starts_with("https://") {
        return Err("the GraphQL endpoint must use HTTPS".to_owned());
    }
    let _ = request.fund_receiver;
    Ok(())
}

fn transfer(request_json: &str) -> String {
    let started = Instant::now();
    let request: TransferRequest = match serde_json::from_str(request_json) {
        Ok(request) => request,
        Err(error) => {
            return response_json(NativeResponse::error(
                format!("invalid parameters: {error}"),
                Timings::pending(started),
            ));
        }
    };

    if let Err(message) = validate_request(&request) {
        return response_json(NativeResponse::error(message, Timings::pending(started)));
    }

    let preflight = (|| -> Result<_, String> {
        let secret = SecKey::from_base58(&request.sender_private_key)
            .map_err(|_| "the sender private key is invalid".to_owned())?;
        let keypair = Keypair::try_from(secret)
            .map_err(|_| "the sender private key is invalid".to_owned())?;
        let sender = keypair.public.into_compressed();
        let receiver = mina_signer::PubKey::from_address(&request.receiver)
            .map_err(|_| "the receiver address is invalid".to_owned())?
            .into_compressed();
        let token = mina_signer::PubKey::from_address(&request.token_address)
            .map_err(|_| "the token contract address is invalid".to_owned())?
            .into_compressed();
        let amount = request
            .amount
            .parse::<u64>()
            .map_err(|_| "the amount is invalid".to_owned())?;
        let token_id = derive_token_id_base58(token.clone());
        let snapshot = network::fetch_network_snapshot(
            &request.graphql_url,
            &sender.clone().into_address(),
            &request.token_address,
            &request.receiver,
            &token_id,
        )?;
        if snapshot.paused {
            return Err("the fungible token contract is paused".to_owned());
        }
        if !snapshot.receiver_exists && !request.fund_receiver {
            return Err(
                "the receiver token account does not exist; enable account creation funding"
                    .to_owned(),
            );
        }
        let fund_receiver = request.fund_receiver && !snapshot.receiver_exists;
        let blinding = Fp::rand(&mut rand::thread_rng());
        let command = build_unsigned_transfer_command(
            sender.clone(),
            receiver.clone(),
            token.clone(),
            amount,
            100_000_000,
            snapshot.fee_payer_nonce,
            fund_receiver,
            snapshot.verification_key_hash,
            blinding,
        );
        Ok((command, snapshot, sender, receiver, token, amount, blinding))
    })();
    let (mut command, snapshot, sender, receiver, token, amount, blinding) = match preflight {
        Ok(result) => result,
        Err(message) => {
            return response_json(NativeResponse::error(message, Timings::pending(started)));
        }
    };
    let compile_started = Instant::now();
    let compiled = match compiled_token() {
        Ok(compiled) => compiled,
        Err(message) => {
            return response_json(NativeResponse::error(
                message,
                Timings {
                    compile: Some(compile_started.elapsed().as_millis()),
                    ..Timings::pending(started)
                },
            ));
        }
    };
    let compile_ms = compile_started.elapsed().as_millis();
    if snapshot.verification_key_hash != compiled.verification_key_hash {
        return response_json(NativeResponse::error(
            format!(
                "the deployed token verification key {} does not match the embedded o1js 2.15 FungibleToken key {}",
                snapshot.verification_key_hash, compiled.verification_key_hash
            ),
            Timings {
                compile: Some(compile_ms),
                ..Timings::pending(started)
            },
        ));
    }

    let proving_started = Instant::now();
    let proof = (|| -> Result<String, String> {
        let token_update = command
            .account_updates
            .0
            .last()
            .ok_or_else(|| "the transfer command has no token update".to_owned())?;
        let account_update_hash = token_update
            .elt
            .account_update_digest
            .get()
            .ok_or_else(|| "the token account update hash is missing".to_owned())?;
        let calls_hash = token_update.elt.calls.hash();
        let witness = witness::generate_transfer_witness(witness::TransferWitnessInput {
            account_update_hash,
            calls_hash,
            token_x: token.x,
            token_is_odd: token.is_odd,
            sender_x: sender.x,
            sender_is_odd: sender.is_odd,
            receiver_x: receiver.x,
            receiver_is_odd: receiver.is_odd,
            amount,
            blinding,
            verification_key_hash: compiled.verification_key_hash,
        })?;
        let response = backend()
            .prove_circuit(ProveCircuitRequest {
                circuit_id: compiled.transfer_circuit_id,
                witness,
            })
            .map_err(|error| error.to_string())?;
        response
            .transaction_proof
            .ok_or_else(|| "the native prover did not return a Mina transaction proof".to_owned())
    })();
    let transaction_proof = match proof {
        Ok(proof) => proof,
        Err(message) => {
            return response_json(NativeResponse::error(
                message,
                Timings {
                    compile: Some(compile_ms),
                    proving: Some(proving_started.elapsed().as_millis()),
                    ..Timings::pending(started)
                },
            ));
        }
    };
    let proving_ms = proving_started.elapsed().as_millis();
    if let Err(message) = attach_transaction_proof(&mut command, &transaction_proof) {
        return response_json(NativeResponse::error(
            message,
            Timings {
                compile: Some(compile_ms),
                proving: Some(proving_ms),
                ..Timings::pending(started)
            },
        ));
    }

    let signing_started = Instant::now();
    let signed = sign_transfer_command(&command, &request.sender_private_key);
    let signature_ms = signing_started.elapsed().as_millis();
    let signed = match signed {
        Ok(signed) => signed,
        Err(message) => {
            return response_json(NativeResponse::error(
                message,
                Timings {
                    compile: Some(compile_ms),
                    proving: Some(proving_ms),
                    signature: Some(signature_ms),
                    ..Timings::pending(started)
                },
            ));
        }
    };

    let zkapp_command_json = match graphql_json::zkapp_command_json(&signed.command) {
        Ok(json) => json,
        Err(message) => {
            return response_json(NativeResponse::error(
                message,
                Timings {
                    compile: Some(compile_ms),
                    proving: Some(proving_ms),
                    signature: Some(signature_ms),
                    ..Timings::pending(started)
                },
            ));
        }
    };
    let submit_started = Instant::now();
    let submitted = network::submit_zkapp_command(&request.graphql_url, &zkapp_command_json);
    let submit_ms = submit_started.elapsed().as_millis();
    match submitted {
        Ok(submitted) => response_json(NativeResponse {
            status: "sent",
            message: format!(
                "Native FungibleToken.transfer was proved on-device and submitted at nonce {}. Transaction hash: {}",
                snapshot.fee_payer_nonce, submitted.hash,
            ),
            transaction_hash: Some(submitted.hash),
            timings_ms: Timings {
                compile: Some(compile_ms),
                proving: Some(proving_ms),
                signature: Some(signature_ms),
                submit: Some(submit_ms),
                total: started.elapsed().as_millis(),
            },
        }),
        Err(message) => response_json(NativeResponse::error(
            message,
            Timings {
                compile: Some(compile_ms),
                proving: Some(proving_ms),
                signature: Some(signature_ms),
                submit: Some(submit_ms),
                ..Timings::pending(started)
            },
        )),
    }
}

fn token_balance(request_json: &str) -> String {
    let result = (|| -> Result<String, String> {
        let request: BalanceRequest = serde_json::from_str(request_json)
            .map_err(|error| format!("invalid parameters: {error}"))?;
        if !request.graphql_url.starts_with("https://") {
            return Err("the GraphQL endpoint must use HTTPS".to_owned());
        }
        mina_signer::PubKey::from_address(&request.address)
            .map_err(|_| "the selected address is invalid".to_owned())?;
        let token = mina_signer::PubKey::from_address(&request.token_address)
            .map_err(|_| "the token contract address is invalid".to_owned())?
            .into_compressed();
        let token_id = derive_token_id_base58(token);
        network::fetch_token_balance(&request.graphql_url, &request.address, &token_id)
    })();
    let response = match result {
        Ok(balance) => BalanceResponse {
            status: "ok",
            message: "Token balance loaded".to_owned(),
            balance: Some(balance),
        },
        Err(message) => BalanceResponse {
            status: "error",
            message,
            balance: None,
        },
    };
    serde_json::to_string(&response).unwrap_or_else(|error| {
        format!(r#"{{"status":"error","message":"{error}","balance":null}}"#)
    })
}

#[no_mangle]
pub extern "system" fn Java_com_lumina_minatokennative_MainActivity_nativeBackendInfo<'local>(
    mut unowned_env: EnvUnowned<'local>,
    _class: JClass<'local>,
) -> JString<'local> {
    unowned_env
        .with_env(|env| -> jni::errors::Result<_> {
            let value = catch_unwind(|| {
                serde_json::to_string_pretty(&backend().info())
                    .unwrap_or_else(|error| error.to_string())
            })
            .unwrap_or_else(|_| {
                "The native Rust backend panicked during initialization".to_owned()
            });
            JString::from_str(env, value)
        })
        .resolve::<ThrowRuntimeExAndDefault>()
}

#[no_mangle]
pub extern "system" fn Java_com_lumina_minatokennative_MainActivity_nativeTransfer<'local>(
    mut unowned_env: EnvUnowned<'local>,
    _class: JClass<'local>,
    request_json: JString<'local>,
) -> JString<'local> {
    unowned_env
        .with_env(|env| -> jni::errors::Result<_> {
            let request_chars = request_json.mutf8_chars(env)?;
            let request_json = request_chars.to_str().to_owned();
            let value = catch_unwind(|| transfer(&request_json))
                .unwrap_or_else(|_| "The native Rust backend panicked".to_owned());
            JString::from_str(env, value)
        })
        .resolve::<ThrowRuntimeExAndDefault>()
}

#[no_mangle]
pub extern "system" fn Java_com_lumina_minatokennative_MainActivity_nativeTokenBalance<'local>(
    mut unowned_env: EnvUnowned<'local>,
    _class: JClass<'local>,
    request_json: JString<'local>,
) -> JString<'local> {
    unowned_env
        .with_env(|env| -> jni::errors::Result<_> {
            let request_chars = request_json.mutf8_chars(env)?;
            let request_json = request_chars.to_str().to_owned();
            let value = catch_unwind(|| token_balance(&request_json))
                .unwrap_or_else(|_| "The native Rust backend panicked".to_owned());
            JString::from_str(env, value)
        })
        .resolve::<ThrowRuntimeExAndDefault>()
}

#[cfg(test)]
mod tests {
    use super::*;
    use mina_signer::{Keypair, SecKey};

    const PRIVATE_KEY: &str = "EKFPQBAbjYkjM6p6fEaZAzufQgQs3spvUw1Uyq2Ghta81cpKrfGg";

    #[test]
    fn rejects_invalid_addresses_without_echoing_the_private_key() {
        let private_key = "EK-secret-never-returned";
        let response = transfer(
            &serde_json::json!({
                "senderPrivateKey": private_key,
                "receiver": "invalid",
                "amount": "1",
                "tokenAddress": "B62token",
                "graphqlUrl": "https://example.test/graphql",
                "fundReceiver": true
            })
            .to_string(),
        );
        assert!(response.contains("receiver"));
        assert!(!response.contains(private_key));
    }

    #[test]
    fn rejects_an_invalid_balance_address_before_network_access() {
        let response = token_balance(
            &serde_json::json!({
                "address": "invalid",
                "tokenAddress": "B62token",
                "graphqlUrl": "https://example.test/graphql"
            })
            .to_string(),
        );
        assert!(response.contains("selected address is invalid"));
        assert!(response.contains("\"balance\":null"));
    }

    #[test]
    fn transfer_call_data_matches_o1js() {
        let sender = mina_signer::PubKey::from_address(
            "B62qiuynJSwKPepZGm8fcYbZ3zT2nynjcM23CD1Xzpofy5yKwMaC5N7",
        )
        .expect("valid sender")
        .into_compressed();
        let receiver = mina_signer::PubKey::from_address(
            "B62qjVQLxt9nYMWGn45mkgwYfcz8e8jvjNCBo11VKJb7vxDNwv5QLPS",
        )
        .expect("valid receiver")
        .into_compressed();
        let call_data = transfer_call_data(&sender, &receiver, 1_000_000_000, Fp::from(42u64));

        assert_eq!(
            call_data.to_string(),
            "14764797053846341985554514283674863099059682072860059805508960257679155238667"
        );
    }

    #[test]
    fn builds_the_four_o1js_transfer_account_updates() {
        let sender = mina_signer::PubKey::from_address(
            "B62qiuynJSwKPepZGm8fcYbZ3zT2nynjcM23CD1Xzpofy5yKwMaC5N7",
        )
        .expect("valid sender")
        .into_compressed();
        let receiver = mina_signer::PubKey::from_address(
            "B62qjVQLxt9nYMWGn45mkgwYfcz8e8jvjNCBo11VKJb7vxDNwv5QLPS",
        )
        .expect("valid receiver")
        .into_compressed();
        let token = mina_signer::PubKey::from_address(
            "B62qmnY6m4c6bdgSPnQGZriSaj9vuSjsfh6qkveGTsFX3yGA5ywRaja",
        )
        .expect("valid token")
        .into_compressed();
        let command = build_unsigned_transfer_command(
            sender.clone(),
            receiver.clone(),
            token.clone(),
            1_000_000_000,
            100_000_000,
            7,
            true,
            Fp::from(123u64),
            Fp::from(42u64),
        );

        assert_eq!(command.account_updates.0.len(), 2);
        let funding = &command.account_updates.0[0].elt.account_update;
        assert_eq!(funding.body.public_key, sender);
        assert_eq!(funding.body.token_id, TokenId::default());
        assert_eq!(funding.body.balance_change.sgn, Sgn::Neg);
        assert!(matches!(
            funding.body.authorization_kind,
            AuthorizationKind::Signature
        ));

        let token_update = &command.account_updates.0[1].elt;
        assert_eq!(token_update.account_update.body.public_key, token.clone());
        assert!(matches!(
            token_update.account_update.body.authorization_kind,
            AuthorizationKind::Proof(hash) if hash == Fp::from(123u64)
        ));
        assert_eq!(token_update.calls.0.len(), 2);
        assert_eq!(
            token_update.account_update.body.call_data.to_string(),
            "14764797053846341985554514283674863099059682072860059805508960257679155238667"
        );
        assert!(matches!(
            token_update.account_update.body.preconditions.account.0.state[3],
            OrIgnore::Check(value) if value == Fp::from(0u64)
        ));

        let derived_token_id = AccountId::new(token, TokenId::default()).derive_token_id();
        let debit = &token_update.calls.0[0].elt.account_update;
        let credit = &token_update.calls.0[1].elt.account_update;
        assert_eq!(debit.body.token_id, derived_token_id);
        assert_eq!(credit.body.token_id, derived_token_id);
        assert_eq!(debit.body.balance_change.sgn, Sgn::Neg);
        assert_eq!(credit.body.balance_change.sgn, Sgn::Pos);
        assert!(debit.body.use_full_commitment);
        assert!(!credit.body.use_full_commitment);
        assert!(matches!(
            debit.body.authorization_kind,
            AuthorizationKind::Signature
        ));
        assert!(matches!(
            credit.body.authorization_kind,
            AuthorizationKind::NoneGiven
        ));
        assert_eq!(command.fee_payer.body.nonce, Nonce::from_u32(7));
        assert_eq!(command.fee_payer.body.fee, Fee::from_u64(100_000_000));
    }

    #[test]
    fn derived_token_id_matches_o1js() {
        let token = mina_signer::PubKey::from_address(
            "B62qmnY6m4c6bdgSPnQGZriSaj9vuSjsfh6qkveGTsFX3yGA5ywRaja",
        )
        .expect("valid token")
        .into_compressed();

        assert_eq!(
            derive_token_id_base58(token),
            "yJmwcJGYKA5x5ReFVWnE3eCZnPJj45P92oJcTADvGXWpLNCMk9"
        );
    }

    #[test]
    fn transfer_command_hashes_match_the_o1js_witness() {
        let sender = mina_signer::PubKey::from_address(
            "B62qkj5CSRx9qWwYtHUWaYp5M3whGuhavCmZWBwsTAK9Du7xsq1NgUb",
        )
        .expect("valid sender")
        .into_compressed();
        let receiver = mina_signer::PubKey::from_address(
            "B62qpTLWDznvPzyrn4ZZDhZpXP1WwjSE4UBGvPfEsHSSYHhvGXnoqzn",
        )
        .expect("valid receiver")
        .into_compressed();
        let token = mina_signer::PubKey::from_address(
            "B62qqFUFipaDDuswoeyaYS5ox4Z2dUBBWvNjKNTuDjenjTGRakjbL12",
        )
        .expect("valid token")
        .into_compressed();
        let command = build_unsigned_transfer_command(
            sender,
            receiver,
            token,
            1_234_567_890,
            100_000_000,
            7,
            true,
            witness::verification_key_hash().expect("verification key"),
            "8526403581930790070278492913739709551282841087180981068687412600503419936070"
                .parse()
                .expect("blinding"),
        );
        // Both values are cross-verified against o1js `AccountUpdate.hash()` /
        // `toPublicInput()` on the identical command JSON (networkId testnet).
        let token_update = command.account_updates.0.last().expect("token update");
        assert_eq!(
            token_update
                .elt
                .account_update_digest
                .get()
                .expect("account update hash")
                .to_string(),
            "5942052871294504843338648535041687405744967511414765779794371228958282456639"
        );
        assert_eq!(
            token_update.elt.calls.hash().to_string(),
            "8251449290756981619132135748088612520442188699884439878052008043642591254857"
        );
    }

    #[test]
    fn attaches_the_proof_then_signs_every_sender_update() {
        let keypair = Keypair::try_from(SecKey::from_base58(PRIVATE_KEY).expect("valid secret"))
            .expect("valid keypair");
        let sender = keypair.public.into_compressed();
        let receiver = mina_signer::PubKey::from_address(
            "B62qjVQLxt9nYMWGn45mkgwYfcz8e8jvjNCBo11VKJb7vxDNwv5QLPS",
        )
        .expect("valid receiver")
        .into_compressed();
        let token = mina_signer::PubKey::from_address(
            "B62qmnY6m4c6bdgSPnQGZriSaj9vuSjsfh6qkveGTsFX3yGA5ywRaja",
        )
        .expect("valid token")
        .into_compressed();
        let mut command = build_unsigned_transfer_command(
            sender,
            receiver,
            token,
            1_000_000_000,
            100_000_000,
            7,
            true,
            Fp::from(123u64),
            Fp::from(42u64),
        );
        attach_decoded_proof(&mut command, ledger::dummy::sideloaded_proof())
            .expect("proof must attach");
        assert!(matches!(
            command.account_updates.0[1]
                .elt
                .account_update
                .authorization,
            Control::Proof(_)
        ));

        let signed = sign_transfer_command(&command, PRIVATE_KEY).expect("command must sign");
        assert_eq!(signed.signed_account_updates, 2);
        assert!(!signed.binprot_base64.is_empty());
    }

    #[test]
    fn serializes_the_transfer_command_to_the_o1js_json_shape() {
        let keypair = Keypair::try_from(SecKey::from_base58(PRIVATE_KEY).expect("valid secret"))
            .expect("valid keypair");
        let sender = keypair.public.into_compressed();
        let receiver = mina_signer::PubKey::from_address(
            "B62qjVQLxt9nYMWGn45mkgwYfcz8e8jvjNCBo11VKJb7vxDNwv5QLPS",
        )
        .expect("valid receiver")
        .into_compressed();
        let token = mina_signer::PubKey::from_address(
            "B62qmnY6m4c6bdgSPnQGZriSaj9vuSjsfh6qkveGTsFX3yGA5ywRaja",
        )
        .expect("valid token")
        .into_compressed();
        let mut command = build_unsigned_transfer_command(
            sender,
            receiver,
            token,
            1_000_000_000,
            100_000_000,
            7,
            true,
            Fp::from(123u64),
            Fp::from(42u64),
        );
        attach_decoded_proof(&mut command, ledger::dummy::sideloaded_proof())
            .expect("proof must attach");
        let wire: MinaBaseZkappCommandTStableV1WireStableV1 = (&command).into();
        let json = graphql_json::zkapp_command_json(&wire).expect("serializable command");

        assert_eq!(
            json["memo"],
            "E4YM2vTHhWEg66xpj52JErHUBU4pZ1yageL4TVDDpTTSsv8mK6YaH"
        );
        assert_eq!(json["feePayer"]["body"]["fee"], "100000000");
        assert_eq!(json["feePayer"]["body"]["nonce"], "7");
        assert_eq!(json["feePayer"]["body"]["validUntil"], serde_json::Value::Null);
        assert!(json["feePayer"]["authorization"].is_string());

        let updates = json["accountUpdates"].as_array().expect("flat updates");
        assert_eq!(updates.len(), 4);
        let depths = updates
            .iter()
            .map(|update| update["body"]["callDepth"].as_u64().expect("call depth"))
            .collect::<Vec<_>>();
        assert_eq!(depths, vec![0, 0, 1, 1]);

        let dummy_vk_hash =
            "3392518251768960475377392625298437850623664973002200885669375116181514017494";
        let funding = &updates[0]["body"];
        assert_eq!(
            funding["tokenId"],
            "wSHV2S4qX9jFsLjQo8r1BsMLH2ZRKsZx6EJd1sbozGPieEC4Jf"
        );
        assert_eq!(funding["balanceChange"]["magnitude"], "1000000000");
        assert_eq!(funding["balanceChange"]["sgn"], "Negative");
        assert_eq!(funding["useFullCommitment"], true);
        assert_eq!(funding["authorizationKind"]["isSigned"], true);
        assert_eq!(funding["authorizationKind"]["isProved"], false);
        assert_eq!(funding["authorizationKind"]["verificationKeyHash"], dummy_vk_hash);
        assert!(updates[0]["authorization"]["signature"].is_string());
        assert_eq!(updates[0]["authorization"]["proof"], serde_json::Value::Null);

        let token_update = &updates[1]["body"];
        assert_eq!(token_update["authorizationKind"]["isProved"], true);
        assert_eq!(token_update["authorizationKind"]["verificationKeyHash"], "123");
        assert!(updates[1]["authorization"]["proof"].is_string());
        assert_eq!(
            token_update["preconditions"]["account"]["state"],
            serde_json::json!([null, null, null, "0", null, null, null, null])
        );
        assert_eq!(
            token_update["update"],
            serde_json::json!({
                "appState": [null, null, null, null, null, null, null, null],
                "delegate": null,
                "verificationKey": null,
                "permissions": null,
                "zkappUri": null,
                "tokenSymbol": null,
                "timing": null,
                "votingFor": null
            })
        );
        assert_eq!(
            token_update["preconditions"]["network"],
            serde_json::json!({
                "snarkedLedgerHash": null,
                "blockchainLength": null,
                "minWindowDensity": null,
                "totalCurrency": null,
                "globalSlotSinceGenesis": null,
                "stakingEpochData": {
                    "ledger": { "hash": null, "totalCurrency": null },
                    "seed": null,
                    "startCheckpoint": null,
                    "lockCheckpoint": null,
                    "epochLength": null
                },
                "nextEpochData": {
                    "ledger": { "hash": null, "totalCurrency": null },
                    "seed": null,
                    "startCheckpoint": null,
                    "lockCheckpoint": null,
                    "epochLength": null
                }
            })
        );

        let debit = &updates[2]["body"];
        assert_eq!(debit["mayUseToken"]["parentsOwnToken"], true);
        assert_eq!(debit["balanceChange"]["sgn"], "Negative");
        assert!(updates[2]["authorization"]["signature"].is_string());

        let credit = &updates[3]["body"];
        assert_eq!(credit["balanceChange"]["sgn"], "Positive");
        assert_eq!(credit["authorizationKind"]["isSigned"], false);
        assert_eq!(credit["authorizationKind"]["isProved"], false);
        assert_eq!(updates[3]["authorization"]["proof"], serde_json::Value::Null);
        assert_eq!(updates[3]["authorization"]["signature"], serde_json::Value::Null);
    }

    #[test]
    fn renders_graphql_literals_with_bare_keys() {
        let literal = graphql_json::graphql_literal(&serde_json::json!({
            "memo": "with \"quotes\"",
            "accountUpdates": [{ "body": { "callDepth": 1, "incrementNonce": false } }],
            "validUntil": null
        }));

        assert!(literal.contains(r#"memo: "with \"quotes\"""#));
        assert!(literal.contains("callDepth: 1"));
        assert!(literal.contains("incrementNonce: false"));
        assert!(literal.contains("validUntil: null"));
        assert!(!literal.contains(r#""memo""#));
    }

    #[test]
    #[ignore = "runs a real Devnet transfer end-to-end; needs the MINA_* environment variables"]
    fn devnet_end_to_end_transfer() {
        let sender = std::env::var("MINA_PRIVATE_KEY").expect("MINA_PRIVATE_KEY");
        let receiver = std::env::var("MINA_RECEIVER_ADDRESS").expect("MINA_RECEIVER_ADDRESS");
        let token = std::env::var("MINA_TOKEN_ADDRESS").expect("MINA_TOKEN_ADDRESS");
        let graphql_url = std::env::var("MINA_GRAPHQL_URL")
            .unwrap_or_else(|_| "https://api.minascan.io/node/devnet/v1/graphql".to_owned());
        let amount =
            std::env::var("MINA_TRANSFER_AMOUNT").unwrap_or_else(|_| "1000000000".to_owned());
        let response = transfer(
            &serde_json::json!({
                "senderPrivateKey": sender,
                "receiver": receiver,
                "amount": amount,
                "tokenAddress": token,
                "graphqlUrl": graphql_url,
                "fundReceiver": true
            })
            .to_string(),
        );
        eprintln!("{response}");
        assert!(
            response.contains(r#""status": "sent""#),
            "the Devnet transfer was not submitted"
        );
    }

    #[test]
    #[ignore = "compiles all eleven FungibleToken methods and creates a native Pickles proof"]
    fn compiles_and_proves_the_native_transfer_circuit() {
        let field = |value: &str| value.parse::<Fp>().expect("field test vector");
        let compiled = compiled_token().expect("compiled FungibleToken program");
        assert_eq!(
            compiled.verification_key_hash.to_string(),
            "11275266297357989434659649579180929660472107786900344600948115953037388411671"
        );
        // The account update hash is the ledger digest of the transfer body
        // built with the embedded (real) verification key hash, matching what
        // transfer() feeds the prover at runtime.
        let witness = witness::generate_transfer_witness(witness::TransferWitnessInput {
            account_update_hash: field(
                "15537917634692260976475066310831007672684196481687388777173706295382163403812",
            ),
            calls_hash: field(
                "7652688051181415380811715514734574835653779492253782967014461790261901546813",
            ),
            token_x: field(
                "2919996120512407313014062828808255422013969845275374011406132452783934981066",
            ),
            token_is_odd: false,
            sender_x: field(
                "26128929354271999245285962662286734919718711533760999607904852494858390193731",
            ),
            sender_is_odd: false,
            receiver_x: field(
                "28755616151314178074317148383765615429855032205626421959078684078997276329907",
            ),
            receiver_is_odd: true,
            amount: 777,
            blinding: field(
                "8554297942514439850942263858623548274610807464913380485764394084910558078826",
            ),
            verification_key_hash: compiled.verification_key_hash,
        })
        .expect("transfer witness");
        let proof = backend()
            .prove_circuit(ProveCircuitRequest {
                circuit_id: compiled.transfer_circuit_id,
                witness,
            })
            .expect("native transfer proof");
        assert!(proof.transaction_proof.is_some());
    }
}

#[cfg(test)]
mod compile_bench {
    use super::*;
    use mina_runtime::{CompileCircuitRequest, CompileProgramRequest};

    fn first_branches(count: usize) -> CompileProgramRequest {
        let full = witness::compile_request().expect("compile request");
        CompileProgramRequest {
            branches: full
                .branches
                .into_iter()
                .take(count)
                .collect::<Vec<CompileCircuitRequest>>(),
            cache_bytes_base64: None,
            want_cache_bytes: false,
        }
    }

    /// Splits the FungibleToken compile cost into its one-off SRS warm-up,
    /// its per-branch cost, and the shared wrap. Proving only needs the
    /// transfer branch, but the program's verification key is derived from
    /// every branch, so all of them are compiled today.
    #[test]
    #[ignore = "benchmark: decomposes the FungibleToken compile time"]
    fn decompose_compile_time() {
        let branch_count = witness::compile_request().expect("request").branches.len();
        let time = |count: usize| {
            let started = Instant::now();
            backend()
                .compile_program(first_branches(count))
                .expect("compiled program");
            started.elapsed().as_millis()
        };
        let cold_one = time(1);
        let warm_one = time(1);
        let warm_two = time(2);
        let warm_all = time(branch_count);
        let per_branch = warm_two.saturating_sub(warm_one);

        eprintln!("branches                 : {branch_count}");
        eprintln!("1 branch (cold, with SRS): {cold_one} ms");
        eprintln!("1 branch (warm)          : {warm_one} ms");
        eprintln!("2 branches (warm)        : {warm_two} ms");
        eprintln!("{branch_count} branches (warm)       : {warm_all} ms");
        eprintln!("SRS/Lagrange warm-up     : ~{} ms", cold_one.saturating_sub(warm_one));
        eprintln!("per additional branch    : ~{per_branch} ms");
        eprintln!("shared wrap + fixed cost : ~{} ms", warm_one.saturating_sub(per_branch));
    }
}
