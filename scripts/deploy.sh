#!/usr/bin/env bash
#
# deploy.sh — Deploy all Cambium contracts to a local Stellar sandbox.
#
# Usage:
#   ./scripts/deploy.sh [network]
#
# If no network is specified, defaults to "local" (standalone sandbox).
# Writes deployed contract IDs to deployed-addresses.<network>.json.
#
# Dependency order:
#   1. credit-token (no deps)
#   2. zk-verifier (no deps)
#   3. registry (depends on credit-token, zk-verifier)
#   4. marketplace (standalone, but references credit-token for pools)
#   5. retirement (depends on credit-token, registry)

set -euo pipefail

NETWORK="${1:-local}"
OUTPUT_FILE="deployed-addresses.${NETWORK}.json"
SOURCE="${STELLAR_SOURCE:-test}"

echo "=== Cambium Protocol — Contract Deployment ==="
echo "Network: ${NETWORK}"
echo "Output:  ${OUTPUT_FILE}"
echo ""

# --- 1. Build all contracts to WASM ---
echo "Building contracts..."
cargo build --release --target wasm32-unknown-unknown 2>&1 | tail -5
echo ""

WASM_DIR="target/wasm32-unknown-unknown/release"

# Check all WASM files exist
for contract in credit_token zk_verifier registry marketplace retirement; do
    WASM_FILE="${WASM_DIR}/${contract}.wasm"
    if [ ! -f "${WASM_FILE}" ]; then
        echo "ERROR: Missing WASM file: ${WASM_FILE}"
        exit 1
    fi
done
echo "All WASM files built successfully."
echo ""

# Helper function to deploy a contract and capture its address
deploy_contract() {
    local name="$1"
    local wasm_file="${WASM_DIR}/${name}.wasm"
    echo "Deploying ${name}..."
    local address
    address=$(stellar contract deploy \
        --wasm "${wasm_file}" \
        --source "${SOURCE}" \
        --network "${NETWORK}" \
        --force)
    echo "  ${name}: ${address}"
    echo "${address}"
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

# Initialize credit-token with registry as admin
echo "Initializing credit-token (admin: ${REGISTRY_ADDR})..."
stellar contract invoke \
    --id "${CREDIT_TOKEN_ADDR}" \
    --fn initialize \
    --admin "${REGISTRY_ADDR}" \
    --network "${NETWORK}" \
    --source "${SOURCE}" \
    --force

# Initialize zk-verifier
echo "Initializing zk-verifier..."
stellar contract invoke \
    --id "${ZK_VERIFIER_ADDR}" \
    --fn initialize \
    --network "${NETWORK}" \
    --source "${SOURCE}" \
    --force

# Initialize registry with credit-token and zk-verifier
echo "Initializing registry (credit_token: ${CREDIT_TOKEN_ADDR}, zk_verifier: ${ZK_VERIFIER_ADDR})..."
stellar contract invoke \
    --id "${REGISTRY_ADDR}" \
    --fn initialize \
    --credit_token "${CREDIT_TOKEN_ADDR}" \
    --zk_verifier "${ZK_VERIFIER_ADDR}" \
    --network "${NETWORK}" \
    --source "${SOURCE}" \
    --force

# Initialize marketplace
echo "Initializing marketplace..."
stellar contract invoke \
    --id "${MARKETPLACE_ADDR}" \
    --fn initialize \
    --network "${NETWORK}" \
    --source "${SOURCE}" \
    --force

# Initialize retirement with credit-token and registry
echo "Initializing retirement (credit_token: ${CREDIT_TOKEN_ADDR}, registry: ${REGISTRY_ADDR})..."
stellar contract invoke \
    --id "${RETIREMENT_ADDR}" \
    --fn initialize \
    --credit_token "${CREDIT_TOKEN_ADDR}" \
    --registry "${REGISTRY_ADDR}" \
    --network "${NETWORK}" \
    --source "${SOURCE}" \
    --force

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
