# Deployments

Active deployments of Cambium Protocol contracts.

## Testnet

> Last updated: 2026-07-14 — canonical deployment via `scripts/deploy.sh testnet`

| Contract | Address | Explorer | Deployed |
|---|---|---|---|
| credit-token | `CBRBMYB6UTJEMMSBQQPYHAIO5QWJAT4EBPIFTEEB6MRY6ZZD5NS5KY36` | [↗](https://stellar.expert/explorer/testnet/contract/CBRBMYB6UTJEMMSBQQPYHAIO5QWJAT4EBPIFTEEB6MRY6ZZD5NS5KY36) | 2026-07-14 |
| zk-verifier  | `CDHHVK26VAEP4APPELQLJQLZUKMCDSXGBWT7K6V7L7T6CHHRDY2MUAD7` | [↗](https://stellar.expert/explorer/testnet/contract/CDHHVK26VAEP4APPELQLJQLZUKMCDSXGBWT7K6V7L7T6CHHRDY2MUAD7) | 2026-07-14 |
| registry     | `CBSLLVCIZBXKPHY73PN5DVHQKNGK4FAZBXMQLKZCJABABUX5OQGPHC43` | [↗](https://stellar.expert/explorer/testnet/contract/CBSLLVCIZBXKPHY73PN5DVHQKNGK4FAZBXMQLKZCJABABUX5OQGPHC43) | 2026-07-14 |
| marketplace  | `CAKXZQTCVDSGVF2BU5FY636O4TDCAX5UJCWYGQKDKMOA5QNBDKPXZ5S7` | [↗](https://stellar.expert/explorer/testnet/contract/CAKXZQTCVDSGVF2BU5FY636O4TDCAX5UJCWYGQKDKMOA5QNBDKPXZ5S7) | 2026-07-14 |
| retirement   | `CDIHLUARSMSYU27QRKXBWVK5HXIJRUAQ3SYQYCK3MZ2UKMCRB275H3G5` | [↗](https://stellar.expert/explorer/testnet/contract/CDIHLUARSMSYU27QRKXBWVK5HXIJRUAQ3SYQYCK3MZ2UKMCRB275H3G5) | 2026-07-14 |

These addresses are also recorded in the README's [Deploying](./README.md#deploying) section and
in `deployed-addresses.testnet.json` (written by `scripts/deploy.sh`).

### How to redeploy

```bash
# 1. Generate and fund deployer identity (once per machine)
stellar keys generate test
stellar keys fund test --network testnet

# 2. Deploy all contracts in dependency order, wire them, write addresses file
./scripts/deploy.sh testnet

# 3. Verify deployment
./scripts/verify-deployment.sh testnet
```

After a redeploy, update the address table above and the one in `README.md` to reflect
the new canonical addresses, then notify downstream consumers (`sdk-js`, `oracle-node`, `web-app`).

### Addresses file

After deployment, contract addresses are written to `deployed-addresses.testnet.json`.
This file is consumed by `sdk-js` integration tests and `oracle-node` for cross-contract calls.
It is **not** committed to the repository — the README and this file are the source of truth
for stable addresses.

## Local Sandbox

```bash
./scripts/deploy.sh local
./scripts/verify-deployment.sh local
```

## Mainnet

> **Not yet deployed.** Requires completed independent security audit (see [SECURITY.md](./SECURITY.md))
> and multi-sig-controlled deployer keys. Do not deploy unaudited contracts to mainnet.
