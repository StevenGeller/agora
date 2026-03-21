# Agora — Agent Payment Guide

How AI agents (or any HTTP client) can pay for Agora API endpoints.

## Endpoints

| Endpoint | Content | Price |
|----------|---------|-------|
| `GET /api/haiku` | Random tech haiku | 0.000001 USDC |
| `GET /api/quote` | Programming quote | 0.000001 USDC |
| `GET /api/fact` | Technical fact | 0.000001 USDC |
| `GET /api/torus` | Torus logographic symbol | 0.000001 USDC |

Each endpoint is available via two payment protocols:
- **x402 v2** at `/api/*` (Coinbase, Base Sepolia, on-chain USDC)
- **MPP** at `/mpp/*` (IETF Payment auth scheme, Tempo method)

---

## x402 v2 (Coinbase / Base Sepolia)

### Network
- Chain: Base Sepolia (eip155:84532)
- Token: USDC (`0x036CbD53842c5426634e7929541eC2318f3dCF7e`)
- Facilitator: `https://x402.org/facilitator`
- Seller: `0x1ecED38210cA1335f9FD38399e64d2C77C2D7cF3`

### Prerequisites
1. A wallet with Base Sepolia USDC
2. Get testnet USDC: https://faucet.circle.com/ (select Base Sepolia)
3. Get testnet ETH for gas: https://faucet.quicknode.com/base/sepolia

### Flow
```
GET /api/haiku
→ 402 + Payment-Required header (base64-encoded V2 JSON)

Parse payment requirements, sign EIP-712 TransferWithAuthorization

GET /api/haiku
Payment-Signature: <base64-encoded payment payload>
→ 200 + content
```

### Using @x402/fetch (JavaScript)
```javascript
import { wrapFetchWithPaymentFromConfig } from "@x402/fetch";
import { ExactEvmScheme } from "@x402/evm";
import { privateKeyToAccount } from "viem/accounts";

const account = privateKeyToAccount("0xYOUR_PRIVATE_KEY");

const paidFetch = wrapFetchWithPaymentFromConfig(fetch, {
  schemes: [{
    network: "eip155:84532",
    client: new ExactEvmScheme(account),
  }],
});

const resp = await paidFetch("https://agora.steven-geller.com/api/haiku");
const data = await resp.json();
console.log(data.haiku);
```

### Using x402-reqwest (Rust)
```rust
use x402_reqwest::{ReqwestWithPayments, ReqwestWithPaymentsBuild, X402Client};
use x402_chain_eip155::V2Eip155ExactClient;
use alloy_signer_local::PrivateKeySigner;
use std::sync::Arc;

let signer: PrivateKeySigner = "0xYOUR_KEY".parse().unwrap();
let client = X402Client::new().register(V2Eip155ExactClient::new(Arc::new(signer)));
let http = reqwest::Client::new().with_payments(client).build();

let resp = http.get("https://agora.steven-geller.com/api/haiku").send().await?;
let data: serde_json::Value = resp.json().await?;
println!("{}", data["haiku"]);
```

### Using curl (manual)
```bash
# Step 1: Get the 402 challenge
curl -s -D- https://agora.steven-geller.com/api/haiku

# The Payment-Required header contains base64-encoded JSON with payment details.
# An agent must sign the EIP-712 typed data and retry with Payment-Signature header.
```

---

## MPP (Machine Payments Protocol)

### Protocol
- Scheme: IETF `Payment` HTTP authentication (draft-ryan-httpauth-payment)
- Method: `tempo`
- Realm: `agora.steven-geller.com`
- Intent: `charge`

### Flow
```
GET /mpp/haiku
→ 402 + WWW-Authenticate: Payment id="...", realm="...", method="tempo",
    intent="charge", expires="...", request="<base64url>"
→ Body: {"type":"https://paymentauth.org/problems/payment-required","title":"Payment Required","status":402}

Build credential from challenge, sign/prove payment

GET /mpp/haiku
Authorization: Payment <base64url-encoded credential JSON>
→ 200 + Payment-Receipt header + content
```

### Credential format
```json
{
  "challenge": {
    "id": "<from WWW-Authenticate>",
    "realm": "agora.steven-geller.com",
    "method": "tempo",
    "intent": "charge",
    "request": "<from WWW-Authenticate>",
    "expires": "<from WWW-Authenticate>"
  },
  "source": "did:key:z6Mk...",
  "payload": {
    "proof": "<payment proof from Tempo/Stripe>",
    "tx": "<transaction hash>"
  }
}
```

Base64url-encode (no padding) the credential JSON and send as:
```
Authorization: Payment <base64url>
```

### Using curl (manual)
```bash
# Step 1: Get the 402 challenge
CHALLENGE=$(curl -s -D- https://agora.steven-geller.com/mpp/haiku 2>/dev/null \
  | grep 'www-authenticate:')
echo "$CHALLENGE"

# Step 2: Parse challenge params, build credential, base64url-encode it
# (In practice, use mppx SDK or mpp-rs)

# Step 3: Retry with credential
curl -s https://agora.steven-geller.com/mpp/haiku \
  -H "Authorization: Payment <base64url-credential>"
```

### Using mppx (JavaScript)
```javascript
// npm install mppx
import { mppx } from "mppx";

const client = mppx.create({ wallet: yourWallet });
const resp = await client.fetch("https://agora.steven-geller.com/mpp/haiku");
const data = await resp.json();
```

### Using mpp-rs (Rust) — when stable
```rust
// mpp-rs is newly released (2026-03-19). Check https://github.com/tempoxyz/mpp-rs
// for the latest API.
```

---

## Demo purchase endpoint

For testing without a wallet, use the built-in demo buyer:

```bash
# x402 v2
curl -s -X POST https://agora.steven-geller.com/demo/purchase \
  -H 'Content-Type: application/json' \
  -d '{"endpoint":"haiku","protocol":"x402"}'

# MPP
curl -s -X POST https://agora.steven-geller.com/demo/purchase \
  -H 'Content-Type: application/json' \
  -d '{"endpoint":"haiku","protocol":"mpp"}'
```

This returns the full step-by-step flow as JSON, showing every HTTP exchange.

---

## Comparison

| | x402 v2 | MPP |
|---|---|---|
| **Spec** | x402.org | IETF draft-ryan-httpauth-payment |
| **402 header** | `Payment-Required` (base64 JSON) | `WWW-Authenticate: Payment` (auth params) |
| **Client header** | `Payment-Signature` (base64 payload) | `Authorization: Payment` (base64url credential) |
| **Receipt** | `X-Payment-Response` | `Payment-Receipt` |
| **Settlement** | On-chain (Base Sepolia USDC) | Tempo/Stripe (or local verification) |
| **Chain** | EVM (Base, Polygon, Solana) | Tempo L1, Stripe, Visa, Lightning |
| **Identity** | Wallet address | DID (did:key) |
| **Rust SDK** | x402-rs (mature) | mpp-rs (new, 2026-03-19) |
| **JS SDK** | @x402/fetch (mature) | mppx (new) |
