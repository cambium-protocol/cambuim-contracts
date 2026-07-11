# Deployments

Active deployments of Cambium Protocol contracts.

## Testnet

> Last updated: Day 3 — pending deployment via `scripts/deploy.sh testnet`

| Contract | Address | Deployed |
|---|---|---|
| credit-token | _pending_ | — |
| zk-verifier | _pending_ | — |
| registry | _pending_ | — |
| marketplace | _pending_ | — |
| retirement | _pending_ | — |

### How to deploy

```bash
# 1. Fund deployer
stellar keys add test   # if not already created
curl "https://friendbot.stellar.org/?addr=$(stellar keys address test)"

# 2. Deploy all contracts
./scripts/deploy.sh testnet

# 3. Verify deployment
./scripts/verify-deployment.sh testnet
```

### Addresses file

After deployment, contract addresses are written to `deployed-addresses.testnet.json`.
This file is consumed by `sdk-js` integration tests and `oracle-node` for cross-contract calls.

**Do not commit this file to the repository** — it contains deployment-specific state.
The README will be updated with stable addresses on Day 5.

## Local Sandbox

```bash
./scripts/deploy.sh local
./scripts/verify-deployment.sh local
```

## Mainnet

> **Not yet deployed.** Requires completed independent security audit (see SECURITY.md)
> and multi-sig-controlled deployer keys. Do not deploy unaudited contracts to mainnet.
