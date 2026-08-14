use soroban_sdk::{contracttype, Address, Bytes, BytesN, Symbol, Vec};

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

/// Multi-sig + timelock governance configuration for the registry.
///
/// Protects the highest-value attack surface in the system: verifying-key
/// updates. A malicious key update could allow forged proofs to mint
/// uncapped credits, so updates require `threshold` signer approvals and a
/// time delay before taking effect.
#[derive(Clone, Debug, PartialEq, Eq)]
#[contracttype]
pub struct GovernanceConfig {
    /// Minimum number of signer approvals required to execute an update.
    pub threshold: u32,
    /// Addresses authorized to propose and approve updates.
    pub signers: Vec<Address>,
    /// Delay (in ledger-time seconds) between reaching threshold and
    /// execution of an update.
    pub timelock_secs: u64,
}

/// Current canonical verifying key for a methodology.
#[derive(Clone, Debug, PartialEq, Eq)]
#[contracttype]
pub struct VkeyState {
    /// Latest version number (bumped on every executed update).
    pub version: u32,
    /// The verifying key bytes.
    pub key: BytesN<32>,
}

/// What a governance proposal changes when executed.
#[derive(Clone, Debug, PartialEq, Eq)]
#[contracttype]
pub enum ProposalTarget {
    /// Rotate the canonical verifying key for a methodology.
    /// Arguments: (methodology, replacement verifying key).
    Vkey(Symbol, BytesN<32>),
    /// Replace the governance configuration (signer set / threshold /
    /// timelock). Argument: the proposed replacement configuration.
    Governance(GovernanceConfig),
}

/// A pending governance proposal, subject to multi-sig approval and a
/// timelock before execution.
///
/// Replaces the former `VkeyProposal` (verifying-key updates only) with a
/// general proposal that can also change the governance configuration itself,
/// enabling signer rotation through the same approval flow.
#[derive(Clone, Debug, PartialEq, Eq)]
#[contracttype]
pub struct Proposal {
    /// Unique proposal identifier.
    pub id: BytesN<32>,
    /// The change being proposed.
    pub target: ProposalTarget,
    /// Ledger timestamp when the proposal was created.
    pub proposed_at: u64,
    /// Signers who have approved so far.
    pub approvals: Vec<Address>,
    /// Whether the proposal has been executed.
    pub executed: bool,
    /// Whether a signer cancelled the proposal before execution.
    pub cancelled: bool,
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
    /// Multi-sig governance configuration.
    GovernanceConfig,
    /// A pending governance proposal, keyed by proposal id.
    Proposal(BytesN<32>),
    /// Canonical verifying key state per methodology.
    Vkey(Symbol),
    /// The contract authorized to record retirements.
    RetirementContract,
}
