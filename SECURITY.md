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

### Setup

Copy the example and fill in your values:

```bash
cp .env.example .env
chmod 600 .env
```

For systemd deployments, set variables in the service unit file rather than `.env`.

## Security Measures

### Payment Verification

- **x402**: Payment signatures are verified by the x402.org facilitator before content is served. The server never handles raw payment signing for end users.
- **MPP**: Challenge IDs use HMAC-SHA256 with constant-time verification to prevent timing attacks. Transaction receipts are verified on-chain via RPC before accepting payment.

### Replay Protection

- MPP tracks consumed transaction hashes in memory. Each `tx` can only be used once.
- x402 replay protection is handled by the facilitator and ERC-3009 nonce mechanism.

### Rate Limiting

- Demo purchase endpoint: 20 requests per 60 seconds (sliding window)
- Balance endpoint: 60 requests per 60 seconds (sliding window)

### Input Handling

- SVG responses from the Torus endpoint are sanitized client-side (allowlisted elements, event handlers stripped)
- All user-facing text is escaped before DOM insertion
- No user input is interpolated into SQL, shell commands, or server-side templates

## Testnet vs Mainnet

The `/test/*` endpoints use Base Sepolia (testnet) USDC with no real monetary value. The `/api/*` endpoints use Base mainnet USDC. Use testnet for development and demos.
