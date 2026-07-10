use soroban_sdk::{contracttype, Bytes, BytesN, Symbol};

/// Canonical metadata for a carbon project.
#[derive(Clone, Debug, PartialEq, Eq)]
#[contracttype]
pub struct Project {
    /// Unique 32-byte project identifier.
    pub id: BytesN<32>,
    /// Methodology code, e.g. "VM0007", "ARR", "BIOCHAR".
    pub methodology: Symbol,
    /// ISO country / region code, e.g. "BRA", "KEN".
    pub geography: Symbol,
    /// Optional cross-reference to an external registry (Verra, Gold Standard).
    pub external_registry_ref: Option<Bytes>,
    /// Version of the verifying key used for this project's proofs.
    pub verifying_key_version: u32,
}

/// Per-year issuance and retirement totals for a project.
#[derive(Clone, Debug, PartialEq, Eq)]
#[contracttype]
pub struct Vintage {
    /// The project this vintage belongs to.
    pub project_id: BytesN<32>,
    /// Vintage year (e.g. 2025).
    pub year: u32,
    /// Cumulative tokens issued for this vintage.
    pub total_issued: i128,
    /// Cumulative tokens retired for this vintage.
    pub total_retired: i128,
}

/// Storage keys used by the registry contract.
#[derive(Clone)]
#[contracttype]
pub enum DataKey {
    /// Registry initialisation flag — stores the credit-token address.
    CreditToken,
    /// Address of the zk-verifier contract.
    ZkVerifier,
    /// A registered project, keyed by project id.
    Project(BytesN<32>),
    /// A vintage record, keyed by (project_id, year).
    Vintage(BytesN<32>, u32),
    /// Multi-sig governance address (used by update_verifying_key).
    GovernanceKey,
}
