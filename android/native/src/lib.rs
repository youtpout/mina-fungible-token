use std::{panic::catch_unwind, sync::OnceLock, time::Instant};

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
use mina_runtime::Backend;
use mina_signer::{CompressedPubKey, Signature};
use serde::{Deserialize, Serialize};

static BACKEND: OnceLock<Backend> = OnceLock::new();

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

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct NativeResponse {
    status: &'static str,
    message: String,
    timings_ms: Timings,
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
            account_update_digest: MutableFp::new(Fp::from(0u64)),
            calls,
        },
        stack_hash: MutableFp::new(Fp::from(0u64)),
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
            return response_json(NativeResponse {
                status: "error",
                message: format!("invalid parameters: {error}"),
                timings_ms: Timings::pending(started),
            });
        }
    };

    if let Err(message) = validate_request(&request) {
        return response_json(NativeResponse {
            status: "error",
            message,
            timings_ms: Timings::pending(started),
        });
    }

    response_json(NativeResponse {
        status: "notReady",
        message: "The parameters are valid. Native transfer construction and proving are not enabled yet; no transaction was submitted.".to_owned(),
        timings_ms: Timings::pending(started),
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

#[cfg(test)]
mod tests {
    use super::*;

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
}
