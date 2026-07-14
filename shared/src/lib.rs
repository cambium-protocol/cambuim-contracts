#![cfg_attr(not(test), no_std)]

use soroban_sdk::{contracterror, contracttype, Address, Bytes, BytesN};

/// Reference to who performed a retirement.
#[derive(Clone, Debug, PartialEq, Eq)]
#[contracttype]
pub enum RetireeRef {
    /// Public retirement — the retiring address is recorded on-chain.
    Public(Address),
    /// Shielded retirement — only a nullifier hash is recorded.
    Shielded(BytesN<32>),
}

/// Side of a limit order.
#[derive(Clone, Debug, PartialEq, Eq)]
#[contracttype]
pub enum OrderSide {
    /// Buy credit tokens with paired asset.
    Buy,
    /// Sell credit tokens for paired asset.
    Sell,
}

/// A zero-knowledge proof submitted with a mint request.
///
/// This is a placeholder type for Day 1. The real implementation will carry
/// Groth16 proof data and public inputs. The interface (this struct passed
/// into `request_mint`) is designed to remain stable — only the internal
/// verification logic changes when zk-verifier is wired in on Day 3.
#[derive(Clone, Debug, PartialEq, Eq)]
#[contracttype]
pub struct Proof {
    /// Opaque proof bytes. Placeholder accepts any non-empty value.
    pub proof_data: Bytes,
    /// Public inputs to the proof circuit.
    pub public_inputs: soroban_sdk::Vec<BytesN<32>>,
}

/// Errors shared across Cambium contracts.
#[derive(Clone, Debug, Eq, PartialEq, Copy)]
#[contracterror]
pub enum Error {
    /// The caller is not authorized for this operation.
    Unauthorized = 1,
    /// The requested entity was not found.
    NotFound = 2,
    /// An arithmetic overflow occurred.
    Overflow = 3,
    /// The proof provided is invalid or malformed.
    InvalidProof = 4,
    /// The project has already been registered.
    AlreadyRegistered = 5,
    /// The amount specified is non-positive.
    NonPositiveAmount = 6,
    /// This feature is not yet implemented.
    NotYetImplemented = 7,
    /// Insufficient token balance for the requested operation.
    InsufficientBalance = 8,
    /// The pool does not exist.
    PoolNotFound = 9,
    /// The retirement record was not found.
    RetirementNotFound = 10,
}
