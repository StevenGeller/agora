# Security

## Reporting Vulnerabilities

If you discover a security vulnerability, please report it privately via email to **hello@steven-geller.com**. Do not open a public issue.

I will acknowledge receipt within 48 hours and aim to release a fix within 7 days of confirmation.

## Environment Variables

Agora requires several secrets that must never be committed to version control:

| Variable | Purpose | Risk if leaked |
|----------|---------|----------------|
| `BUYER_PRIVATE_KEY` | Demo wallet private key (EVM) | Funds can be drained from the wallet |
| `SELLER_ADDRESS` | Seller wallet address | Low (public on-chain) |
| `MPP_SECRET` | HMAC key for MPP challenge IDs | Forged payment challenges |
| `FACILITATOR_URL` | x402 facilitator endpoint | Redirect payments to attacker facilitator |
| `PRICE_USDC` | Price per API call | Low (visible in 402 responses) |

### Setup

Copy the example and fill in your values:

```bash
cp .env.example .env
chmod 600 .env
```

For systemd deployments, set variables in the service unit file rather than `.env`.

## Security Measures

### Payment Verification

- **x402**: Payment signatures are verified by the x402.org facilitator before content is served. The server never handles raw payment signing for end users. The facilitator checks ERC-3009 `TransferWithAuthorization` signatures and settles on-chain.
- **MPP**: The server verifies payments directly on-chain via Tempo RPC. It calls `eth_getTransactionReceipt`, parses receipt logs for a Transfer event matching the correct token contract, recipient address, and amount. No external facilitator is involved. Challenge IDs use HMAC-SHA256 with constant-time verification (`hmac.verify_slice`) to prevent timing attacks.

### Replay Protection

- **MPP**: Tracks consumed transaction hashes in an in-memory `HashSet`. Each tx hash can only be used once. If the server restarts, the set resets (acceptable for testnet, should be persisted for mainnet).
- **x402**: Replay protection is handled by the facilitator and the ERC-3009 nonce mechanism.

### Rate Limiting

- Demo purchase endpoint: 20 requests per 60 seconds (sliding window)
- Balance endpoint: 60 requests per 60 seconds (sliding window)
- Additional per-IP rate limiting via Caddy reverse proxy

### Input Handling

- SVG responses from the Torus endpoint are sanitized client-side (allowlisted elements, event handlers stripped)
- All user-facing text is escaped before DOM insertion (`textContent` based escaping)
- No user input is interpolated into SQL, shell commands, or server-side templates
- Internal error details (RPC URLs, private key parse errors) are logged to stderr but never returned to clients

## Testnet vs Mainnet

| | Testnet | Mainnet |
|---|---|---|
| **x402** | `/test/*` on Base Sepolia (eip155:84532), no real value | `/api/*` on Base (eip155:8453), real USDC |
| **MPP** | `/mpp/*` on Tempo Moderato (chain 42431), no real value | `/mpp-mainnet/*` on Tempo (chain 4217), real pathUSD |

Use testnet for development and demos. Mainnet endpoints require funded wallets with real tokens.
