# Agora — Trade-offs & Future Improvements

Decisions made for v1 with notes on what to improve later.

## Protocol Coverage

- **x402 only (v1)**: Real x402 v1 flow via x402-axum middleware on Base Sepolia. Payment signing and settlement require a funded testnet wallet.
  - **v2**: Fund the demo wallet with Base Sepolia USDC (Circle faucet) so payments actually settle.

- **MPP not yet implemented**: mpp-rs SDK is 3 days old (released 2026-03-19), Stripe MPP requires contacting machine-payments@stripe.com for access.
  - **v2**: Add MPP as a second payment rail on the same endpoints. Same content, toggle between x402 and MPP to compare the 402 flows side by side. Use mpp-rs when it stabilizes, or implement manually from the spec at mpp.dev.

## Payment Flow

- **Demo wallet uses Hardhat default key**: The server-side buyer proxy uses a well-known testnet private key. Payments fail at facilitator verification because the wallet has no USDC.
  - **v2**: Set DEMO_PRIVATE_KEY and SELLER_ADDRESS env vars with a funded testnet wallet.

- **Server acts as both buyer and seller**: The demo purchase endpoint calls its own x402-gated endpoints. This demonstrates the flow but doesn't show a true client-server separation.
  - **v2**: Add browser-side wallet support (MetaMask / viem) so visitors can pay from their own wallet.

## Content

- **Torus endpoint calls local API**: Requires torus.service to be running. No graceful fallback if Torus is down.
  - **v2**: Cache a few Torus symbols, return cached on failure.

- **Hardcoded content**: 12 haikus, 12 quotes, 13 facts. No external data sources.
  - **v2**: Could add dynamic content (LLM-generated haikus, live data feeds).

## Infrastructure

- **resource URL shows localhost**: The x402 402 response includes `resource: "http://127.0.0.1:3033/..."` because with_base_url is set to localhost.
  - **v2**: Set base URL to https://agora.steven-geller.com when serving publicly.

- **Torus endpoint not x402-gated**: /api/torus is unprotected (no price tag) because the Torus proxy adds complexity.
  - **v2**: Gate it like the other endpoints.
