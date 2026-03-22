# Agora — Trade-offs & Future Improvements

Decisions made for the current version with notes on what to improve later.

## Protocol Coverage

- **x402 v2**: Full implementation via `x402-axum` middleware on both Base Sepolia (testnet) and Base (mainnet). Payment signing and settlement require a funded wallet with USDC.

- **MPP**: Full implementation of IETF `draft-ryan-httpauth-payment` with Tempo `method`. Server returns `WWW-Authenticate: Payment` challenges, verifies `Authorization: Payment` credentials, and checks transaction receipts directly on Tempo chain via RPC. ~625 lines of manual implementation (RLP encoding, JSON-RPC, ERC-20 calldata, HMAC challenge binding, receipt parsing) since the mpp-rs SDK was too new to depend on.

## Payment Flow

- **Demo wallet acts as both buyer and seller**: The demo purchase endpoint uses a server-side wallet to call its own 402-gated endpoints. The balance barely moves (just gas costs on self-transfers). This demonstrates the protocol handshake but not the economic flow of a real buyer-seller interaction.
  - **Future**: Add browser-side wallet support (MetaMask / viem) so visitors can pay from their own wallet.

## Content

- **Torus endpoint calls local API**: Requires torus.service to be running. Falls back to an "unavailable" SVG if Torus is down.
  - **Future**: Cache a few Torus symbols, return cached on failure.

- **Hardcoded content**: 12 haikus, 12 quotes, 13 facts, 15 Torus words. No external data sources.
  - **Future**: Could add dynamic content (LLM-generated haikus, live data feeds).

## Security

- **Replay protection is in-memory**: Consumed transaction hashes are stored in a `HashSet` in memory. If the server restarts, the hash set resets. On testnet this is acceptable. On mainnet with real value, this should be persisted to disk or a database.

- **Rate limiting is global**: The sliding window rate limiter (20 purchases/min, 60 balance checks/min) is per-server, not per-IP. Behind Cloudflare/Caddy, this is supplemented by Caddy's per-IP rate limiting on the reverse proxy.

## MPP-Specific

- **Tempo gas costs**: A pathUSD transfer costs ~300,000 gas on Tempo, roughly 3x what the same ERC-20 transfer costs on Ethereum L1. On testnet this is free. On mainnet the ratio matters at volume.

- **No session-based authorization**: The current implementation does per-request challenge-response. The MPP spec architecture supports session-based authorization (spending limits per session), which would reduce latency for high-frequency agents. Not implemented since the SDKs are too new.

- **Single payment method**: Only the `tempo` method is implemented. The MPP spec is pluggable (Stripe, Visa, Lightning are other methods), but each method requires its own verification implementation.
