# Security Policy

## Scope

This security policy covers the Cambium Protocol Soroban smart contracts:

- `credit-token` — ERC-20-like carbon credit token
- `registry` — Project registration and vintage tracking
- `marketplace` — AMM pool for credit swaps
- `retirement` — Carbon credit retirement with on-chain records
- `zk-verifier` — Zero-knowledge proof verification (currently mock)

## Known Limitations

1. **zk-verifier is a mock** — always returns `true`. Must be replaced with real Groth16/BN254 verification before production.
2. **place_limit_order is deferred** — returns `NotYetImplemented`.
3. **retire shield=true is deferred** — returns `NotYetImplemented`.
4. **No rate limiting** — contracts have no anti-spam measures.
5. **No pause mechanism** — no emergency shutdown capability.

## Reporting

If you discover a security vulnerability, please report it privately:

- **Email:** [to be added before mainnet]
- **Do NOT open a public GitHub issue for security vulnerabilities.**

## Audit Status

| Phase | Status |
|---|---|
| Internal review | In progress |
| External audit | Not started |
| Bug bounty | Not active |

## Pre-mainnet Checklist

Before mainnet deployment, all of the following must be complete:

- [ ] Real ZK verifier (Groth16 + BN254) deployed and tested
- [ ] External security audit by qualified firm
- [ ] Multi-sig admin keys for upgrade authority
- [ ] Emergency pause mechanism implemented
- [ ] Rate limiting or gas budget constraints
- [ ] Upgrade path documented and tested
- [ ] Bug bounty program launched
