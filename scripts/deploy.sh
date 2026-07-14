#!/usr/bin/env bash
#
# deploy.sh — Deploy all Cambium contracts to a Stellar network.
#
# Usage:
#   ./scripts/deploy.sh [network]
#
# Supported networks: local, testnet, futurenet, mainnet
# Defaults to "local" (standalone sandbox).
#
# Writes deployed contract IDs to deployed-addresses.<network>.json, which
# sdk-js and oracle-node both consume.
#
# For testnet/futurenet: automatically funds the deployer via Friendbot if
# the account does not yet exist.
#
# Dependency order:
#   1. credit-token (no deps)
#   2. zk-verifier  (no deps)
#   3. registry     (depends on credit-token, zk-verifier)
#   4. marketplace  (standalone, but references credit-token for pools)
#   5. retirement   (depends on credit-token, registry)
#
# Requires: stellar-cli >=27, Rust + wasm32v1-none target
# Tested against stellar-cli 27.0.0

set -euo pipefail

NETWORK="${1:-local}"
OUTPUT_FILE="deployed-addresses.${NETWORK}.json"
SOURCE="${STELLAR_SOURCE:-test}"

# --- Network validation ---
case "${NETWORK}" in
    local)
        RPC_URL="${STELLAR_RPC_URL:-http://localhost:8000}"
        PASSPHRASE="Standalone Network ; February 2017"
        ;;
    testnet)
        RPC_URL="${STELLAR_RPC_URL:-https://soroban-testnet.stellar.org}"
        PASSPHRASE="Test SDF Network ; September 2015"
        ;;
    futurenet)
        RPC_URL="${STELLAR_RPC_URL:-https://soroban-futurenet.stellar.org}"
        PASSPHRASE="Test SDF Future Network ; October 2022"
        ;;
    mainnet)
        RPC_URL="${STELLAR_RPC_URL:-https://soroban-mainnet.stellar.org}"
        PASSPHRASE="Public Global Stellar Network ; September 2015"
        echo "WARNING: Mainnet deployment requires a completed security audit."
        echo "See SECURITY.md before proceeding."
        read -rp "Type 'yes-audited' to confirm: " CONFIRM
        if [ "${CONFIRM}" != "yes-audited" ]; then
            echo "Aborted."
            exit 1
        fi
        ;;
    *)
        echo "ERROR: Unknown network '${NETWORK}'. Supported: local, testnet, futurenet, mainnet"
        exit 1
        ;;
esac

echo "=== Cambium Protocol — Contract Deployment ==="
echo "Network: ${NETWORK}"
echo "RPC:     ${RPC_URL}"
echo "Output:  ${OUTPUT_FILE}"
echo ""

# --- Fund deployer via Friendbot (testnet/futurenet only) ---
fund_deployer() {
    if [ "${NETWORK}" = "testnet" ] || [ "${NETWORK}" = "futurenet" ]; then
        # stellar keys fund handles funding via Friendbot and is idempotent —
        # it does nothing if the account already has a balance.
        echo "Ensuring deployer '${SOURCE}' is funded on ${NETWORK}..."
        stellar keys fund "${SOURCE}" --network "${NETWORK}" 2>&1 || {
            echo "WARNING: Friendbot funding returned non-zero exit. Account may already be funded."
        }
        echo ""
    fi
}

fund_deployer

# --- 1. Build all contracts to WASM ---
# Use `stellar contract build` which targets wasm32v1-none and applies the
# correct Soroban-specific compilation flags required by the protocol runtime.
# Do NOT use `cargo build --target wasm32-unknown-unknown` — it produces WASM
# that uses the reference-types proposal which the Soroban VM rejects.
#
# Note: stellar contract build does not accept --package multiple times,
# so each package is built in sequence.
echo "Building contracts..."
for pkg in cambium-credit-token cambium-zk-verifier cambium-registry cambium-marketplace cambium-retirement; do
    stellar contract build --package "${pkg}" --optimize=false 2>&1 | grep -E "(✅|❌|error)" | head -3
done
echo ""

# stellar contract build writes artifacts to target/wasm32v1-none/release/
# Cargo names WASM artifacts after the crate name (with cambium_ prefix).
WASM_DIR="target/wasm32v1-none/release"

declare -A WASM_FILES
WASM_FILES[credit_token]="${WASM_DIR}/cambium_credit_token.wasm"
WASM_FILES[zk_verifier]="${WASM_DIR}/cambium_zk_verifier.wasm"
WASM_FILES[registry]="${WASM_DIR}/cambium_registry.wasm"
WASM_FILES[marketplace]="${WASM_DIR}/cambium_marketplace.wasm"
WASM_FILES[retirement]="${WASM_DIR}/cambium_retirement.wasm"

# Check all WASM files exist
for contract in credit_token zk_verifier registry marketplace retirement; do
    WASM_FILE="${WASM_FILES[$contract]}"
    if [ ! -f "${WASM_FILE}" ]; then
        echo "ERROR: Missing WASM file: ${WASM_FILE}"
        echo "Expected contracts to be compiled with 'stellar contract build'."
        exit 1
    fi
done
echo "All WASM files built successfully."
echo ""

# Helper function to deploy a contract and capture its address.
# stellar contract deploy (v27): outputs the contract ID on stdout.
# All diagnostic output goes to stderr so the calling $(...) only
# captures the contract address.
deploy_contract() {
    local name="$1"
    local wasm_file="${WASM_FILES[$name]}"
    echo "  Deploying ${name}..." >&2
    local address
    address=$(stellar contract deploy \
        --wasm "${wasm_file}" \
        --source "${SOURCE}" \
        --network "${NETWORK}" \
        2>/dev/null)
    echo "  ${name}: ${address}" >&2
    printf '%s' "${address}"
}

# --- 2. Deploy contracts in dependency order ---

echo "=== Deploying contracts ==="

# 2a. Deploy credit-token (no dependencies)
CREDIT_TOKEN_ADDR=$(deploy_contract "credit_token")

# 2b. Deploy zk-verifier (no dependencies)
ZK_VERIFIER_ADDR=$(deploy_contract "zk_verifier")

# 2c. Deploy registry (depends on credit-token, zk-verifier)
REGISTRY_ADDR=$(deploy_contract "registry")

# 2d. Deploy marketplace (standalone)
MARKETPLACE_ADDR=$(deploy_contract "marketplace")

# 2e. Deploy retirement (depends on credit-token, registry)
RETIREMENT_ADDR=$(deploy_contract "retirement")

echo ""
echo "=== Initializing contracts ==="

# stellar contract invoke (v27): function name and args come after the `--`
# separator, as `-- <function_name> --arg-name value`.

# Initialize credit-token with registry as admin
echo "Initializing credit-token (admin: ${REGISTRY_ADDR})..."
stellar contract invoke \
    --id "${CREDIT_TOKEN_ADDR}" \
    --source "${SOURCE}" \
    --network "${NETWORK}" \
    -- initialize \
    --admin "${REGISTRY_ADDR}"

# Initialize zk-verifier
echo "Initializing zk-verifier..."
stellar contract invoke \
    --id "${ZK_VERIFIER_ADDR}" \
    --source "${SOURCE}" \
    --network "${NETWORK}" \
    -- initialize

# Initialize registry with credit-token and zk-verifier
echo "Initializing registry (credit_token: ${CREDIT_TOKEN_ADDR}, zk_verifier: ${ZK_VERIFIER_ADDR})..."
stellar contract invoke \
    --id "${REGISTRY_ADDR}" \
    --source "${SOURCE}" \
    --network "${NETWORK}" \
    -- initialize \
    --credit_token "${CREDIT_TOKEN_ADDR}" \
    --zk_verifier "${ZK_VERIFIER_ADDR}"

# Initialize marketplace
echo "Initializing marketplace..."
stellar contract invoke \
    --id "${MARKETPLACE_ADDR}" \
    --source "${SOURCE}" \
    --network "${NETWORK}" \
    -- initialize

# Initialize retirement with credit-token and registry
echo "Initializing retirement (credit_token: ${CREDIT_TOKEN_ADDR}, registry: ${REGISTRY_ADDR})..."
stellar contract invoke \
    --id "${RETIREMENT_ADDR}" \
    --source "${SOURCE}" \
    --network "${NETWORK}" \
    -- initialize \
    --credit_token "${CREDIT_TOKEN_ADDR}" \
    --registry "${REGISTRY_ADDR}"

echo ""
echo "=== Writing ${OUTPUT_FILE} ==="

cat > "${OUTPUT_FILE}" <<EOF
{
  "network": "${NETWORK}",
  "deployed_at": "$(date -u +%Y-%m-%dT%H:%M:%SZ)",
  "contracts": {
    "credit_token": "${CREDIT_TOKEN_ADDR}",
    "zk_verifier": "${ZK_VERIFIER_ADDR}",
    "registry": "${REGISTRY_ADDR}",
    "marketplace": "${MARKETPLACE_ADDR}",
    "retirement": "${RETIREMENT_ADDR}"
  }
}
EOF

echo "Deployment complete!"
echo ""
cat "${OUTPUT_FILE}"
