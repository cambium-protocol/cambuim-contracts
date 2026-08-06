# Security Policy

## Scope

This security policy covers the Cambium Protocol Soroban smart contracts:

- `credit-token` — ERC-20-like carbon credit token
- `registry` — Project registration and vintage tracking
- `marketplace` — AMM pool and limit order book for credit trading
- `retirement` — Carbon credit retirement with on-chain records
- `zk-verifier` — Zero-knowledge proof verification (currently mock)

## Known Limitations

1. **zk-verifier is a mock** — always returns `true`. Must be replaced with real Groth16/BN254 verification before production.
2. **Shielded retirement uses a plaintext nullifier** — the `shield=true` path records only a nullifier and omits the caller from events, but the nullifier itself is currently provided by the caller and checked against a committed set; it does not yet verify a ZK membership proof from `zk-circuits`.
3. **No rate limiting** — contracts have no anti-spam measures.
4. **No pause mechanism** — no emergency shutdown capability.
5. **Allowlist is not a substitute for KYC** — `credit-token`'s allowlist gates transfers, but on-chain addresses are pseudonymous; compliance depends on off-chain identity verification before `set_allowlisted` is called.

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
- [ ] Emergency pause mechanism implemented
- [ ] Rate limiting or gas budget constraints
- [ ] Upgrade path documented and tested
- [ ] Bug bounty program launched
- [ ] Multi-sig signers held by independent, geographically distributed parties
