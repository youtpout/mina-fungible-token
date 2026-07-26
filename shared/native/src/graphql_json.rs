//! Serializes a signed zkApp command into the JSON shape accepted by the
//! Mina daemon `sendZkapp` GraphQL mutation, which is the same shape as
//! o1js `ZkappCommand.toJSON()`: camelCase keys, a flat `accountUpdates`
//! list ordered by pre-order traversal with an explicit `callDepth`, and
//! `null` for every ignored precondition or kept update field.

use mina_p2p_messages::v2::{
    MinaBaseAccountUpdateAccountPreconditionStableV1,
    MinaBaseAccountUpdateAuthorizationKindStableV1, MinaBaseAccountUpdateBodyEventsStableV1,
    MinaBaseAccountUpdateFeePayerStableV1, MinaBaseAccountUpdateMayUseTokenStableV1,
    MinaBaseAccountUpdatePreconditionsStableV1, MinaBaseAccountUpdateTStableV1,
    MinaBaseAccountUpdateUpdateStableV1, MinaBaseAccountUpdateUpdateStableV1AppStateA,
    MinaBaseAccountUpdateUpdateStableV1Delegate, MinaBaseAccountUpdateUpdateStableV1Permissions,
    MinaBaseAccountUpdateUpdateStableV1Timing, MinaBaseAccountUpdateUpdateStableV1TokenSymbol,
    MinaBaseAccountUpdateUpdateStableV1VerificationKey,
    MinaBaseAccountUpdateUpdateStableV1VotingFor, MinaBaseAccountUpdateUpdateStableV1ZkappUri,
    MinaBaseControlStableV2, MinaBaseZkappCommandTStableV1WireStableV1,
    MinaBaseZkappCommandTStableV1WireStableV1AccountUpdatesAA,
    MinaBaseZkappPreconditionAccountStableV2Balance,
    MinaBaseZkappPreconditionAccountStableV2Delegate,
    MinaBaseZkappPreconditionAccountStableV2ProvedState,
    MinaBaseZkappPreconditionAccountStableV2ReceiptChainHash,
    MinaBaseZkappPreconditionAccountStableV2StateA,
    MinaBaseZkappPreconditionProtocolStateEpochDataStableV1,
    MinaBaseZkappPreconditionProtocolStateEpochDataStableV1EpochLedger,
    MinaBaseZkappPreconditionProtocolStateEpochDataStableV1EpochSeed,
    MinaBaseZkappPreconditionProtocolStateEpochDataStableV1StartCheckpoint,
    MinaBaseZkappPreconditionProtocolStateStableV1,
    MinaBaseZkappPreconditionProtocolStateStableV1Amount,
    MinaBaseZkappPreconditionProtocolStateStableV1GlobalSlot,
    MinaBaseZkappPreconditionProtocolStateStableV1Length,
    MinaBaseZkappPreconditionProtocolStateStableV1SnarkedLedgerHash,
    MinaStateBlockchainStateValueStableV2SignedAmount,
};
use serde_json::{json, Value};

/// o1js fills `authorizationKind.verificationKeyHash` with this dummy hash
/// whenever the account update is not proof-authorized.
const DUMMY_VERIFICATION_KEY_HASH: &str = crate::witness::DUMMY_VERIFICATION_KEY_HASH;

pub fn zkapp_command_json(
    command: &MinaBaseZkappCommandTStableV1WireStableV1,
) -> Result<Value, String> {
    let mut account_updates = Vec::new();
    for root in command.account_updates.iter() {
        push_account_updates(&root.elt, 0, &mut account_updates)?;
    }
    Ok(json!({
        "feePayer": fee_payer_json(&command.fee_payer),
        "accountUpdates": account_updates,
        "memo": command.memo.to_base58check(),
    }))
}

fn push_account_updates(
    node: &MinaBaseZkappCommandTStableV1WireStableV1AccountUpdatesAA,
    call_depth: u32,
    out: &mut Vec<Value>,
) -> Result<(), String> {
    out.push(account_update_json(&node.account_update, call_depth)?);
    for call in node.calls.iter() {
        push_account_updates(&call.elt, call_depth + 1, out)?;
    }
    Ok(())
}

fn fee_payer_json(fee_payer: &MinaBaseAccountUpdateFeePayerStableV1) -> Value {
    json!({
        "body": {
            "publicKey": fee_payer.body.public_key.to_string(),
            "fee": fee_payer.body.fee.as_u64().to_string(),
            "validUntil": fee_payer.body.valid_until.as_ref().map(|v| v.as_u32().to_string()),
            "nonce": fee_payer.body.nonce.to_string(),
        },
        "authorization": fee_payer.authorization.to_string(),
    })
}

fn account_update_json(
    update: &MinaBaseAccountUpdateTStableV1,
    call_depth: u32,
) -> Result<Value, String> {
    let body = &update.body;
    Ok(json!({
        "body": {
            "publicKey": body.public_key.to_string(),
            "tokenId": body.token_id.to_string(),
            "update": update_json(&body.update)?,
            "balanceChange": balance_change_json(&body.balance_change),
            "incrementNonce": body.increment_nonce,
            "events": events_json(&body.events),
            "actions": events_json(&body.actions),
            "callData": body.call_data.to_decimal(),
            "callDepth": call_depth,
            "preconditions": preconditions_json(&body.preconditions),
            "useFullCommitment": body.use_full_commitment,
            "implicitAccountCreationFee": body.implicit_account_creation_fee,
            "mayUseToken": may_use_token_json(&body.may_use_token),
            "authorizationKind": authorization_kind_json(&body.authorization_kind),
        },
        "authorization": authorization_json(&update.authorization)?,
    }))
}

fn update_json(update: &MinaBaseAccountUpdateUpdateStableV1) -> Result<Value, String> {
    if !matches!(
        update.verification_key,
        MinaBaseAccountUpdateUpdateStableV1VerificationKey::Keep
    ) || !matches!(
        update.permissions,
        MinaBaseAccountUpdateUpdateStableV1Permissions::Keep
    ) || !matches!(update.timing, MinaBaseAccountUpdateUpdateStableV1Timing::Keep)
    {
        return Err(
            "serializing verification key, permission, or timing updates is not supported"
                .to_owned(),
        );
    }
    let app_state = update
        .app_state
        .0
        .iter()
        .map(|v| match v {
            MinaBaseAccountUpdateUpdateStableV1AppStateA::Set(value) => Some(value.to_decimal()),
            MinaBaseAccountUpdateUpdateStableV1AppStateA::Keep => None,
        })
        .collect::<Vec<_>>();
    let delegate = match &update.delegate {
        MinaBaseAccountUpdateUpdateStableV1Delegate::Set(v) => Some(v.to_string()),
        MinaBaseAccountUpdateUpdateStableV1Delegate::Keep => None,
    };
    let zkapp_uri = match &update.zkapp_uri {
        MinaBaseAccountUpdateUpdateStableV1ZkappUri::Set(v) => Some(v.to_string()),
        MinaBaseAccountUpdateUpdateStableV1ZkappUri::Keep => None,
    };
    let token_symbol = match &update.token_symbol {
        MinaBaseAccountUpdateUpdateStableV1TokenSymbol::Set(v) => Some(v.to_string()),
        MinaBaseAccountUpdateUpdateStableV1TokenSymbol::Keep => None,
    };
    let voting_for = match &update.voting_for {
        MinaBaseAccountUpdateUpdateStableV1VotingFor::Set(v) => Some(v.to_string()),
        MinaBaseAccountUpdateUpdateStableV1VotingFor::Keep => None,
    };
    Ok(json!({
        "appState": app_state,
        "delegate": delegate,
        "verificationKey": Value::Null,
        "permissions": Value::Null,
        "zkappUri": zkapp_uri,
        "tokenSymbol": token_symbol,
        "timing": Value::Null,
        "votingFor": voting_for,
    }))
}

fn balance_change_json(balance_change: &MinaStateBlockchainStateValueStableV2SignedAmount) -> Value {
    json!({
        "magnitude": balance_change.magnitude.as_u64().to_string(),
        "sgn": balance_change.sgn.to_string(),
    })
}

fn events_json(events: &MinaBaseAccountUpdateBodyEventsStableV1) -> Value {
    Value::Array(
        events
            .0
            .iter()
            .map(|event| {
                Value::Array(
                    event
                        .iter()
                        .map(|field| Value::String(field.to_decimal()))
                        .collect(),
                )
            })
            .collect(),
    )
}

fn preconditions_json(preconditions: &MinaBaseAccountUpdatePreconditionsStableV1) -> Value {
    let valid_while = match &preconditions.valid_while {
        MinaBaseZkappPreconditionProtocolStateStableV1GlobalSlot::Check(v) => Some(json!({
            "lower": v.lower.as_u32().to_string(),
            "upper": v.upper.as_u32().to_string(),
        })),
        MinaBaseZkappPreconditionProtocolStateStableV1GlobalSlot::Ignore => None,
    };
    json!({
        "network": network_precondition_json(&preconditions.network),
        "account": account_precondition_json(&preconditions.account),
        "validWhile": valid_while,
    })
}

fn network_precondition_json(network: &MinaBaseZkappPreconditionProtocolStateStableV1) -> Value {
    let snarked_ledger_hash = match &network.snarked_ledger_hash {
        MinaBaseZkappPreconditionProtocolStateStableV1SnarkedLedgerHash::Check(v) => {
            Some(v.to_string())
        }
        MinaBaseZkappPreconditionProtocolStateStableV1SnarkedLedgerHash::Ignore => None,
    };
    let global_slot_since_genesis = match &network.global_slot_since_genesis {
        MinaBaseZkappPreconditionProtocolStateStableV1GlobalSlot::Check(v) => Some(json!({
            "lower": v.lower.as_u32().to_string(),
            "upper": v.upper.as_u32().to_string(),
        })),
        MinaBaseZkappPreconditionProtocolStateStableV1GlobalSlot::Ignore => None,
    };
    json!({
        "snarkedLedgerHash": snarked_ledger_hash,
        "blockchainLength": length_bounds_json(&network.blockchain_length),
        "minWindowDensity": length_bounds_json(&network.min_window_density),
        "totalCurrency": amount_bounds_json(&network.total_currency),
        "globalSlotSinceGenesis": global_slot_since_genesis,
        "stakingEpochData": epoch_data_json(&network.staking_epoch_data),
        "nextEpochData": epoch_data_json(&network.next_epoch_data),
    })
}

fn epoch_data_json(epoch_data: &MinaBaseZkappPreconditionProtocolStateEpochDataStableV1) -> Value {
    let seed = match &epoch_data.seed {
        MinaBaseZkappPreconditionProtocolStateEpochDataStableV1EpochSeed::Check(v) => {
            Some(v.to_string())
        }
        MinaBaseZkappPreconditionProtocolStateEpochDataStableV1EpochSeed::Ignore => None,
    };
    let start_checkpoint = match &epoch_data.start_checkpoint {
        MinaBaseZkappPreconditionProtocolStateEpochDataStableV1StartCheckpoint::Check(v) => {
            Some(v.to_string())
        }
        MinaBaseZkappPreconditionProtocolStateEpochDataStableV1StartCheckpoint::Ignore => None,
    };
    let lock_checkpoint = match &epoch_data.lock_checkpoint {
        MinaBaseZkappPreconditionProtocolStateEpochDataStableV1StartCheckpoint::Check(v) => {
            Some(v.to_string())
        }
        MinaBaseZkappPreconditionProtocolStateEpochDataStableV1StartCheckpoint::Ignore => None,
    };
    json!({
        "ledger": epoch_ledger_json(&epoch_data.ledger),
        "seed": seed,
        "startCheckpoint": start_checkpoint,
        "lockCheckpoint": lock_checkpoint,
        "epochLength": length_bounds_json(&epoch_data.epoch_length),
    })
}

fn epoch_ledger_json(
    ledger: &MinaBaseZkappPreconditionProtocolStateEpochDataStableV1EpochLedger,
) -> Value {
    let hash = match &ledger.hash {
        MinaBaseZkappPreconditionProtocolStateStableV1SnarkedLedgerHash::Check(v) => {
            Some(v.to_string())
        }
        MinaBaseZkappPreconditionProtocolStateStableV1SnarkedLedgerHash::Ignore => None,
    };
    json!({
        "hash": hash,
        "totalCurrency": amount_bounds_json(&ledger.total_currency),
    })
}

fn length_bounds_json(length: &MinaBaseZkappPreconditionProtocolStateStableV1Length) -> Value {
    match length {
        MinaBaseZkappPreconditionProtocolStateStableV1Length::Check(v) => json!({
            "lower": v.lower.as_u32().to_string(),
            "upper": v.upper.as_u32().to_string(),
        }),
        MinaBaseZkappPreconditionProtocolStateStableV1Length::Ignore => Value::Null,
    }
}

fn amount_bounds_json(amount: &MinaBaseZkappPreconditionProtocolStateStableV1Amount) -> Value {
    match amount {
        MinaBaseZkappPreconditionProtocolStateStableV1Amount::Check(v) => json!({
            "lower": v.lower.as_u64().to_string(),
            "upper": v.upper.as_u64().to_string(),
        }),
        MinaBaseZkappPreconditionProtocolStateStableV1Amount::Ignore => Value::Null,
    }
}

fn account_precondition_json(
    account: &MinaBaseAccountUpdateAccountPreconditionStableV1,
) -> Value {
    let account = &account.0;
    let balance = match &account.balance {
        MinaBaseZkappPreconditionAccountStableV2Balance::Check(v) => Some(json!({
            "lower": v.lower.as_u64().to_string(),
            "upper": v.upper.as_u64().to_string(),
        })),
        MinaBaseZkappPreconditionAccountStableV2Balance::Ignore => None,
    };
    let nonce = match &account.nonce {
        MinaBaseZkappPreconditionProtocolStateStableV1Length::Check(v) => Some(json!({
            "lower": v.lower.as_u32().to_string(),
            "upper": v.upper.as_u32().to_string(),
        })),
        MinaBaseZkappPreconditionProtocolStateStableV1Length::Ignore => None,
    };
    let receipt_chain_hash = match &account.receipt_chain_hash {
        MinaBaseZkappPreconditionAccountStableV2ReceiptChainHash::Check(v) => Some(v.to_decimal()),
        MinaBaseZkappPreconditionAccountStableV2ReceiptChainHash::Ignore => None,
    };
    let delegate = match &account.delegate {
        MinaBaseZkappPreconditionAccountStableV2Delegate::Check(v) => Some(v.to_string()),
        MinaBaseZkappPreconditionAccountStableV2Delegate::Ignore => None,
    };
    let state = account
        .state
        .0
        .iter()
        .map(|v| match v {
            MinaBaseZkappPreconditionAccountStableV2StateA::Check(value) => {
                Some(value.to_decimal())
            }
            MinaBaseZkappPreconditionAccountStableV2StateA::Ignore => None,
        })
        .collect::<Vec<_>>();
    let action_state = match &account.action_state {
        MinaBaseZkappPreconditionAccountStableV2StateA::Check(v) => Some(v.to_decimal()),
        MinaBaseZkappPreconditionAccountStableV2StateA::Ignore => None,
    };
    let proved_state = match &account.proved_state {
        MinaBaseZkappPreconditionAccountStableV2ProvedState::Check(v) => Some(*v),
        MinaBaseZkappPreconditionAccountStableV2ProvedState::Ignore => None,
    };
    let is_new = match &account.is_new {
        MinaBaseZkappPreconditionAccountStableV2ProvedState::Check(v) => Some(*v),
        MinaBaseZkappPreconditionAccountStableV2ProvedState::Ignore => None,
    };
    json!({
        "balance": balance,
        "nonce": nonce,
        "receiptChainHash": receipt_chain_hash,
        "delegate": delegate,
        "state": state,
        "actionState": action_state,
        "provedState": proved_state,
        "isNew": is_new,
    })
}

fn may_use_token_json(may_use_token: &MinaBaseAccountUpdateMayUseTokenStableV1) -> Value {
    let (parents_own_token, inherit_from_parent) = match may_use_token {
        MinaBaseAccountUpdateMayUseTokenStableV1::ParentsOwnToken => (true, false),
        MinaBaseAccountUpdateMayUseTokenStableV1::InheritFromParent => (false, true),
        MinaBaseAccountUpdateMayUseTokenStableV1::No => (false, false),
    };
    json!({
        "parentsOwnToken": parents_own_token,
        "inheritFromParent": inherit_from_parent,
    })
}

fn authorization_kind_json(
    authorization_kind: &MinaBaseAccountUpdateAuthorizationKindStableV1,
) -> Value {
    let (is_signed, is_proved, verification_key_hash) = match authorization_kind {
        MinaBaseAccountUpdateAuthorizationKindStableV1::Signature => {
            (true, false, DUMMY_VERIFICATION_KEY_HASH.to_owned())
        }
        MinaBaseAccountUpdateAuthorizationKindStableV1::Proof(hash) => {
            (false, true, hash.to_decimal())
        }
        MinaBaseAccountUpdateAuthorizationKindStableV1::NoneGiven => {
            (false, false, DUMMY_VERIFICATION_KEY_HASH.to_owned())
        }
    };
    json!({
        "isSigned": is_signed,
        "isProved": is_proved,
        "verificationKeyHash": verification_key_hash,
    })
}

fn authorization_json(authorization: &MinaBaseControlStableV2) -> Result<Value, String> {
    match authorization {
        MinaBaseControlStableV2::Signature(signature) => Ok(json!({
            "proof": Value::Null,
            "signature": signature.to_string(),
        })),
        MinaBaseControlStableV2::Proof(proof) => {
            let proof = serde_json::to_value(proof.as_ref())
                .map_err(|error| format!("the transaction proof cannot be serialized: {error}"))?;
            if !proof.is_string() {
                return Err("the transaction proof did not serialize to base64".to_owned());
            }
            Ok(json!({ "proof": proof, "signature": Value::Null }))
        }
        MinaBaseControlStableV2::NoneGiven => Ok(json!({
            "proof": Value::Null,
            "signature": Value::Null,
        })),
    }
}

/// Renders a JSON value as a GraphQL input literal: object keys are bare
/// identifiers, everything else keeps its JSON syntax. This mirrors how
/// o1js embeds the zkApp command into the `sendZkapp` mutation.
pub fn graphql_literal(value: &Value) -> String {
    match value {
        Value::Null => "null".to_owned(),
        Value::Bool(v) => v.to_string(),
        Value::Number(v) => v.to_string(),
        Value::String(v) => serde_json::to_string(v).unwrap_or_else(|_| "null".to_owned()),
        Value::Array(items) => {
            let items = items.iter().map(graphql_literal).collect::<Vec<_>>();
            format!("[{}]", items.join(", "))
        }
        Value::Object(fields) => {
            let fields = fields
                .iter()
                .map(|(key, value)| format!("{key}: {}", graphql_literal(value)))
                .collect::<Vec<_>>();
            format!("{{{}}}", fields.join(", "))
        }
    }
}
