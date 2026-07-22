use std::{panic::catch_unwind, sync::OnceLock, time::Instant};

use jni::{
    errors::ThrowRuntimeExAndDefault,
    objects::{JClass, JString},
    EnvUnowned,
};
use mina_runtime::Backend;
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
}
