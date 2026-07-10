// TODO(day3): Implement multi-sig + timelock governance for verifying-key updates.
//
// The `update_verifying_key` function is the highest-value attack surface in
// the system (a malicious key update could allow forged proofs to mint
// uncapped credits). It must be gated behind a multi-sig governance address
// with a time-lock delay before the new key becomes active.
//
// Interface (stable — implementation fills in on Day 3):
//   fn update_verifying_key(env: Env, methodology: Symbol, new_key: BytesN<32>)
//       -> Result<(), cambium_shared::Error>;
