use lazy_static::lazy_static;
use std::sync::RwLock;

use solana_sdk::{pubkey::Pubkey, signature::Keypair, signer::Signer};

lazy_static! {
    static ref VALIDATOR_AUTHORITY: RwLock<Option<Keypair>> = RwLock::new(None);
}

pub fn validator_authority() -> Keypair {
    VALIDATOR_AUTHORITY
        .read()
        .expect("RwLock VALIDATOR_AUTHORITY poisoned")
        .as_ref()
        .expect("Validator authority needs to be set on startup")
        .insecure_clone()
}

pub fn validator_authority_id() -> Pubkey {
    VALIDATOR_AUTHORITY
        .read()
        .expect("RwLock VALIDATOR_AUTHORITY poisoned")
        .as_ref()
        .map(|x| x.pubkey())
        .expect("Validator authority needs to be set on startup")
}

pub fn has_validator_authority() -> bool {
    VALIDATOR_AUTHORITY
        .read()
        .expect("RwLock VALIDATOR_AUTHORITY poisoned")
        .is_some()
}

pub fn set_validator_authority(keypair: Keypair) {
    {
        let auhority = VALIDATOR_AUTHORITY
            .read()
            .expect("RwLock VALIDATOR_AUTHORITY poisoned");

        if let Some(authority) = auhority.as_ref() {
            panic!("Validator authority can only be set once, but was set before to '{}'", authority.pubkey());
        }
    }

    VALIDATOR_AUTHORITY
        .write()
        .expect("RwLock VALIDATOR_AUTHORITY poisoned")
        .replace(keypair);
}