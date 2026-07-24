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

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SubmittedTransaction {
    pub hash: String,
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
#[serde(rename_all = "camelCase")]
struct SendZkappData {
    send_zkapp: Option<SendZkappPayload>,
}

#[derive(Debug, Deserialize)]
struct SendZkappPayload {
    zkapp: SubmittedZkapp,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct SubmittedZkapp {
    hash: String,
    #[serde(default)]
    failure_reason: Option<Vec<ZkappFailureReason>>,
}

#[derive(Debug, Deserialize)]
struct ZkappFailureReason {
    #[serde(default)]
    failures: Vec<String>,
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

fn send_zkapp_mutation(zkapp_command: &serde_json::Value) -> String {
    format!(
        r#"mutation {{
  sendZkapp(input: {{ zkappCommand: {} }}) {{
    zkapp {{
      hash
      failureReason {{ failures index }}
    }}
  }}
}}"#,
        crate::graphql_json::graphql_literal(zkapp_command)
    )
}

fn parse_send_zkapp(
    response: GraphQlResponse<SendZkappData>,
) -> Result<SubmittedTransaction, String> {
    if !response.errors.is_empty() {
        let messages = response
            .errors
            .into_iter()
            .map(|error| error.message)
            .collect::<Vec<_>>()
            .join("; ");
        return Err(format!(
            "the Mina GraphQL endpoint rejected the transaction: {messages}"
        ));
    }
    let zkapp = response
        .data
        .and_then(|data| data.send_zkapp)
        .ok_or_else(|| "the Mina GraphQL response did not contain data".to_owned())?
        .zkapp;
    if let Some(failure_reason) = zkapp.failure_reason {
        let failures = failure_reason
            .into_iter()
            .flat_map(|reason| reason.failures)
            .collect::<Vec<_>>();
        if !failures.is_empty() {
            return Err(format!(
                "the transaction was rejected: {}",
                failures.join("; ")
            ));
        }
    }
    Ok(SubmittedTransaction { hash: zkapp.hash })
}

pub fn submit_zkapp_command(
    graphql_url: &str,
    zkapp_command: &serde_json::Value,
) -> Result<SubmittedTransaction, String> {
    // The proof makes the mutation weigh a few hundred kilobytes and the node
    // verifies it synchronously before answering, so leave far more room
    // than the lightweight snapshot queries get (o1js waits five minutes).
    let client = reqwest::blocking::Client::builder()
        .timeout(Duration::from_secs(300))
        .build()
        .map_err(|error| format!("could not initialize the HTTPS client: {error}"))?;
    let response = client
        .post(graphql_url)
        .json(&serde_json::json!({ "query": send_zkapp_mutation(zkapp_command) }))
        .send()
        .map_err(|error| format!("could not reach the Mina GraphQL endpoint: {error}"))?
        .error_for_status()
        .map_err(|error| format!("the Mina GraphQL endpoint rejected the request: {error}"))?
        .json::<GraphQlResponse<SendZkappData>>()
        .map_err(|error| format!("could not decode the Mina GraphQL response: {error}"))?;
    parse_send_zkapp(response)
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
    fn builds_a_send_zkapp_mutation_with_unquoted_keys() {
        let mutation = send_zkapp_mutation(&serde_json::json!({
            "feePayer": { "body": { "publicKey": "B62qtest", "fee": "100000000" } },
            "accountUpdates": [],
            "memo": "E4YM2vTH"
        }));

        assert!(mutation.contains("sendZkapp(input: { zkappCommand: "));
        assert!(mutation.contains("feePayer: "));
        assert!(mutation.contains(r#"publicKey: "B62qtest""#));
        assert!(mutation.contains("failureReason { failures index }"));
        assert!(!mutation.contains(r#""feePayer""#));
    }

    #[test]
    fn parses_a_successful_send_zkapp_response() {
        let response = serde_json::from_value(serde_json::json!({
            "data": {
                "sendZkapp": {
                    "zkapp": {
                        "hash": "5JuJ1eRxdopMHgm1eZAXjRNXvhuQVQipDGnMFDPYQXvKPWkx1SF7",
                        "failureReason": null
                    }
                }
            }
        }))
        .expect("valid fixture");

        let submitted = parse_send_zkapp(response).expect("submission must succeed");
        assert_eq!(
            submitted.hash,
            "5JuJ1eRxdopMHgm1eZAXjRNXvhuQVQipDGnMFDPYQXvKPWkx1SF7"
        );
    }

    #[test]
    fn surfaces_send_zkapp_rejections() {
        let graphql_error: GraphQlResponse<SendZkappData> =
            serde_json::from_value(serde_json::json!({
                "errors": [{ "message": "Invalid_nonce" }]
            }))
            .expect("valid fixture");
        let error = parse_send_zkapp(graphql_error).expect_err("GraphQL error must fail");
        assert!(error.contains("Invalid_nonce"));

        let failure: GraphQlResponse<SendZkappData> = serde_json::from_value(serde_json::json!({
            "data": {
                "sendZkapp": {
                    "zkapp": {
                        "hash": "5Ju...",
                        "failureReason": [
                            { "index": "2", "failures": ["Overflow"] }
                        ]
                    }
                }
            }
        }))
        .expect("valid fixture");
        let error = parse_send_zkapp(failure).expect_err("failure reason must fail");
        assert!(error.contains("Overflow"));
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
