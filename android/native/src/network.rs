use std::{str::FromStr, time::Duration};

use mina_curves::pasta::Fp;
use serde::Deserialize;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct NetworkSnapshot {
    pub fee_payer_nonce: u32,
    pub verification_key_hash: Fp,
    pub paused: bool,
    pub receiver_exists: bool,
}

#[derive(Debug, Deserialize)]
struct GraphQlResponse<T> {
    data: Option<T>,
    #[serde(default)]
    errors: Vec<GraphQlError>,
}

#[derive(Debug, Deserialize)]
struct GraphQlError {
    message: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct SnapshotData {
    fee_payer: Option<FeePayerAccount>,
    token_contract: Option<TokenContractAccount>,
    receiver_token_account: Option<ReceiverAccount>,
}

#[derive(Debug, Deserialize)]
struct FeePayerAccount {
    nonce: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct TokenContractAccount {
    zkapp_state: Option<Vec<String>>,
    verification_key: Option<VerificationKey>,
}

#[derive(Debug, Deserialize)]
struct VerificationKey {
    hash: String,
}

#[derive(Debug, Deserialize)]
struct ReceiverAccount {
    #[serde(rename = "publicKey")]
    _public_key: String,
}

#[derive(Debug, Deserialize)]
struct BalanceData {
    account: Option<BalanceAccount>,
}

#[derive(Debug, Deserialize)]
struct BalanceAccount {
    balance: AccountBalance,
}

#[derive(Debug, Deserialize)]
struct AccountBalance {
    total: String,
}

fn snapshot_query(fee_payer: &str, token_address: &str, receiver: &str, token_id: &str) -> String {
    format!(
        r#"query {{
  feePayer: account(publicKey: "{fee_payer}") {{ nonce }}
  tokenContract: account(publicKey: "{token_address}") {{
    zkappState
    verificationKey {{ hash }}
  }}
  receiverTokenAccount: account(publicKey: "{receiver}", token: "{token_id}") {{ publicKey }}
}}"#
    )
}

fn parse_snapshot(response: GraphQlResponse<SnapshotData>) -> Result<NetworkSnapshot, String> {
    if !response.errors.is_empty() {
        let messages = response
            .errors
            .into_iter()
            .map(|error| error.message)
            .collect::<Vec<_>>()
            .join("; ");
        return Err(format!(
            "the Mina GraphQL endpoint returned an error: {messages}"
        ));
    }
    let data = response
        .data
        .ok_or_else(|| "the Mina GraphQL response did not contain data".to_owned())?;
    let fee_payer = data
        .fee_payer
        .ok_or_else(|| "the fee payer account does not exist".to_owned())?;
    let token = data
        .token_contract
        .ok_or_else(|| "the token contract account does not exist".to_owned())?;
    let nonce = fee_payer
        .nonce
        .parse::<u32>()
        .map_err(|_| "the fee payer nonce is invalid".to_owned())?;
    let state = token
        .zkapp_state
        .ok_or_else(|| "the token contract has no zkApp state".to_owned())?;
    let paused_value = state
        .get(3)
        .ok_or_else(|| "the token contract zkApp state is incomplete".to_owned())?;
    let verification_key_hash = token
        .verification_key
        .ok_or_else(|| "the token contract has no verification key".to_owned())?
        .hash;
    let verification_key_hash = Fp::from_str(&verification_key_hash)
        .map_err(|_| "the token verification key hash is invalid".to_owned())?;

    Ok(NetworkSnapshot {
        fee_payer_nonce: nonce,
        verification_key_hash,
        paused: paused_value != "0",
        receiver_exists: data.receiver_token_account.is_some(),
    })
}

pub fn fetch_network_snapshot(
    graphql_url: &str,
    fee_payer: &str,
    token_address: &str,
    receiver: &str,
    token_id: &str,
) -> Result<NetworkSnapshot, String> {
    let client = reqwest::blocking::Client::builder()
        .timeout(Duration::from_secs(30))
        .build()
        .map_err(|error| format!("could not initialize the HTTPS client: {error}"))?;
    let response = client
        .post(graphql_url)
        .json(&serde_json::json!({
            "query": snapshot_query(fee_payer, token_address, receiver, token_id)
        }))
        .send()
        .map_err(|error| format!("could not reach the Mina GraphQL endpoint: {error}"))?
        .error_for_status()
        .map_err(|error| format!("the Mina GraphQL endpoint rejected the request: {error}"))?
        .json::<GraphQlResponse<SnapshotData>>()
        .map_err(|error| format!("could not decode the Mina GraphQL response: {error}"))?;
    parse_snapshot(response)
}

pub fn fetch_token_balance(
    graphql_url: &str,
    public_key: &str,
    token_id: &str,
) -> Result<String, String> {
    let client = reqwest::blocking::Client::builder()
        .timeout(Duration::from_secs(30))
        .build()
        .map_err(|error| format!("could not initialize the HTTPS client: {error}"))?;
    let query = format!(
        r#"query {{
  account(publicKey: "{public_key}", token: "{token_id}") {{
    balance {{ total }}
  }}
}}"#
    );
    let response = client
        .post(graphql_url)
        .json(&serde_json::json!({ "query": query }))
        .send()
        .map_err(|error| format!("could not reach the Mina GraphQL endpoint: {error}"))?
        .error_for_status()
        .map_err(|error| format!("the Mina GraphQL endpoint rejected the request: {error}"))?
        .json::<GraphQlResponse<BalanceData>>()
        .map_err(|error| format!("could not decode the Mina GraphQL response: {error}"))?;
    if !response.errors.is_empty() {
        let messages = response
            .errors
            .into_iter()
            .map(|error| error.message)
            .collect::<Vec<_>>()
            .join("; ");
        return Err(format!(
            "the Mina GraphQL endpoint returned an error: {messages}"
        ));
    }
    Ok(response
        .data
        .and_then(|data| data.account)
        .map(|account| account.balance.total)
        .unwrap_or_else(|| "0".to_owned()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_the_transfer_network_snapshot() {
        let response = serde_json::from_value(serde_json::json!({
            "data": {
                "feePayer": { "nonce": "7" },
                "tokenContract": {
                    "zkappState": ["0", "1", "2", "0", "4", "5", "6", "7"],
                    "verificationKey": { "hash": "123" }
                },
                "receiverTokenAccount": null
            }
        }))
        .expect("valid fixture");

        let snapshot = parse_snapshot(response).expect("valid snapshot");
        assert_eq!(snapshot.fee_payer_nonce, 7);
        assert_eq!(snapshot.verification_key_hash, Fp::from(123u64));
        assert!(!snapshot.paused);
        assert!(!snapshot.receiver_exists);
    }

    #[test]
    fn surfaces_graphql_errors() {
        let response = serde_json::from_value(serde_json::json!({
            "errors": [{ "message": "account query failed" }]
        }))
        .expect("valid fixture");

        let error = parse_snapshot(response).expect_err("GraphQL error must fail");
        assert!(error.contains("account query failed"));
    }

    #[test]
    fn parses_existing_and_missing_token_balances() {
        let existing: GraphQlResponse<BalanceData> = serde_json::from_value(serde_json::json!({
            "data": { "account": { "balance": { "total": "123456789" } } }
        }))
        .expect("valid balance response");
        assert_eq!(
            existing
                .data
                .and_then(|data| data.account)
                .map(|account| account.balance.total)
                .unwrap_or_else(|| "0".to_owned()),
            "123456789"
        );

        let missing: GraphQlResponse<BalanceData> = serde_json::from_value(serde_json::json!({
            "data": { "account": null }
        }))
        .expect("valid missing-account response");
        assert_eq!(
            missing
                .data
                .and_then(|data| data.account)
                .map(|account| account.balance.total)
                .unwrap_or_else(|| "0".to_owned()),
            "0"
        );
    }
}
