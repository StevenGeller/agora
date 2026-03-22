# Agora — Agent Payment Guide

How AI agents (or any HTTP client) can pay for Agora API endpoints.

## Endpoints

| Endpoint | Content | Price |
|----------|---------|-------|
| `GET /test/haiku` | Random tech haiku | 0.001 USDC (testnet) |
| `GET /test/quote` | Programming quote | 0.001 USDC (testnet) |
| `GET /test/fact` | Technical fact | 0.001 USDC (testnet) |
| `GET /test/torus` | Torus logographic symbol | 0.001 USDC (testnet) |
| `GET /api/haiku` | Random tech haiku | 0.001 USDC (mainnet) |
| `GET /api/quote` | Programming quote | 0.001 USDC (mainnet) |
| `GET /api/fact` | Technical fact | 0.001 USDC (mainnet) |
| `GET /api/torus` | Torus logographic symbol | 0.001 USDC (mainnet) |
| `GET /mpp/haiku` | Random tech haiku | 0.001 pathUSD (testnet) |
| `GET /mpp/quote` | Programming quote | 0.001 pathUSD (testnet) |
| `GET /mpp/fact` | Technical fact | 0.001 pathUSD (testnet) |
| `GET /mpp/torus` | Torus logographic symbol | 0.001 pathUSD (testnet) |
| `GET /mpp-mainnet/*` | Same content | 0.001 pathUSD (mainnet) |

Two payment protocols are available:
- **x402 v2** at `/test/*` and `/api/*` (Coinbase, Base chain, USDC)
- **MPP** at `/mpp/*` and `/mpp-mainnet/*` (IETF Payment auth scheme, Tempo chain, pathUSD)

---

## x402 v2 (Coinbase / Base)

### Networks
- **Testnet**: Base Sepolia (eip155:84532), USDC at `0x036CbD53842c5426634e7929541eC2318f3dCF7e`
- **Mainnet**: Base (eip155:8453), USDC at `0x833589fCD6eDb6E08f4c7C32D4f71b54bdA02913`
- Facilitator: `https://x402.org/facilitator`
- Seller: `0x1ecED38210cA1335f9FD38399e64d2C77C2D7cF3`

### Prerequisites
1. A wallet with Base Sepolia USDC (testnet) or Base USDC (mainnet)
2. Get testnet USDC: https://faucet.circle.com/ (select Base Sepolia)
3. Get testnet ETH for gas: https://faucet.quicknode.com/base/sepolia

### Flow
```
GET /test/haiku
→ 402 + Payment-Required header (base64-encoded V2 JSON)

Parse payment requirements, sign EIP-712 TransferWithAuthorization

GET /test/haiku
Payment-Signature: <base64-encoded payment payload>
→ 200 + X-Payment-Response header + content
```

The `Payment-Required` header is a proprietary x402 header (not standard HTTP auth). It contains a base64-encoded JSON blob with version, accepted schemes, network, recipient, token contract, and timeout. The client signs an EIP-712 typed-data message and the facilitator settles on-chain.

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

const resp = await paidFetch("https://agora.steven-geller.com/test/haiku");
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

let resp = http.get("https://agora.steven-geller.com/test/haiku").send().await?;
let data: serde_json::Value = resp.json().await?;
println!("{}", data["haiku"]);
```

### Using curl (manual)
```bash
# Step 1: Get the 402 challenge
curl -s -D- https://agora.steven-geller.com/test/haiku

# The Payment-Required header contains base64-encoded JSON with payment details.
# An agent must sign the EIP-712 typed data and retry with Payment-Signature header.
```

---

## MPP (Machine Payments Protocol)

### Protocol
- Scheme: IETF `Payment` HTTP authentication ([draft-ryan-httpauth-payment](https://datatracker.ietf.org/doc/draft-ryan-httpauth-payment/))
- Method: `tempo`
- Realm: `agora.steven-geller.com`
- Intent: `charge`

### Networks
- **Testnet**: Tempo Moderato (chain 42431), pathUSD at `0x20c0000000000000000000000000000000000000`
- **Mainnet**: Tempo (chain 4217), same token address

### Flow
```
GET /mpp/haiku
→ 402 + WWW-Authenticate: Payment id="...", realm="...", method="tempo",
    intent="charge", expires="...", description="...", request="<base64url>"
→ Body: {"type":"https://paymentauth.org/problems/payment-required","title":"Payment Required","status":402}

Send real pathUSD transfer on Tempo chain to seller address.
Build credential with challenge params and tx hash.

GET /mpp/haiku
Authorization: Payment <base64url-encoded credential JSON>
→ 200 + Payment-Receipt header + content
```

MPP uses standard HTTP auth semantics (`WWW-Authenticate` / `Authorization`). The challenge ID is HMAC-SHA256 bound to the server, so the server can verify it was issued by this server without any external call. The server verifies the payment by checking the transaction receipt directly on-chain via RPC, no facilitator in the path.

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
  "source": "eip155:42431:<wallet-address>",
  "payload": {
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

# Step 2: Send pathUSD transfer on Tempo to seller address
# Step 3: Parse challenge params, build credential with tx hash, base64url-encode it
# (In practice, use mppx SDK or mpp-rs)

# Step 4: Retry with credential
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

### Using mpp-rs (Rust)
```rust
// mpp-rs is newly released. Check https://github.com/tempoxyz/mpp-rs
// for the latest API.
```

---

## Demo purchase endpoint

For testing without a wallet, use the built-in demo buyer:

```bash
# x402 v2 testnet
curl -s -X POST https://agora.steven-geller.com/demo/purchase \
  -H 'Content-Type: application/json' \
  -d '{"endpoint":"haiku","protocol":"x402-testnet"}'

# x402 v2 mainnet
curl -s -X POST https://agora.steven-geller.com/demo/purchase \
  -H 'Content-Type: application/json' \
  -d '{"endpoint":"haiku","protocol":"x402-mainnet"}'

# MPP testnet
curl -s -X POST https://agora.steven-geller.com/demo/purchase \
  -H 'Content-Type: application/json' \
  -d '{"endpoint":"haiku","protocol":"mpp-testnet"}'

# MPP mainnet
curl -s -X POST https://agora.steven-geller.com/demo/purchase \
  -H 'Content-Type: application/json' \
  -d '{"endpoint":"haiku","protocol":"mpp-mainnet"}'
```

This returns the full step-by-step flow as JSON, showing every HTTP exchange.

---

## Comparison

| | x402 v2 | MPP |
|---|---|---|
| **Spec** | x402.org | IETF draft-ryan-httpauth-payment |
| **402 header** | `Payment-Required` (base64 JSON, proprietary) | `WWW-Authenticate: Payment` (standard HTTP auth) |
| **Client header** | `Payment-Signature` (base64 payload) | `Authorization: Payment` (base64url credential) |
| **Receipt** | `X-Payment-Response` | `Payment-Receipt` |
| **Settlement** | Facilitator settles on-chain (Base USDC) | Client settles directly on-chain (Tempo pathUSD) |
| **Verification** | Delegated to facilitator | Server verifies receipt on-chain via RPC |
| **Challenge binding** | None (facilitator tracks nonces) | HMAC-SHA256 bound to server |
| **Chain** | EVM (Base, Polygon, Solana) | Tempo (EVM-compatible) |
| **Identity** | Wallet address | DID / wallet address |
| **Rust SDK** | x402-rs (mature) | mpp-rs (new) |
| **JS SDK** | @x402/fetch (mature) | mppx (new) |
