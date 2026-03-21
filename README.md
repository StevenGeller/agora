# Agora

Paid API sampler comparing two protocols fighting to own machine-to-machine payments: **x402** (Coinbase, crypto-native) and **MPP** (Stripe/IETF, fiat-native).

Live at [agora.steven-geller.com](https://agora.steven-geller.com)

![Agora](static/og-image.png)

## What This Is

Agora serves four micro-endpoints (haiku, quote, fact, torus glyph) behind HTTP 402 paywalls. Each endpoint is available through both payment protocols, so you can compare the handshake flows side by side.

The web UI walks through every step of each protocol's 402 challenge-response cycle, from initial request through payment signing to content delivery.

### x402 (Coinbase)

Full [x402 v2](https://www.x402.org) implementation using the official `x402-axum` middleware. Payments settle on-chain via ERC-3009 `TransferWithAuthorization` on Base (USDC), verified by the x402.org facilitator.

- Testnet: `/test/*` (Base Sepolia, eip155:84532)
- Mainnet: `/api/*` (Base, eip155:8453)

### MPP (Stripe / IETF)

Implementation of the [Machine Payments Protocol](https://mpp.dev) per IETF [draft-ryan-httpauth-payment](https://datatracker.ietf.org/doc/draft-ryan-httpauth-payment/). Payments settle on Tempo L1 using pathUSD, with on-chain receipt verification.

- Endpoints: `/mpp/*`
- Settlement: Tempo Moderato chain (eip155:42431)

## Architecture

```
                         ┌─────────────────────────────┐
                         │     Caddy reverse proxy      │
                         │     agora.steven-geller.com  │
                         └──────────┬──────────────────┘
                                    │
                         ┌──────────▼──────────────────┐
                         │     Axum server (:3033)      │
                         │                              │
                         │  /test/*  x402 testnet       │
                         │  /api/*   x402 mainnet       │
                         │  /mpp/*   MPP (Tempo)        │
                         │  /demo/*  buyer proxy        │
                         │  /.well-known/x402 discovery │
                         └──────────┬──────────────────┘
                                    │
              ┌─────────────────────┼─────────────────────┐
              │                     │                     │
    ┌─────────▼────────┐ ┌─────────▼────────┐ ┌──────────▼───────┐
    │ x402.org         │ │ Base Sepolia /   │ │ Tempo Moderato   │
    │ facilitator      │ │ Base mainnet     │ │ RPC              │
    └──────────────────┘ └──────────────────┘ └──────────────────┘
```

**`src/main.rs`** — Axum server, x402 middleware integration, demo buyer proxy, content endpoints, rate limiting

**`src/mpp.rs`** — MPP protocol: HMAC-bound challenge generation, credential verification, on-chain receipt validation, RLP-encoded Tempo transactions

**`static/`** — Single-page frontend with step-by-step flow visualization

## Setup

### Prerequisites

- Rust 1.75+
- A funded wallet on Base Sepolia (for x402) and/or Tempo Moderato (for MPP)

### Get Testnet Tokens

- USDC on Base Sepolia: [faucet.circle.com](https://faucet.circle.com/) (select Base Sepolia)
- ETH for gas on Base Sepolia: [faucet.quicknode.com/base/sepolia](https://faucet.quicknode.com/base/sepolia)
- pathUSD on Tempo Moderato: [tempo.xyz faucet](https://docs.tempo.xyz)

### Configure

```bash
cp .env.example .env
# Edit .env with your wallet key and seller address
```

### Build and Run

```bash
cargo build --release
source .env && ./target/release/agora
```

The server starts on `127.0.0.1:3033`. For production, put it behind a reverse proxy (Caddy, nginx) with TLS.

### systemd (production)

```ini
[Unit]
Description=Agora paid API sampler
After=network.target

[Service]
Type=simple
WorkingDirectory=/home/user/agora
ExecStart=/home/user/agora/target/release/agora
Environment=SELLER_ADDRESS=0x...
Environment=BUYER_PRIVATE_KEY=0x...
Environment=MPP_SECRET=your-secret-here
Environment=PRICE_USDC=0.001
Restart=on-failure

[Install]
WantedBy=multi-user.target
```

## API Reference

### Discovery

```
GET /.well-known/x402
```

Returns JSON with supported protocols, networks, endpoints, and pricing.

### Paid Endpoints

| Path | Protocol | Content |
|------|----------|---------|
| `GET /test/haiku` | x402 v2 (testnet) | Random tech haiku |
| `GET /test/quote` | x402 v2 (testnet) | Programming quote |
| `GET /test/fact` | x402 v2 (testnet) | Technical fact |
| `GET /test/torus` | x402 v2 (testnet) | Torus logographic symbol |
| `GET /api/haiku` | x402 v2 (mainnet) | Same content, real USDC |
| `GET /mpp/haiku` | MPP (Tempo) | Same content, pathUSD |

All endpoints return JSON. Without payment, they return HTTP 402 with protocol-specific challenge headers.

### Demo Buyer

For testing without your own wallet:

```bash
# x402 testnet
curl -s -X POST https://agora.steven-geller.com/demo/purchase \
  -H 'Content-Type: application/json' \
  -d '{"endpoint":"haiku","protocol":"x402-testnet"}'

# MPP
curl -s -X POST https://agora.steven-geller.com/demo/purchase \
  -H 'Content-Type: application/json' \
  -d '{"endpoint":"haiku","protocol":"mpp"}'
```

Returns the full step-by-step handshake as JSON.

## Agent Integration

See [AGENTS.md](AGENTS.md) for complete integration guides with code examples in Rust, JavaScript, and curl for both protocols.

See [agents.txt](static/agents.txt) (also served at `/agents.txt`) for the machine-readable version.

## Protocol Comparison

| | x402 v2 | MPP |
|---|---|---|
| **Spec** | [x402.org](https://www.x402.org) | [IETF draft-ryan-httpauth-payment](https://datatracker.ietf.org/doc/draft-ryan-httpauth-payment/) |
| **402 header** | `Payment-Required` (base64 JSON) | `WWW-Authenticate: Payment` (RFC 9110 auth params) |
| **Client header** | `Payment-Signature` (base64 payload) | `Authorization: Payment` (base64url credential) |
| **Receipt** | `X-Payment-Response` | `Payment-Receipt` |
| **Settlement** | On-chain ERC-3009 (Base USDC) | On-chain TIP-20 transfer (Tempo pathUSD) |
| **Identity** | Wallet address | DID / wallet address |
| **Rust SDK** | [x402-rs](https://github.com/coinbase/x402) (mature) | [mpp-rs](https://github.com/tempoxyz/mpp-rs) (new) |
| **JS SDK** | [@x402/fetch](https://www.npmjs.com/package/@x402/fetch) (mature) | [mppx](https://www.npmjs.com/package/mppx) (new) |

## Known Limitations

See [TRADEOFFS.md](TRADEOFFS.md) for documented trade-offs and planned improvements.

## License

[MIT](LICENSE)
