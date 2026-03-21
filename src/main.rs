mod mpp;

use axum::{
    Router,
    extract::Json,
    middleware as axum_mw,
    response::IntoResponse,
    routing::{get, post},
};
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use base64::Engine;
use rand::seq::SliceRandom;
use serde::{Deserialize, Serialize};
use std::collections::VecDeque;
use std::net::SocketAddr;
use std::sync::{Mutex, OnceLock};
use std::time::Instant;
use tower_http::services::ServeDir;

// ---------------------------------------------------------------------------
// Rate limiter (global sliding window)
// ---------------------------------------------------------------------------

struct RateLimiter {
    window: std::time::Duration,
    max_events: usize,
    events: Mutex<VecDeque<Instant>>,
}

impl RateLimiter {
    fn new(max_events: usize, window_secs: u64) -> Self {
        Self {
            window: std::time::Duration::from_secs(window_secs),
            max_events,
            events: Mutex::new(VecDeque::new()),
        }
    }

    fn check(&self) -> bool {
        let now = Instant::now();
        let mut events = self.events.lock().unwrap();
        while let Some(&front) = events.front() {
            if now.duration_since(front) > self.window {
                events.pop_front();
            } else {
                break;
            }
        }
        if events.len() >= self.max_events {
            false
        } else {
            events.push_back(now);
            true
        }
    }
}

static PURCHASE_LIMITER: OnceLock<RateLimiter> = OnceLock::new();
static BALANCE_LIMITER: OnceLock<RateLimiter> = OnceLock::new();

use x402_axum::X402Middleware;
use x402_chain_eip155::{KnownNetworkEip155, V2Eip155Exact, V2Eip155ExactClient};
use x402_reqwest::{ReqwestWithPayments, ReqwestWithPaymentsBuild, X402Client};
use x402_types::networks::USDC;

use alloy_signer_local::PrivateKeySigner;
use std::sync::Arc;

// ---------------------------------------------------------------------------
// Content
// ---------------------------------------------------------------------------

const HAIKUS: &[&str] = &[
    "packets cross the wire\nrouters hum in steady pulse\ndata finds its home",
    "the kernel awakes\nprocesses fork into dawn\nmemory is shared",
    "a single zero\nflips the meaning of it all\nbinary twilight",
    "cache lines overflow\nthe branch predictor was wrong\npipeline flushed clean",
    "merkle roots hold fast\nhashes chain through every block\ntrust without a name",
    "segfault in the night\na pointer drifts past its bounds\ncore dump tells the tale",
    "three handshakes exchanged\nthe socket opens its mouth\nstreams begin to flow",
    "bits travel through glass\nphotons bounce at every bend\nfiber carries light",
    "the garbage collector\nsweeps the heap while threads all sleep\nmemory renewed",
    "consensus is reached\nvalidators sign the block\nstate transitions sealed",
    "the load balancer\nspreads requests like autumn leaves\nno server complains",
    "ssh tunnel dug\nthrough firewalls and proxy chains\nremote shell awaits",
];

const QUOTES: &[(&str, &str)] = &[
    ("The best way to predict the future is to invent it.", "Alan Kay"),
    ("Simplicity is prerequisite for reliability.", "Edsger W. Dijkstra"),
    ("Programs must be written for people to read, and only incidentally for machines to execute.", "Harold Abelson"),
    ("The most dangerous phrase in the language is: we have always done it this way.", "Grace Hopper"),
    ("Any sufficiently advanced technology is indistinguishable from magic.", "Arthur C. Clarke"),
    ("First, solve the problem. Then, write the code.", "John Johnson"),
    ("Talk is cheap. Show me the code.", "Linus Torvalds"),
    ("The function of good software is to make the complex appear to be simple.", "Grady Booch"),
    ("Measuring programming progress by lines of code is like measuring aircraft building progress by weight.", "Bill Gates"),
    ("It is not enough for code to work.", "Robert C. Martin"),
    ("The best performance improvement is the transition from the nonworking state to the working state.", "John Ousterhout"),
    ("A language that does not affect the way you think about programming is not worth knowing.", "Alan Perlis"),
];

const FACTS: &[&str] = &[
    "The first computer bug was an actual moth found in a Harvard Mark II relay in 1947.",
    "A mass of 1 kilogram can store about 9 x 10^16 joules of energy, per E=mc^2.",
    "The Apollo 11 guidance computer had 74KB of memory and operated at 0.043 MHz.",
    "There are approximately 10^80 atoms in the observable universe.",
    "A single strand of optical fiber can carry 100 terabits per second.",
    "The first message sent over ARPANET in 1969 was 'LO' -- the system crashed before 'LOGIN' completed.",
    "Quantum computers use qubits that can exist in superposition, representing 0 and 1 simultaneously.",
    "The human brain has roughly 86 billion neurons, each with up to 10,000 synaptic connections.",
    "GPS satellites must account for both special and general relativistic time dilation to maintain accuracy.",
    "The Bitcoin network consumes more electricity annually than many mid-sized countries.",
    "ECC memory can detect and correct single-bit errors, making it essential for servers and scientific computing.",
    "The Voyager 1 probe, launched in 1977, communicates at 160 bits per second from 15 billion miles away.",
    "A modern CPU transistor gate is roughly 3 nanometers wide, about 15 silicon atoms across.",
];

const TORUS_WORDS: &[&str] = &[
    "time", "light", "void", "dream", "echo",
    "flux", "pulse", "wave", "root", "seed",
    "fire", "stone", "wind", "rain", "star",
];

// ---------------------------------------------------------------------------
// Request/response types
// ---------------------------------------------------------------------------

#[derive(Serialize)]
struct HaikuResponse {
    haiku: String,
}

#[derive(Serialize)]
struct QuoteResponse {
    quote: String,
    author: String,
}

#[derive(Serialize)]
struct FactResponse {
    fact: String,
}

#[derive(Deserialize)]
struct PurchaseRequest {
    endpoint: String,
    #[serde(default = "default_protocol")]
    protocol: String,
}

fn default_protocol() -> String {
    "x402-testnet".into()
}

#[derive(Serialize)]
struct Step {
    name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    detail: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    headers: Option<serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    wallet: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    content: Option<String>,
    tx_hash: None,
    #[serde(skip_serializing_if = "Option::is_none")]
    tx_hash: Option<String>,
}

#[derive(Serialize)]
struct PurchaseResponse {
    steps: Vec<Step>,
    result: serde_json::Value,
    elapsed_ms: u128,
}

// ---------------------------------------------------------------------------
// Paid endpoint handlers
// ---------------------------------------------------------------------------

async fn haiku_handler() -> impl IntoResponse {
    let mut rng = rand::thread_rng();
    let haiku = HAIKUS.choose(&mut rng).unwrap();
    Json(HaikuResponse {
        haiku: haiku.to_string(),
    })
}

async fn quote_handler() -> impl IntoResponse {
    let mut rng = rand::thread_rng();
    let (quote, author) = QUOTES.choose(&mut rng).unwrap();
    Json(QuoteResponse {
        quote: quote.to_string(),
        author: author.to_string(),
    })
}

async fn fact_handler() -> impl IntoResponse {
    let mut rng = rand::thread_rng();
    let fact = FACTS.choose(&mut rng).unwrap();
    Json(FactResponse {
        fact: fact.to_string(),
    })
}

async fn torus_handler() -> Json<serde_json::Value> {
    let word = {
        let mut rng = rand::thread_rng();
        *TORUS_WORDS.choose(&mut rng).unwrap()
    };

    let client = reqwest::Client::new();
    let body = match client
        .post("http://127.0.0.1:3031/api/generate")
        .json(&serde_json::json!({"text": word}))
        .send()
        .await
    {
        Ok(resp) => {
            let json: serde_json::Value = resp.json().await.unwrap_or_default();
            json.get("svg").and_then(|s| s.as_str()).unwrap_or("<svg/>").to_string()
        }
        Err(e) => {
            eprintln!("torus fetch failed: {}", e);
            "<svg xmlns=\"http://www.w3.org/2000/svg\" viewBox=\"0 0 200 200\"><text x=\"100\" y=\"100\" text-anchor=\"middle\" fill=\"currentColor\" font-size=\"14\">unavailable</text></svg>".to_string()
        }
    };

    Json(serde_json::json!({"svg": body, "word": word}))
}

// ---------------------------------------------------------------------------
// Demo buyer: balance handler
// ---------------------------------------------------------------------------

#[derive(Deserialize)]
struct BalanceRequest {
    protocol: String,
}

async fn balance_handler(Json(req): Json<BalanceRequest>) -> impl IntoResponse {
    let limiter = BALANCE_LIMITER.get_or_init(|| RateLimiter::new(60, 60));
    if !limiter.check() {
        return Json(serde_json::json!({"error": "rate limit exceeded"}));
    }

    let private_key = std::env::var("BUYER_PRIVATE_KEY").unwrap_or_default();
    let signer: PrivateKeySigner = match private_key.parse() {
        Ok(s) => s,
        Err(_) => return Json(serde_json::json!({"error": "no wallet configured"})),
    };
    let wallet = format!("{:?}", signer.address());

    let (rpc_url, token_address, chain_label, token_symbol, explorer_url) = match req.protocol.as_str() {
        "mpp" => (
            std::env::var("TEMPO_RPC_URL")
                .unwrap_or_else(|_| "https://rpc.moderato.tempo.xyz".into()),
            "0x20c0000000000000000000000000000000000000",
            "Tempo Moderato",
            "pathUSD",
            "https://explore.tempo.xyz",
        ),
        "x402-testnet" => (
            "https://sepolia.base.org".into(),
            "0x036CbD53842c5426634e7929541eC2318f3dCF7e", // USDC on Base Sepolia
            "Base Sepolia",
            "USDC",
            "https://sepolia.basescan.org",
        ),
        _ => return Json(serde_json::json!({"error": "unknown protocol"})),
    };

    // ERC-20 balanceOf(address) call
    let addr_padded = format!("{:0>64}", &wallet[2..]);
    let data = format!("0x70a08231{}", addr_padded);

    let client = reqwest::Client::new();
    let resp = client
        .post(&rpc_url)
        .json(&serde_json::json!({
            "jsonrpc": "2.0",
            "method": "eth_call",
            "params": [{"to": token_address, "data": data}, "latest"],
            "id": 1
        }))
        .send()
        .await;

    let balance_raw = match resp {
        Ok(r) => {
            let json: serde_json::Value = r.json().await.unwrap_or_default();
            json.get("result")
                .and_then(|v| v.as_str())
                .unwrap_or("0x0")
                .to_string()
        }
        Err(e) => {
            eprintln!("balance rpc failed: {}", e);
            return Json(serde_json::json!({"error": "balance check failed"}));
        }
    };

    // Parse hex balance (6 decimals for both USDC and pathUSD)
    let raw = u128::from_str_radix(balance_raw.trim_start_matches("0x"), 16).unwrap_or(0);
    let whole = raw / 1_000_000;
    let frac = raw % 1_000_000;
    let formatted = format!("{}.{:06}", whole, frac);

    Json(serde_json::json!({
        "wallet": wallet,
        "balance": formatted,
        "token": token_symbol,
        "chain": chain_label,
        "explorer": explorer_url,
    }))
}

// ---------------------------------------------------------------------------
// Demo buyer: purchase handler
// ---------------------------------------------------------------------------

async fn purchase_handler(Json(req): Json<PurchaseRequest>) -> impl IntoResponse {
    let limiter = PURCHASE_LIMITER.get_or_init(|| RateLimiter::new(20, 60));
    if !limiter.check() {
        return Json(serde_json::json!({"error": "rate limit exceeded, try again later"}));
    }

    let start = Instant::now();
    let endpoint = req.endpoint;
    let protocol = req.protocol;

    let base_path = match endpoint.as_str() {
        "haiku" | "quote" | "fact" | "torus" => endpoint.as_str(),
        _ => {
            return Json(serde_json::json!({
                "error": format!("unknown endpoint: {}", endpoint)
            }));
        }
    };

    // x402-testnet → /test/*, x402-mainnet → /api/*, mpp → /mpp/*
    let path = match protocol.as_str() {
        "x402-testnet" => format!("/test/{}", base_path),
        "x402-mainnet" => format!("/api/{}", base_path),
        "mpp" => format!("/mpp/{}", base_path),
        _ => {
            return Json(serde_json::json!({
                "error": format!("unknown protocol: {}", protocol)
            }));
        }
    };

    let is_mpp = protocol == "mpp";

    let url = format!("http://127.0.0.1:3033{}", path);
    let mut steps: Vec<Step> = Vec::new();

    // Step 1: record the initial request
    steps.push(Step {
        name: "request".into(),
        detail: Some(format!("GET {} [{}]", path, protocol)),
        headers: None,
        wallet: None,
        content: None,
        tx_hash: None,
    });

    // Make initial request without payment -- expect 402
    let plain_client = reqwest::Client::new();
    let resp = match plain_client.get(&url).send().await {
        Ok(r) => r,
        Err(e) => {
            return Json(serde_json::json!({
                "error": format!("request failed: {}", e)
            }));
        }
    };

    let status = resp.status().as_u16();
    if status != 402 {
        // Unexpected: endpoint didn't require payment
        let body = resp.text().await.unwrap_or_default();
        let result: serde_json::Value =
            serde_json::from_str(&body).unwrap_or(serde_json::json!({"raw": body}));
        steps.push(Step {
            name: format!("{}", status),
            detail: Some("No payment required".into()),
            headers: None,
            wallet: None,
            content: Some(body.chars().take(500).collect()),
            tx_hash: None,
        });
        let elapsed = start.elapsed().as_millis();
        return Json(serde_json::json!(PurchaseResponse {
            steps,
            result,
            elapsed_ms: elapsed,
        }));
    }

    // Step 2: parse the 402 response
    let www_auth = resp
        .headers()
        .get("www-authenticate")
        .map(|v| v.to_str().unwrap_or("").to_string());
    let body_bytes = resp.bytes().await.unwrap_or_default();
    let body_json: serde_json::Value =
        serde_json::from_slice(&body_bytes).unwrap_or(serde_json::Value::Null);

    steps.push(Step {
        name: "402".into(),
        detail: Some("Payment Required".into()),
        headers: Some(serde_json::json!({
            "status": 402,
            "www-authenticate": www_auth,
            "body": body_json,
        })),
        wallet: None,
        content: None,
        tx_hash: None,
    });

    if is_mpp {
        return purchase_mpp(url, www_auth, steps, start).await;
    }

    // --- x402 flow ---

    // Step 3: sign payment with the demo wallet
    let private_key_hex = match std::env::var("BUYER_PRIVATE_KEY") {
        Ok(k) => k,
        Err(_) => {
            let elapsed = start.elapsed().as_millis();
            return Json(serde_json::json!({
                "error": "demo wallet not configured",
                "steps": steps,
                "elapsed_ms": elapsed,
            }));
        }
    };

    let signer: PrivateKeySigner = match private_key_hex.parse() {
        Ok(s) => s,
        Err(e) => {
            eprintln!("x402: invalid BUYER_PRIVATE_KEY: {}", e);
            let elapsed = start.elapsed().as_millis();
            return Json(serde_json::json!({
                "error": "wallet configuration error",
                "steps": steps,
                "elapsed_ms": elapsed,
            }));
        }
    };
    let wallet_addr = format!("{:?}", signer.address());
    let signer = Arc::new(signer);

    steps.push(Step {
        name: "sign".into(),
        detail: Some("EIP-712 typed-data signature (x402 V2, ERC-3009)".into()),
        headers: None,
        wallet: Some(wallet_addr),
        content: None,
        tx_hash: None,
    });

    // Step 4: retry with auto-payment via x402-reqwest middleware (V2)
    let x402_client = X402Client::new().register(V2Eip155ExactClient::new(signer));
    let paid_client = reqwest::Client::new()
        .with_payments(x402_client)
        .build();

    let paid_resp = match paid_client.get(&url).send().await {
        Ok(r) => r,
        Err(e) => {
            steps.push(Step {
                name: "error".into(),
                detail: Some(format!("paid request failed: {}", e)),
                headers: None,
                wallet: None,
                content: None,
                tx_hash: None,
            });
            let elapsed = start.elapsed().as_millis();
            return Json(serde_json::json!({
                "steps": steps,
                "result": null,
                "elapsed_ms": elapsed,
            }));
        }
    };

    collect_result(paid_resp, steps, start).await
}

// --- MPP purchase flow ---

async fn purchase_mpp(
    url: String,
    www_auth: Option<String>,
    mut steps: Vec<Step>,
    start: Instant,
) -> Json<serde_json::Value> {
    let challenge_str = www_auth.unwrap_or_default();

    let extract = |key: &str| -> String {
        let pattern = format!("{}=\"", key);
        challenge_str
            .find(&pattern)
            .map(|i| {
                let s = i + pattern.len();
                let e = challenge_str[s..].find('"').map(|j| s + j).unwrap_or(s);
                challenge_str[s..e].to_string()
            })
            .unwrap_or_default()
    };

    let id = extract("id");
    let realm = extract("realm");
    let method = extract("method");
    let intent = extract("intent");
    let request = extract("request");
    let expires = extract("expires");

    // Step 3: send real pathUSD transfer on Tempo
    let private_key = std::env::var("BUYER_PRIVATE_KEY").unwrap_or_default();
    let tempo_rpc = std::env::var("TEMPO_RPC_URL")
        .unwrap_or_else(|_| "https://rpc.moderato.tempo.xyz".into());
    let seller_addr: alloy_primitives::Address = std::env::var("SELLER_ADDRESS")
        .unwrap_or_default()
        .parse()
        .unwrap_or_default();
    let pathusd: alloy_primitives::Address =
        "0x20c0000000000000000000000000000000000000".parse().unwrap();
    let price_usdc = std::env::var("PRICE_USDC").unwrap_or_else(|_| "0.001".into());
    let amount_raw = {
        let parts: Vec<&str> = price_usdc.split('.').collect();
        let whole: u64 = parts[0].parse().unwrap_or(0);
        let frac: u64 = if parts.len() > 1 {
            format!("{:0<6}", parts[1])[..6].parse().unwrap_or(0)
        } else { 0 };
        alloy_primitives::U256::from(whole * 1_000_000 + frac)
    };

    let buyer_signer: alloy_signer_local::PrivateKeySigner = match private_key.parse() {
        Ok(s) => s,
        Err(e) => {
            eprintln!("mpp: invalid BUYER_PRIVATE_KEY: {}", e);
            steps.push(Step {
                name: "error".into(),
                detail: Some("wallet configuration error".into()),
                headers: None, wallet: None, content: None, tx_hash: None,
            });
            let elapsed = start.elapsed().as_millis();
            return Json(serde_json::json!({"steps": steps, "result": null, "elapsed_ms": elapsed}));
        }
    };
    let wallet_addr = format!("{:?}", buyer_signer.address());

    steps.push(Step {
        name: "transfer".into(),
        detail: Some(format!(
            "Sending {} pathUSD on Tempo to {:?}",
            price_usdc, seller_addr
        )),
        headers: None,
        wallet: Some(wallet_addr.clone()),
        content: None,
        tx_hash: None,
    });

    // Send real on-chain transfer
    let tx_hash = match mpp::send_tempo_transfer(
        &tempo_rpc, &private_key, pathusd, seller_addr, amount_raw,
    ).await {
        Ok(h) => h,
        Err(e) => {
            steps.push(Step {
                name: "error".into(),
                detail: Some({
                    eprintln!("mpp: transfer failed: {}", e);
                    "transfer failed".into()
                }),
                headers: None, wallet: None, content: None, tx_hash: None,
            });
            let elapsed = start.elapsed().as_millis();
            return Json(serde_json::json!({"steps": steps, "result": null, "elapsed_ms": elapsed}));
        }
    };

    steps.push(Step {
        name: "settled".into(),
        detail: Some("pathUSD transfer confirmed".into()),
        headers: None,
        wallet: None,
        content: None,
        tx_hash: Some(tx_hash.clone()),
    });

    // Step 4: build credential with REAL tx hash
    let credential = serde_json::json!({
        "challenge": {
            "id": id, "realm": realm, "method": method,
            "intent": intent, "request": request, "expires": expires,
        },
        "source": format!("eip155:{}:{}", 42431, wallet_addr),
        "payload": { "tx": tx_hash }
    });
    let credential_b64 = URL_SAFE_NO_PAD.encode(credential.to_string().as_bytes());

    steps.push(Step {
        name: "sign".into(),
        detail: Some("Authorization: Payment with real tx proof".into()),
        headers: Some(serde_json::json!({ "credential": credential })),
        wallet: Some(wallet_addr),
        content: None,
        tx_hash: None,
    });

    // Step 5: retry with real credential
    let client = reqwest::Client::new();
    let paid_resp = match client
        .get(&url)
        .header("Authorization", format!("Payment {}", credential_b64))
        .send()
        .await
    {
        Ok(r) => r,
        Err(e) => {
            steps.push(Step {
                name: "error".into(),
                detail: Some(format!("paid request failed: {}", e)),
                headers: None, wallet: None, content: None, tx_hash: None,
            });
            let elapsed = start.elapsed().as_millis();
            return Json(serde_json::json!({"steps": steps, "result": null, "elapsed_ms": elapsed}));
        }
    };

    collect_result(paid_resp, steps, start).await
}

// --- Shared result collection ---

async fn collect_result(
    paid_resp: reqwest::Response,
    mut steps: Vec<Step>,
    start: Instant,
) -> Json<serde_json::Value> {
    let paid_status = paid_resp.status().as_u16();

    // Capture payment response/receipt headers and extract tx hash
    let receipt_header = paid_resp
        .headers()
        .get("payment-receipt")
        .or_else(|| paid_resp.headers().get("x-payment-response"))
        .map(|v| v.to_str().unwrap_or("").to_string());

    // Try to extract tx hash from x402 payment response (base64 JSON with "transaction" field)
    let x402_tx_hash = receipt_header.as_ref().and_then(|h| {
        use base64::Engine;
        let decoded = base64::engine::general_purpose::STANDARD.decode(h).ok()
            .or_else(|| base64::engine::general_purpose::URL_SAFE_NO_PAD.decode(h).ok())?;
        let json: serde_json::Value = serde_json::from_slice(&decoded).ok()?;
        json.get("transaction")
            .or_else(|| json.get("tx"))
            .or_else(|| json.get("txHash"))
            .and_then(|v| v.as_str())
            .map(|s| s.to_string())
    });

    steps.push(Step {
        name: "retry".into(),
        detail: Some(format!("status {}", paid_status)),
        headers: receipt_header.map(|h| serde_json::json!({ "receipt": h })),
        wallet: None,
        content: None,
        tx_hash: x402_tx_hash,
    });

    let content_type: String = paid_resp
        .headers()
        .get("content-type")
        .map(|v| v.to_str().unwrap_or("").to_string())
        .unwrap_or_default();

    let result_body: String = paid_resp.text().await.unwrap_or_default();

    if paid_status != 200 {
        steps.push(Step {
            name: "error".into(),
            detail: Some(format!(
                "Payment failed (HTTP {}). The server rejected the payment — \
                 this usually means the wallet has insufficient funds on this network.",
                paid_status
            )),
            headers: None,
            wallet: None,
            content: None,
            tx_hash: None,
        });
        let elapsed = start.elapsed().as_millis();
        return Json(serde_json::json!({
            "steps": steps,
            "error": format!("payment failed with status {}", paid_status),
            "elapsed_ms": elapsed,
        }));
    }

    let result: serde_json::Value = if content_type.contains("json") {
        serde_json::from_str(&result_body).unwrap_or(serde_json::json!({"raw": result_body}))
    } else {
        serde_json::json!({"raw": result_body})
    };

    steps.push(Step {
        name: "200".into(),
        detail: None,
        headers: None,
        wallet: None,
        content: Some(result_body.chars().take(500).collect()),
        tx_hash: None,
    });

    let elapsed = start.elapsed().as_millis();
    Json(serde_json::json!(PurchaseResponse {
        steps,
        result,
        elapsed_ms: elapsed,
    }))
}

// ---------------------------------------------------------------------------
// Main
// ---------------------------------------------------------------------------

fn x402_routes<F: x402_types::facilitator::Facilitator + Clone + Send + Sync + 'static>(
    prefix: &str,
    x402: &X402Middleware<F>,
    price_tag: x402_types::proto::v2::PriceTag,
) -> Router {
    Router::new()
        .route(
            &format!("{}/haiku", prefix),
            get(haiku_handler).layer(
                x402.with_price_tag(price_tag.clone())
                    .with_description("A random haiku about technology".into()),
            ),
        )
        .route(
            &format!("{}/quote", prefix),
            get(quote_handler).layer(
                x402.with_price_tag(price_tag.clone())
                    .with_description("A curated programming quote".into()),
            ),
        )
        .route(
            &format!("{}/fact", prefix),
            get(fact_handler).layer(
                x402.with_price_tag(price_tag.clone())
                    .with_description("A random technical fact".into()),
            ),
        )
        .route(
            &format!("{}/torus", prefix),
            get(torus_handler).layer(
                x402.with_price_tag(price_tag)
                    .with_description("A Torus logographic symbol".into()),
            ),
        )
}

#[tokio::main]
async fn main() {
    let seller_address_str = std::env::var("SELLER_ADDRESS")
        .expect("SELLER_ADDRESS must be set");
    let base_url = std::env::var("BASE_URL")
        .unwrap_or_else(|_| "https://agora.steven-geller.com".into());
    let facilitator_url = std::env::var("FACILITATOR_URL")
        .unwrap_or_else(|_| "https://x402.org/facilitator".into());
    let price_usdc = std::env::var("PRICE_USDC")
        .unwrap_or_else(|_| "0.001".into());
    let mpp_secret = std::env::var("MPP_SECRET")
        .unwrap_or_else(|_| "agora-mpp-change-this-in-production".into());

    if let Ok(key) = std::env::var("BUYER_PRIVATE_KEY") {
        if let Ok(signer) = key.parse::<PrivateKeySigner>() {
            eprintln!("buyer wallet: {:?}", signer.address());
        }
    }

    let seller_address: alloy_primitives::Address = seller_address_str
        .parse()
        .expect("invalid SELLER_ADDRESS");

    let x402 = X402Middleware::new(&facilitator_url)
        .with_base_url(base_url.parse().unwrap());

    // --- Testnet: /test/* (Base Sepolia, eip155:84532) ---
    let usdc_testnet = USDC::base_sepolia();
    let price_testnet = usdc_testnet.parse(price_usdc.as_str()).expect("invalid PRICE_USDC");
    let tag_testnet = V2Eip155Exact::price_tag(seller_address, price_testnet);
    let testnet_routes = x402_routes("/test", &x402, tag_testnet);

    // --- Mainnet: /api/* (Base, eip155:8453) ---
    let usdc_mainnet = USDC::base();
    let price_mainnet = usdc_mainnet.parse(price_usdc.as_str()).expect("invalid PRICE_USDC");
    let tag_mainnet = V2Eip155Exact::price_tag(seller_address, price_mainnet);
    let mainnet_routes = x402_routes("/api", &x402, tag_mainnet);

    // --- MPP: /mpp/* (Tempo chain) ---
    let tempo_rpc = std::env::var("TEMPO_RPC_URL")
        .unwrap_or_else(|_| "https://rpc.moderato.tempo.xyz".into());
    let pathusd: alloy_primitives::Address =
        "0x20c0000000000000000000000000000000000000".parse().unwrap();

    // Parse price into token base units (6 decimals for TIP-20)
    let mpp_amount_raw = {
        let parts: Vec<&str> = price_usdc.split('.').collect();
        let whole: u64 = parts[0].parse().unwrap_or(0);
        let frac: u64 = if parts.len() > 1 {
            let f = parts[1];
            let padded = format!("{:0<6}", f);
            padded[..6].parse().unwrap_or(0)
        } else {
            0
        };
        alloy_primitives::U256::from(whole * 1_000_000 + frac)
    };

    let mpp_config = mpp::MppConfig {
        realm: "agora.steven-geller.com".into(),
        method: "tempo".into(),
        amount: price_usdc.clone(),
        amount_raw: mpp_amount_raw,
        currency: "pathUSD".into(),
        recipient: seller_address,
        token_address: pathusd,
        rpc_url: tempo_rpc.clone(),
        description: "Agora API call".into(),
        secret: mpp_secret.as_bytes().to_vec(),
        consumed_hashes: std::sync::Arc::new(std::sync::Mutex::new(std::collections::HashSet::new())),
    };
    let mpp_routes = Router::new()
        .route("/mpp/haiku", get(haiku_handler))
        .route("/mpp/quote", get(quote_handler))
        .route("/mpp/fact", get(fact_handler))
        .route("/mpp/torus", get(torus_handler))
        .route_layer(axum_mw::from_fn_with_state(mpp_config.clone(), mpp::mpp_middleware))
        .with_state(mpp_config);

    // --- Discovery ---
    let disc_price = price_usdc.clone();
    let disc_seller = seller_address_str.clone();
    let disc_facilitator = facilitator_url.clone();
    let discovery = Router::new().route(
        "/.well-known/x402",
        get(move || {
            let p = disc_price.clone();
            let s = disc_seller.clone();
            let f = disc_facilitator.clone();
            async move {
                Json(serde_json::json!({
                    "x402Version": 2,
                    "price": format!("{} USDC", p),
                    "seller": s,
                    "facilitator": f,
                    "networks": {
                        "testnet": { "chain": "eip155:84532", "name": "Base Sepolia", "prefix": "/test" },
                        "mainnet": { "chain": "eip155:8453", "name": "Base", "prefix": "/api" },
                    },
                    "endpoints": ["haiku", "quote", "fact", "torus"],
                    "mpp": { "prefix": "/mpp", "realm": "agora.steven-geller.com", "method": "tempo" },
                    "docs": "/agents.txt"
                }))
            }
        }),
    );

    let app = Router::new()
        .merge(testnet_routes)
        .merge(mainnet_routes)
        .merge(mpp_routes)
        .merge(discovery)
        .route("/demo/purchase", post(purchase_handler))
        .route("/demo/balance", post(balance_handler))
        .fallback_service(ServeDir::new("static"));

    let addr: SocketAddr = "127.0.0.1:3033".parse().unwrap();
    eprintln!("agora listening on {}", addr);
    eprintln!("x402 v2 testnet: /test/* (eip155:84532 Base Sepolia)");
    eprintln!("x402 v2 mainnet: /api/*  (eip155:8453 Base)");
    eprintln!("mpp:             /mpp/*  (tempo chain, pathUSD, rpc={})", tempo_rpc);
    eprintln!("seller: {} | price: {} USDC", seller_address, price_usdc);
    eprintln!("base url: {}", base_url);

    let listener = tokio::net::TcpListener::bind(addr).await.unwrap();
    axum::serve(listener, app).await.unwrap();
}
