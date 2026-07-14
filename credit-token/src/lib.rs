#![cfg_attr(not(test), no_std)]

#[cfg(any(test, feature = "testutils"))]
extern crate std;

use soroban_sdk::{contract, contracterror, contractimpl, contracttype, Address, Env};

#[derive(Clone)]
#[contracttype]
pub enum DataKey {
    Balance(Address),
    Allowance(Address, Address),
    Admin,
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
            .set(&DataKey::Balance(to), &new_to_balance);

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
            .set(&DataKey::Balance(from), &(from_balance - amount));

        let to_balance = Self::balance(env.clone(), to.clone());
        let new_to_balance = to_balance.checked_add(amount).ok_or(TokenError::Overflow)?;
        env.storage()
            .persistent()
            .set(&DataKey::Balance(to), &new_to_balance);

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
            .set(&DataKey::Allowance(from, spender), &amount);

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

        let current_balance = Self::balance(env.clone(), to.clone());
        let new_balance = current_balance
            .checked_add(amount)
            .ok_or(TokenError::Overflow)?;

        env.storage()
            .persistent()
            .set(&DataKey::Balance(to), &new_balance);

        Ok(())
    }

    /// Burn `amount` tokens from `from`.
    ///
    /// # Authorization
    /// Only callable by the admin address (the registry contract).
    pub fn burn(env: Env, from: Address, amount: i128) -> Result<(), TokenError> {
        if amount <= 0 {
            return Err(TokenError::NegativeAmount);
        }

        let admin: Address = env
            .storage()
            .instance()
            .get(&DataKey::Admin)
            .expect("not initialized");

        admin.require_auth();

        let from_balance = Self::balance(env.clone(), from.clone());
        if from_balance < amount {
            return Err(TokenError::InsufficientBalance);
        }

        env.storage()
            .persistent()
            .set(&DataKey::Balance(from), &(from_balance - amount));

        Ok(())
    }
}

#[cfg(test)]
mod tests;
