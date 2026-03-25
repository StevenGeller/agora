// MPP (Machine Payments Protocol) implementation
// Based on IETF draft-ryan-httpauth-payment
// https://datatracker.ietf.org/doc/draft-ryan-httpauth-payment/
//
// Payment verification via Tempo blockchain (EVM-compatible L1)
// https://docs.tempo.xyz

use alloy_primitives::{Address, U256};
use axum::body::Body;
use axum::http::{header, Request, Response, StatusCode};
use axum::middleware::Next;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use base64::Engine;
use hmac::{Hmac, Mac};
use sha2::Sha256;
use std::collections::HashSet;
use std::sync::{Arc, Mutex};
use tracing::{error, info, warn};

type HmacSha256 = Hmac<Sha256>;

// ERC-20 Transfer event: keccak256("Transfer(address,address,uint256)")
const TRANSFER_EVENT_SIG: &str =
    "0xddf252ad1be2c89b69c2b068fc378daa952ba7f163c4a11628f55a4df523b3ef";

/// Configuration for MPP payment challenges and verification.
#[derive(Clone)]
pub struct MppConfig {
    pub realm: String,
    pub method: String,
    pub amount: String,   // human-readable, e.g. "0.001"
    pub amount_raw: U256, // in token base units (6 decimals)
    pub currency: String,
    pub recipient: Address,
    pub token_address: Address, // pathUSD on Tempo
    pub rpc_url: String,        // Tempo RPC endpoint
    pub description: String,
    pub secret: Vec<u8>,
    pub consumed_hashes: Arc<Mutex<HashSet<String>>>,
    pub consumed_hashes_path: String, // disk persistence for replay protection
}

/// Build the base64url-encoded `request` parameter for the challenge.
fn encode_request(amount: &str, currency: &str, recipient: &Address) -> String {
    let json = serde_json::json!({
        "amount": amount,
        "currency": currency,
        "recipient": format!("{:?}", recipient),
    });
    URL_SAFE_NO_PAD.encode(json.to_string().as_bytes())
}

/// Compute HMAC-SHA256 challenge ID per the spec.
fn compute_challenge_id(
    secret: &[u8],
    realm: &str,
    method: &str,
    intent: &str,
    request_b64: &str,
    expires: &str,
) -> String {
    let input = format!(
        "{}|{}|{}|{}|{}||",
        realm, method, intent, request_b64, expires
    );
    let mut mac = HmacSha256::new_from_slice(secret).expect("HMAC key");
    mac.update(input.as_bytes());
    let result = mac.finalize();
    URL_SAFE_NO_PAD.encode(result.into_bytes())
}

/// Build the full WWW-Authenticate: Payment challenge header value.
pub fn build_challenge(config: &MppConfig) -> String {
    let request_b64 = encode_request(&config.amount, &config.currency, &config.recipient);
    let expires = (chrono::Utc::now() + chrono::Duration::minutes(5))
        .format("%Y-%m-%dT%H:%M:%SZ")
        .to_string();

    let id = compute_challenge_id(
        &config.secret,
        &config.realm,
        &config.method,
        "charge",
        &request_b64,
        &expires,
    );

    format!(
        "Payment id=\"{}\", realm=\"{}\", method=\"{}\", intent=\"charge\", expires=\"{}\", description=\"{}\", request=\"{}\"",
        id, config.realm, config.method, expires, config.description, request_b64
    )
}

/// Build the 402 problem+json body per RFC 9457.
pub fn build_problem_body() -> String {
    serde_json::json!({
        "type": "https://paymentauth.org/problems/payment-required",
        "title": "Payment Required",
        "status": 402,
    })
    .to_string()
}

/// Verify an Authorization: Payment credential against the config.
/// Checks HMAC binding, expiry, AND verifies the on-chain transaction on Tempo.
pub async fn verify_credential(config: &MppConfig, auth_header: &str) -> bool {
    let token = auth_header.strip_prefix("Payment ").unwrap_or(auth_header);
    let decoded = match URL_SAFE_NO_PAD.decode(token) {
        Ok(d) => d,
        Err(_) => return false,
    };
    let cred: serde_json::Value = match serde_json::from_slice(&decoded) {
        Ok(v) => v,
        Err(_) => return false,
    };

    // 1. Verify challenge HMAC binding
    let challenge = match cred.get("challenge") {
        Some(c) => c,
        None => return false,
    };
    let id = challenge.get("id").and_then(|v| v.as_str()).unwrap_or("");
    let realm = challenge
        .get("realm")
        .and_then(|v| v.as_str())
        .unwrap_or("");
    let method = challenge
        .get("method")
        .and_then(|v| v.as_str())
        .unwrap_or("");
    let intent = challenge
        .get("intent")
        .and_then(|v| v.as_str())
        .unwrap_or("");
    let request = challenge
        .get("request")
        .and_then(|v| v.as_str())
        .unwrap_or("");
    let expires = challenge
        .get("expires")
        .and_then(|v| v.as_str())
        .unwrap_or("");

    // Constant-time HMAC verification (prevents timing attacks)
    let input = format!("{}|{}|{}|{}|{}||", realm, method, intent, request, expires);
    let mut mac = HmacSha256::new_from_slice(&config.secret).expect("HMAC key");
    mac.update(input.as_bytes());
    let provided_bytes = match URL_SAFE_NO_PAD.decode(id) {
        Ok(b) => b,
        Err(_) => {
            warn!("mpp: invalid challenge ID encoding");
            return false;
        }
    };
    if mac.verify_slice(&provided_bytes).is_err() {
        warn!("mpp: challenge ID mismatch");
        return false;
    }

    // 2. Check expiry
    if let Ok(exp) = chrono::DateTime::parse_from_rfc3339(expires) {
        if exp < chrono::Utc::now() {
            warn!("mpp: challenge expired");
            return false;
        }
    }

    // 3. Extract transaction hash from payload
    let payload = match cred.get("payload") {
        Some(p) => p,
        None => {
            warn!("mpp: no payload in credential");
            return false;
        }
    };
    let tx_hash = match payload.get("tx").and_then(|v| v.as_str()) {
        Some(h) => h.to_string(),
        None => {
            warn!("mpp: no tx hash in payload");
            return false;
        }
    };

    // 4. Check for replay — reject already-consumed tx hashes
    {
        let consumed = config.consumed_hashes.lock().unwrap();
        if consumed.contains(&tx_hash) {
            warn!(tx = %tx_hash, "mpp: replayed tx");
            return false;
        }
    }

    // 5. Verify the transaction on Tempo chain
    match verify_tempo_tx(config, &tx_hash).await {
        Ok(true) => {
            // Persist to disk before inserting (tx_hash moves on insert)
            if let Ok(mut f) = std::fs::OpenOptions::new()
                .create(true)
                .append(true)
                .open(&config.consumed_hashes_path)
            {
                let _ = std::io::Write::write_fmt(&mut f, format_args!("{}\n", tx_hash));
            }
            let mut consumed = config.consumed_hashes.lock().unwrap();
            consumed.insert(tx_hash);
            true
        }
        Ok(false) => {
            warn!(tx = %tx_hash, "mpp: tx verification failed");
            false
        }
        Err(e) => {
            error!(error = %e, "mpp: tx verification error");
            false
        }
    }
}

/// Verify a transaction receipt on Tempo chain.
/// Checks that the tx contains a Transfer event to the seller for the right amount.
async fn verify_tempo_tx(config: &MppConfig, tx_hash: &str) -> Result<bool, String> {
    let client = reqwest::Client::new();
    let resp = client
        .post(&config.rpc_url)
        .json(&serde_json::json!({
            "jsonrpc": "2.0",
            "method": "eth_getTransactionReceipt",
            "params": [tx_hash],
            "id": 1
        }))
        .send()
        .await
        .map_err(|e| format!("rpc request failed: {}", e))?;

    let body: serde_json::Value = resp
        .json()
        .await
        .map_err(|e| format!("rpc response parse failed: {}", e))?;

    let receipt = body.get("result").ok_or("no result in rpc response")?;
    if receipt.is_null() {
        return Err("transaction not found (null receipt)".into());
    }

    // Check status (0x1 = success)
    let status = receipt
        .get("status")
        .and_then(|v| v.as_str())
        .unwrap_or("0x0");
    if status != "0x1" {
        return Err(format!("transaction reverted (status: {})", status));
    }

    // Check logs for a Transfer event to the seller address
    let logs = receipt
        .get("logs")
        .and_then(|v| v.as_array())
        .ok_or("no logs in receipt")?;

    let seller_topic = format!(
        "0x000000000000000000000000{}",
        hex::encode(config.recipient.as_slice())
    );
    let token_addr = format!("{:?}", config.token_address).to_lowercase();

    for log in logs {
        let log_addr = log
            .get("address")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_lowercase();

        // Must be from the correct token contract
        if log_addr != token_addr {
            continue;
        }

        let topics = match log.get("topics").and_then(|v| v.as_array()) {
            Some(t) => t,
            None => continue,
        };

        // topic[0] must be Transfer event signature
        if topics.len() < 3 {
            continue;
        }
        let topic0 = topics[0].as_str().unwrap_or("");
        if topic0 != TRANSFER_EVENT_SIG {
            continue;
        }

        // topic[2] = recipient (indexed `to` address)
        let topic2 = topics[2].as_str().unwrap_or("").to_lowercase();
        if topic2 != seller_topic {
            continue;
        }

        // data = uint256 amount transferred
        let data = log.get("data").and_then(|v| v.as_str()).unwrap_or("0x0");
        let amount = U256::from_str_radix(data.trim_start_matches("0x"), 16).unwrap_or(U256::ZERO);

        if amount >= config.amount_raw {
            info!(tx = %tx_hash, amount = %amount, to = ?config.recipient, "mpp: verified tx");
            return Ok(true);
        }
    }

    Err(format!(
        "no matching Transfer event found in {} logs",
        logs.len()
    ))
}

/// Build a Payment-Receipt from a verified transaction.
pub fn build_receipt(config: &MppConfig, tx_hash: &str) -> String {
    let receipt = serde_json::json!({
        "status": "settled",
        "method": config.method,
        "chain": "tempo",
        "tx": tx_hash,
        "timestamp": chrono::Utc::now().format("%Y-%m-%dT%H:%M:%SZ").to_string(),
        "amount": config.amount,
        "currency": config.currency,
    });
    URL_SAFE_NO_PAD.encode(receipt.to_string().as_bytes())
}

/// Axum middleware that enforces MPP payment on a route.
/// Verifies real on-chain Tempo transactions.
pub async fn mpp_middleware(
    axum::extract::State(config): axum::extract::State<MppConfig>,
    req: Request<Body>,
    next: Next,
) -> Response<Body> {
    // Check for Authorization: Payment header
    if let Some(auth) = req.headers().get(header::AUTHORIZATION) {
        if let Ok(auth_str) = auth.to_str() {
            if auth_str.starts_with("Payment ") && verify_credential(&config, auth_str).await {
                // Extract tx hash for receipt
                let tx_hash = extract_tx_hash(auth_str).unwrap_or_default();

                let mut resp = next.run(req).await;
                let receipt = build_receipt(&config, &tx_hash);
                resp.headers_mut()
                    .insert("payment-receipt", receipt.parse().unwrap());
                return resp;
            }
        }
    }

    // No valid payment — return 402 challenge
    let challenge = build_challenge(&config);
    let body = build_problem_body();

    Response::builder()
        .status(StatusCode::PAYMENT_REQUIRED)
        .header("WWW-Authenticate", challenge)
        .header("Cache-Control", "no-store")
        .header(header::CONTENT_TYPE, "application/problem+json")
        .body(Body::from(body))
        .unwrap()
}

fn extract_tx_hash(auth_header: &str) -> Option<String> {
    let token = auth_header.strip_prefix("Payment ")?;
    let decoded = URL_SAFE_NO_PAD.decode(token).ok()?;
    let cred: serde_json::Value = serde_json::from_slice(&decoded).ok()?;
    cred.get("payload")?
        .get("tx")?
        .as_str()
        .map(|s| s.to_string())
}

/// Send a real pathUSD transfer on Tempo and return the tx hash.
/// Used by the demo buyer.
pub async fn send_tempo_transfer(
    rpc_url: &str,
    private_key: &str,
    token_address: Address,
    to: Address,
    amount: U256,
) -> Result<String, String> {
    let signer: alloy_signer_local::PrivateKeySigner = private_key
        .parse()
        .map_err(|e| format!("invalid key: {}", e))?;
    let from = signer.address();
    let client = reqwest::Client::new();

    // 1. Get nonce
    let nonce = json_rpc_call(
        &client,
        rpc_url,
        "eth_getTransactionCount",
        serde_json::json!([format!("{:?}", from), "latest"]),
    )
    .await?;
    let nonce = u64::from_str_radix(
        nonce.as_str().ok_or("bad nonce")?.trim_start_matches("0x"),
        16,
    )
    .map_err(|e| format!("nonce parse: {}", e))?;

    // 2. Get gas price
    let gas_price = json_rpc_call(&client, rpc_url, "eth_gasPrice", serde_json::json!([])).await?;
    let gas_price = U256::from_str_radix(
        gas_price
            .as_str()
            .ok_or("bad gas")?
            .trim_start_matches("0x"),
        16,
    )
    .unwrap_or(U256::from(1_000_000_000u64)); // 1 gwei fallback

    // 3. Encode transfer(address,uint256) calldata
    // selector: 0xa9059cbb
    let mut calldata = vec![0xa9, 0x05, 0x9c, 0xbb];
    // address padded to 32 bytes
    calldata.extend_from_slice(&[0u8; 12]);
    calldata.extend_from_slice(to.as_slice());
    // uint256 amount padded to 32 bytes
    let amount_bytes: [u8; 32] = amount.to_be_bytes::<32>();
    calldata.extend_from_slice(&amount_bytes);

    // 4. Get chain ID
    let chain_id_hex =
        json_rpc_call(&client, rpc_url, "eth_chainId", serde_json::json!([])).await?;
    let chain_id = u64::from_str_radix(
        chain_id_hex
            .as_str()
            .ok_or("bad chain id")?
            .trim_start_matches("0x"),
        16,
    )
    .map_err(|e| format!("chain id parse: {}", e))?;

    // 5. Build legacy transaction for signing
    let gas_limit = 500_000u64; // Tempo TIP-20 transfers need ~300K gas
    let tx_for_signing = encode_legacy_tx_for_signing(
        nonce,
        gas_price,
        gas_limit,
        token_address,
        U256::ZERO,
        &calldata,
        chain_id,
    );

    // 6. Sign
    use alloy_primitives::keccak256;
    use alloy_signer::Signer;
    let tx_hash = keccak256(&tx_for_signing);
    let sig = signer
        .sign_hash(&tx_hash)
        .await
        .map_err(|e| format!("signing failed: {}", e))?;

    // 7. Encode signed transaction
    let signed_tx = encode_signed_legacy_tx(
        nonce,
        gas_price,
        gas_limit,
        token_address,
        U256::ZERO,
        &calldata,
        chain_id,
        &sig,
    );

    let raw_tx = format!("0x{}", alloy_primitives::hex::encode(&signed_tx));

    // 8. Send
    let tx_hash_result = json_rpc_call(
        &client,
        rpc_url,
        "eth_sendRawTransaction",
        serde_json::json!([raw_tx]),
    )
    .await?;

    let hash = tx_hash_result
        .as_str()
        .ok_or("no tx hash in response")?
        .to_string();

    info!(tx = %hash, "mpp: sent pathUSD transfer");

    // 9. Wait for receipt (poll up to 15 seconds)
    for _ in 0..30 {
        tokio::time::sleep(std::time::Duration::from_millis(500)).await;
        let receipt = json_rpc_call(
            &client,
            rpc_url,
            "eth_getTransactionReceipt",
            serde_json::json!([&hash]),
        )
        .await;
        if let Ok(r) = receipt {
            if !r.is_null() {
                let status = r.get("status").and_then(|v| v.as_str()).unwrap_or("0x0");
                if status == "0x1" {
                    info!(tx = %hash, "mpp: tx confirmed");
                    return Ok(hash);
                } else {
                    return Err(format!("tx reverted: {}", hash));
                }
            }
        }
    }

    // Return hash even if not yet confirmed — server will verify on its own
    Ok(hash)
}

async fn json_rpc_call(
    client: &reqwest::Client,
    rpc_url: &str,
    method: &str,
    params: serde_json::Value,
) -> Result<serde_json::Value, String> {
    let resp = client
        .post(rpc_url)
        .json(&serde_json::json!({
            "jsonrpc": "2.0",
            "method": method,
            "params": params,
            "id": 1
        }))
        .send()
        .await
        .map_err(|e| format!("rpc error: {}", e))?;

    let body: serde_json::Value = resp
        .json()
        .await
        .map_err(|e| format!("rpc parse error: {}", e))?;

    if let Some(err) = body.get("error") {
        return Err(format!("rpc error: {}", err));
    }

    body.get("result")
        .cloned()
        .ok_or("no result in rpc response".into())
}

// --- RLP encoding for legacy transactions ---

fn rlp_encode_u64(val: u64) -> Vec<u8> {
    if val == 0 {
        return vec![0x80]; // empty string
    }
    let bytes = val.to_be_bytes();
    let start = bytes.iter().position(|&b| b != 0).unwrap_or(7);
    let trimmed = &bytes[start..];
    if trimmed.len() == 1 && trimmed[0] < 0x80 {
        trimmed.to_vec()
    } else {
        let mut out = vec![0x80 + trimmed.len() as u8];
        out.extend_from_slice(trimmed);
        out
    }
}

fn rlp_encode_u256(val: U256) -> Vec<u8> {
    if val.is_zero() {
        return vec![0x80];
    }
    let bytes: [u8; 32] = val.to_be_bytes::<32>();
    let start = bytes.iter().position(|&b| b != 0).unwrap_or(31);
    let trimmed = &bytes[start..];
    if trimmed.len() == 1 && trimmed[0] < 0x80 {
        trimmed.to_vec()
    } else {
        let mut out = vec![0x80 + trimmed.len() as u8];
        out.extend_from_slice(trimmed);
        out
    }
}

fn rlp_encode_bytes(data: &[u8]) -> Vec<u8> {
    if data.len() == 1 && data[0] < 0x80 {
        return data.to_vec();
    }
    if data.is_empty() {
        return vec![0x80];
    }
    if data.len() < 56 {
        let mut out = vec![0x80 + data.len() as u8];
        out.extend_from_slice(data);
        out
    } else {
        let len_bytes = (data.len() as u64).to_be_bytes();
        let start = len_bytes.iter().position(|&b| b != 0).unwrap_or(7);
        let len_trimmed = &len_bytes[start..];
        let mut out = vec![0xb7 + len_trimmed.len() as u8];
        out.extend_from_slice(len_trimmed);
        out.extend_from_slice(data);
        out
    }
}

fn rlp_encode_address(addr: Address) -> Vec<u8> {
    let mut out = vec![0x80 + 20];
    out.extend_from_slice(addr.as_slice());
    out
}

fn rlp_list(items: &[Vec<u8>]) -> Vec<u8> {
    let payload: Vec<u8> = items.iter().flatten().copied().collect();
    if payload.len() < 56 {
        let mut out = vec![0xc0 + payload.len() as u8];
        out.extend_from_slice(&payload);
        out
    } else {
        let len_bytes = (payload.len() as u64).to_be_bytes();
        let start = len_bytes.iter().position(|&b| b != 0).unwrap_or(7);
        let len_trimmed = &len_bytes[start..];
        let mut out = vec![0xf7 + len_trimmed.len() as u8];
        out.extend_from_slice(len_trimmed);
        out.extend_from_slice(&payload);
        out
    }
}

fn encode_legacy_tx_for_signing(
    nonce: u64,
    gas_price: U256,
    gas_limit: u64,
    to: Address,
    value: U256,
    data: &[u8],
    chain_id: u64,
) -> Vec<u8> {
    // EIP-155: [nonce, gasPrice, gasLimit, to, value, data, chainId, 0, 0]
    rlp_list(&[
        rlp_encode_u64(nonce),
        rlp_encode_u256(gas_price),
        rlp_encode_u64(gas_limit),
        rlp_encode_address(to),
        rlp_encode_u256(value),
        rlp_encode_bytes(data),
        rlp_encode_u64(chain_id),
        vec![0x80], // empty for EIP-155
        vec![0x80], // empty for EIP-155
    ])
}

#[allow(clippy::too_many_arguments)]
fn encode_signed_legacy_tx(
    nonce: u64,
    gas_price: U256,
    gas_limit: u64,
    to: Address,
    value: U256,
    data: &[u8],
    chain_id: u64,
    sig: &alloy_primitives::Signature,
) -> Vec<u8> {
    let v = sig.v() as u64 + chain_id * 2 + 35;
    let r = sig.r();
    let s = sig.s();

    rlp_list(&[
        rlp_encode_u64(nonce),
        rlp_encode_u256(gas_price),
        rlp_encode_u64(gas_limit),
        rlp_encode_address(to),
        rlp_encode_u256(value),
        rlp_encode_bytes(data),
        rlp_encode_u64(v),
        rlp_encode_u256(r),
        rlp_encode_u256(s),
    ])
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_config() -> MppConfig {
        MppConfig {
            realm: "test.example.com".into(),
            method: "tempo".into(),
            amount: "0.001".into(),
            amount_raw: U256::from(1000u64),
            currency: "pathUSD".into(),
            recipient: "0x1ecED38210cA1335f9FD38399e64d2C77C2D7cF3"
                .parse()
                .unwrap(),
            token_address: "0x20c0000000000000000000000000000000000000"
                .parse()
                .unwrap(),
            rpc_url: "https://rpc.test".into(),
            description: "Test".into(),
            secret: b"test-secret-32-bytes-long-enough".to_vec(),
            consumed_hashes: Arc::new(Mutex::new(HashSet::new())),
            consumed_hashes_path: "/dev/null".into(),
        }
    }

    #[test]
    fn encode_request_roundtrip() {
        let addr: Address = "0x1ecED38210cA1335f9FD38399e64d2C77C2D7cF3"
            .parse()
            .unwrap();
        let encoded = encode_request("0.001", "pathUSD", &addr);
        let decoded = URL_SAFE_NO_PAD.decode(&encoded).unwrap();
        let json: serde_json::Value = serde_json::from_slice(&decoded).unwrap();
        assert_eq!(json["amount"], "0.001");
        assert_eq!(json["currency"], "pathUSD");
    }

    #[test]
    fn challenge_id_deterministic() {
        let secret = b"test-secret";
        let id1 = compute_challenge_id(
            secret,
            "realm",
            "method",
            "charge",
            "req",
            "2026-01-01T00:00:00Z",
        );
        let id2 = compute_challenge_id(
            secret,
            "realm",
            "method",
            "charge",
            "req",
            "2026-01-01T00:00:00Z",
        );
        assert_eq!(id1, id2);
    }

    #[test]
    fn challenge_id_varies_with_realm() {
        let secret = b"test-secret";
        let id1 = compute_challenge_id(secret, "realm1", "m", "charge", "r", "exp");
        let id2 = compute_challenge_id(secret, "realm2", "m", "charge", "r", "exp");
        assert_ne!(id1, id2);
    }

    #[test]
    fn challenge_id_varies_with_secret() {
        let id1 = compute_challenge_id(b"secret1", "r", "m", "charge", "r", "exp");
        let id2 = compute_challenge_id(b"secret2", "r", "m", "charge", "r", "exp");
        assert_ne!(id1, id2);
    }

    #[test]
    fn build_problem_body_format() {
        let body = build_problem_body();
        let json: serde_json::Value = serde_json::from_str(&body).unwrap();
        assert_eq!(json["status"], 402);
        assert_eq!(json["title"], "Payment Required");
    }

    #[test]
    fn build_challenge_contains_required_fields() {
        let config = test_config();
        let challenge = build_challenge(&config);
        assert!(challenge.contains("realm=\"test.example.com\""));
        assert!(challenge.contains("method=\"tempo\""));
        assert!(challenge.contains("intent=\"charge\""));
        assert!(challenge.contains("expires="));
        assert!(challenge.contains("request="));
        assert!(challenge.contains("id="));
    }

    #[test]
    fn build_receipt_roundtrip() {
        let config = test_config();
        let receipt_b64 = build_receipt(&config, "0xabc123");
        let decoded = URL_SAFE_NO_PAD.decode(&receipt_b64).unwrap();
        let json: serde_json::Value = serde_json::from_slice(&decoded).unwrap();
        assert_eq!(json["status"], "settled");
        assert_eq!(json["tx"], "0xabc123");
        assert_eq!(json["method"], "tempo");
        assert_eq!(json["amount"], "0.001");
        assert_eq!(json["currency"], "pathUSD");
    }

    #[test]
    fn extract_tx_hash_valid() {
        let credential = serde_json::json!({
            "challenge": {},
            "payload": { "tx": "0xdeadbeef" }
        });
        let b64 = URL_SAFE_NO_PAD.encode(credential.to_string().as_bytes());
        let header = format!("Payment {}", b64);
        assert_eq!(extract_tx_hash(&header), Some("0xdeadbeef".to_string()));
    }

    #[test]
    fn extract_tx_hash_invalid() {
        assert_eq!(extract_tx_hash("Payment invalid"), None);
        assert_eq!(extract_tx_hash("Bearer token"), None);
    }

    // --- RLP encoding tests ---

    #[test]
    fn rlp_u64_zero() {
        assert_eq!(rlp_encode_u64(0), vec![0x80]);
    }

    #[test]
    fn rlp_u64_single_byte() {
        assert_eq!(rlp_encode_u64(1), vec![0x01]);
        assert_eq!(rlp_encode_u64(127), vec![0x7f]);
    }

    #[test]
    fn rlp_u64_two_bytes() {
        assert_eq!(rlp_encode_u64(128), vec![0x81, 0x80]);
        assert_eq!(rlp_encode_u64(256), vec![0x82, 0x01, 0x00]);
    }

    #[test]
    fn rlp_u256_zero() {
        assert_eq!(rlp_encode_u256(U256::ZERO), vec![0x80]);
    }

    #[test]
    fn rlp_u256_small() {
        assert_eq!(rlp_encode_u256(U256::from(1u64)), vec![0x01]);
        assert_eq!(rlp_encode_u256(U256::from(127u64)), vec![0x7f]);
    }

    #[test]
    fn rlp_bytes_empty() {
        assert_eq!(rlp_encode_bytes(&[]), vec![0x80]);
    }

    #[test]
    fn rlp_bytes_single_low() {
        assert_eq!(rlp_encode_bytes(&[0x42]), vec![0x42]);
    }

    #[test]
    fn rlp_bytes_single_high() {
        assert_eq!(rlp_encode_bytes(&[0x80]), vec![0x81, 0x80]);
    }

    #[test]
    fn rlp_bytes_multi() {
        let data = vec![0x01, 0x02, 0x03];
        let encoded = rlp_encode_bytes(&data);
        assert_eq!(encoded[0], 0x80 + 3);
        assert_eq!(&encoded[1..], &[0x01, 0x02, 0x03]);
    }

    #[test]
    fn rlp_list_empty() {
        assert_eq!(rlp_list(&[]), vec![0xc0]);
    }

    #[test]
    fn rlp_list_single_item() {
        let items = vec![rlp_encode_u64(1)]; // [0x01]
        let result = rlp_list(&items);
        assert_eq!(result, vec![0xc1, 0x01]);
    }

    #[test]
    fn rlp_address_length() {
        let addr: Address = "0x1ecED38210cA1335f9FD38399e64d2C77C2D7cF3"
            .parse()
            .unwrap();
        let encoded = rlp_encode_address(addr);
        assert_eq!(encoded.len(), 21); // 1 byte prefix + 20 bytes address
        assert_eq!(encoded[0], 0x80 + 20);
    }
}
