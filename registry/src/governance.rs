//! Multi-sig + timelock governance for protocol updates.
//!
//! Verifying-key updates are the highest-value attack surface in the system:
//! a malicious key update could allow forged proofs to mint uncapped credits.
//! Governance therefore requires `threshold` signer approvals followed by a
//! timelock delay before a change takes effect. The same flow also covers
//! updates to the governance configuration itself (signer rotation), so no
//! single signer set can become permanently self-appointed.
//!
//! The contract entry points live in `lib.rs` (`init_governance`,
//! `propose_update`, `approve_update`, `execute_vkey_update`,
//! `execute_governance_update`, `cancel_update`); this module contains the
//! shared helpers they use.

use cambium_shared::Error;
use soroban_sdk::xdr::ToXdr;
use soroban_sdk::{Address, BytesN, Env};

use crate::{DataKey, GovernanceConfig, Proposal};

/// Load the governance configuration, or `Error::NotFound` if uninitialized.
pub(crate) fn config(env: &Env) -> Result<GovernanceConfig, Error> {
    env.storage()
        .instance()
        .get(&DataKey::GovernanceConfig)
        .ok_or(Error::NotFound)
}

/// Whether `addr` is an authorized governance signer.
pub(crate) fn is_signer(config: &GovernanceConfig, addr: &Address) -> bool {
    config.signers.contains(addr)
}

/// Compute the deterministic id of a proposal from its contents.
pub(crate) fn proposal_id(env: &Env, proposal: &Proposal) -> BytesN<32> {
    env.crypto().keccak256(&proposal.clone().to_xdr(env)).into()
}
