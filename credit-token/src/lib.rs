#![cfg_attr(not(test), no_std)]

#[cfg(any(test, feature = "testutils"))]
extern crate std;

use soroban_sdk::{contract, contracterror, contractimpl, contracttype, Address, Env, Symbol};

#[derive(Clone)]
#[contracttype]
pub enum DataKey {
    Balance(Address),
    Allowance(Address, Address),
    Admin,
    /// Address of the contract authorized to burn tokens (the retirement
    /// contract). Set by the admin via `set_burner`.
    Burner,
    /// Whether compliance allowlisting is enforced on all transfers.
    AllowlistEnabled,
    /// Addresses permitted to hold and transact while the allowlist is on.
    Allowlisted(Address),
}

#[contracterror]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u32)]
pub enum TokenError {
    InsufficientBalance = 100,
    Overflow = 101,
    NegativeAmount = 102,
    Unauthorized = 103,
    AllowanceUnderflow = 104,
}

#[contract]
pub struct CreditTokenContract;

#[contractimpl]
impl CreditTokenContract {
    /// Initialize the token with an admin (registry contract address).
    /// Can only be called once.
    pub fn initialize(env: Env, admin: Address) {
        if env.storage().instance().has(&DataKey::Admin) {
            panic!("already initialized");
        }
        env.storage().instance().set(&DataKey::Admin, &admin);
    }

    /// Return the admin (registry contract) address.
    pub fn admin(env: Env) -> Address {
        env.storage()
            .instance()
            .get(&DataKey::Admin)
            .expect("not initialized")
    }

    /// Return the token balance for `id`.
    pub fn balance(env: Env, id: Address) -> i128 {
        env.storage()
            .persistent()
            .get(&DataKey::Balance(id))
            .unwrap_or(0)
    }

    /// Transfer `amount` tokens from `from` to `to`.
    /// Requires auth from `from`.
    pub fn transfer(env: Env, from: Address, to: Address, amount: i128) -> Result<(), TokenError> {
        if amount <= 0 {
            return Err(TokenError::NegativeAmount);
        }
        from.require_auth();
        Self::enforce_allowlist(&env, &from, &to)?;

        let from_balance = Self::balance(env.clone(), from.clone());
        if from_balance < amount {
            return Err(TokenError::InsufficientBalance);
        }

        env.storage()
            .persistent()
            .set(&DataKey::Balance(from.clone()), &(from_balance - amount));

        let to_balance = Self::balance(env.clone(), to.clone());
        let new_to_balance = to_balance.checked_add(amount).ok_or(TokenError::Overflow)?;
        env.storage()
            .persistent()
            .set(&DataKey::Balance(to.clone()), &new_to_balance);

        env.events()
            .publish((Symbol::new(&env, "transfer"), from.clone(), to), (amount,));

        Ok(())
    }

    /// Transfer `amount` from `from` to `to` using an existing allowance
    /// granted to `spender`. Requires auth from `spender`.
    pub fn transfer_from(
        env: Env,
        spender: Address,
        from: Address,
        to: Address,
        amount: i128,
    ) -> Result<(), TokenError> {
        if amount <= 0 {
            return Err(TokenError::NegativeAmount);
        }
        spender.require_auth();
        Self::enforce_allowlist(&env, &from, &to)?;

        let allowance = Self::allowance(env.clone(), from.clone(), spender.clone());
        if allowance < amount {
            return Err(TokenError::AllowanceUnderflow);
        }

        let from_balance = Self::balance(env.clone(), from.clone());
        if from_balance < amount {
            return Err(TokenError::InsufficientBalance);
        }

        env.storage().persistent().set(
            &DataKey::Allowance(from.clone(), spender),
            &(allowance - amount),
        );

        env.storage()
            .persistent()
            .set(&DataKey::Balance(from.clone()), &(from_balance - amount));

        let to_balance = Self::balance(env.clone(), to.clone());
        let new_to_balance = to_balance.checked_add(amount).ok_or(TokenError::Overflow)?;
        env.storage()
            .persistent()
            .set(&DataKey::Balance(to.clone()), &new_to_balance);

        env.events()
            .publish((Symbol::new(&env, "transfer"), from.clone(), to), (amount,));

        Ok(())
    }

    /// Approve `spender` to transfer up to `amount` tokens from `from`.
    /// Requires auth from `from`.
    pub fn approve(
        env: Env,
        from: Address,
        spender: Address,
        amount: i128,
    ) -> Result<(), TokenError> {
        if amount <= 0 {
            return Err(TokenError::NegativeAmount);
        }
        from.require_auth();

        env.storage()
            .persistent()
            .set(&DataKey::Allowance(from.clone(), spender.clone()), &amount);

        env.events()
            .publish((Symbol::new(&env, "approve"), from, spender), (amount,));

        Ok(())
    }

    /// Return the allowance `spender` has over `owner`'s tokens.
    pub fn allowance(env: Env, owner: Address, spender: Address) -> i128 {
        env.storage()
            .persistent()
            .get(&DataKey::Allowance(owner, spender))
            .unwrap_or(0)
    }

    /// Mint `amount` tokens to `to`.
    ///
    /// # Authorization
    /// Only callable by the admin address set at initialization (i.e. the
    /// registry contract). Any other caller receives `TokenError::Unauthorized`.
    pub fn mint(env: Env, to: Address, amount: i128) -> Result<(), TokenError> {
        if amount <= 0 {
            return Err(TokenError::NegativeAmount);
        }

        let admin: Address = env
            .storage()
            .instance()
            .get(&DataKey::Admin)
            .expect("not initialized");

        // Authorization check: caller must be the registry (admin).
        admin.require_auth();

        // Compliance gate: recipients must be allowlisted when enabled.
        Self::enforce_allowlist(&env, &to, &to)?;

        let current_balance = Self::balance(env.clone(), to.clone());
        let new_balance = current_balance
            .checked_add(amount)
            .ok_or(TokenError::Overflow)?;

        env.storage()
            .persistent()
            .set(&DataKey::Balance(to.clone()), &new_balance);

        env.events()
            .publish((Symbol::new(&env, "mint"), admin, to), (amount,));

        Ok(())
    }

    /// Set the address authorized to burn tokens on behalf of holders.
    ///
    /// This is the retirement contract: it burns credits permanently and is
    /// expected to validate the holder's authorization (`from.require_auth()`)
    /// itself before calling `burn`.
    ///
    /// # Authorization
    /// Only callable by the admin address (the registry contract).
    pub fn set_burner(env: Env, burner: Address) -> Result<(), TokenError> {
        let admin: Address = env
            .storage()
            .instance()
            .get(&DataKey::Admin)
            .expect("not initialized");
        admin.require_auth();

        env.storage().instance().set(&DataKey::Burner, &burner);
        Ok(())
    }

    /// Return the authorized burner contract address, if one has been set.
    pub fn get_burner(env: Env) -> Option<Address> {
        env.storage().instance().get(&DataKey::Burner)
    }

    /// Burn `amount` tokens from `from`.
    ///
    /// # Authorization
    /// If a burner contract has been configured (via `set_burner`), only that
    /// contract may burn — it must have validated `from`'s authorization
    /// itself (see `retirement::retire`). Otherwise, only the admin address
    /// (the registry contract) may burn.
    pub fn burn(env: Env, from: Address, amount: i128) -> Result<(), TokenError> {
        if amount <= 0 {
            return Err(TokenError::NegativeAmount);
        }

        let admin: Address = env
            .storage()
            .instance()
            .get(&DataKey::Admin)
            .expect("not initialized");

        let burner: Option<Address> = env.storage().instance().get(&DataKey::Burner);
        match burner {
            Some(b) => b.require_auth(),
            None => admin.require_auth(),
        }

        // Compliance gate: the holder must be allowlisted when enabled.
        Self::enforce_allowlist(&env, &from, &from)?;

        let from_balance = Self::balance(env.clone(), from.clone());
        if from_balance < amount {
            return Err(TokenError::InsufficientBalance);
        }

        env.storage()
            .persistent()
            .set(&DataKey::Balance(from.clone()), &(from_balance - amount));

        env.events()
            .publish((Symbol::new(&env, "burn"), admin, from), (amount,));

        Ok(())
    }

    /// Enable or disable compliance allowlisting.
    ///
    /// When enabled, all transfers, mints and burns only succeed for addresses
    /// that have been explicitly allowlisted via `set_allowlisted`.
    ///
    /// # Authorization
    /// Only callable by the admin address (the registry contract).
    pub fn enable_allowlist(env: Env, enabled: bool) -> Result<(), TokenError> {
        let admin: Address = env
            .storage()
            .instance()
            .get(&DataKey::Admin)
            .expect("not initialized");
        admin.require_auth();

        env.storage()
            .instance()
            .set(&DataKey::AllowlistEnabled, &enabled);
        Ok(())
    }

    /// Add or remove an address from the compliance allowlist.
    ///
    /// # Authorization
    /// Only callable by the admin address (the registry contract).
    pub fn set_allowlisted(env: Env, address: Address, allowed: bool) -> Result<(), TokenError> {
        let admin: Address = env
            .storage()
            .instance()
            .get(&DataKey::Admin)
            .expect("not initialized");
        admin.require_auth();

        env.storage()
            .persistent()
            .set(&DataKey::Allowlisted(address), &allowed);
        Ok(())
    }

    /// Return whether an address is allowlisted.
    pub fn is_allowlisted(env: Env, address: Address) -> bool {
        env.storage()
            .persistent()
            .get(&DataKey::Allowlisted(address))
            .unwrap_or(false)
    }

    /// Internal gate: reject holders that are not allowlisted whenever the
    /// allowlist is enabled.
    fn enforce_allowlist(env: &Env, from: &Address, to: &Address) -> Result<(), TokenError> {
        let enabled: bool = env
            .storage()
            .instance()
            .get(&DataKey::AllowlistEnabled)
            .unwrap_or(false);
        if !enabled {
            return Ok(());
        }
        if Self::is_allowlisted(env.clone(), from.clone())
            && Self::is_allowlisted(env.clone(), to.clone())
        {
            Ok(())
        } else {
            Err(TokenError::Unauthorized)
        }
    }
}

#[cfg(test)]
mod tests;
