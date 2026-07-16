use crate::{
    memory::{get_transfer_throttles_memory, VM},
    types::wrapped_types::WrappedNat,
};
use candid::Nat;
use ic_stable_structures::{storable::Bound, StableBTreeMap, Storable};
use std::{borrow::Cow, cell::RefCell};

pub const MAX_SUCCESSFUL_TRANSFERS: u16 = 5;
pub const COOLDOWN_NS: u64 = 5 * 60 * 1_000_000_000;
pub const BURST_WINDOW_NS: u64 = 60 * 1_000_000;
pub const MAX_BURST_CALLS: u8 = 5;
pub const TRANSFER_COOLDOWN_ERROR_CODE: u64 = 9_001;
pub const TRANSFER_BURST_ERROR_CODE: u64 = 9_002;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct TransferThrottleState {
    successful_transfers: u16,
    cooldown_until_ns: u64,
    burst_window_started_at_ns: u64,
    burst_calls: u8,
}

impl Storable for TransferThrottleState {
    fn to_bytes(&self) -> Cow<'_, [u8]> {
        Cow::Owned(self.into_bytes())
    }

    fn into_bytes(self) -> Vec<u8> {
        let mut bytes = Vec::with_capacity(19);
        bytes.extend_from_slice(&self.successful_transfers.to_be_bytes());
        bytes.extend_from_slice(&self.cooldown_until_ns.to_be_bytes());
        bytes.extend_from_slice(&self.burst_window_started_at_ns.to_be_bytes());
        bytes.push(self.burst_calls);
        bytes
    }

    fn from_bytes(bytes: Cow<[u8]>) -> Self {
        let bytes = bytes.as_ref();
        assert_eq!(bytes.len(), 19, "invalid transfer throttle state");
        Self {
            successful_transfers: u16::from_be_bytes([bytes[0], bytes[1]]),
            cooldown_until_ns: u64::from_be_bytes(bytes[2..10].try_into().unwrap()),
            burst_window_started_at_ns: u64::from_be_bytes(bytes[10..18].try_into().unwrap()),
            burst_calls: bytes[18],
        }
    }

    const BOUND: Bound = Bound::Bounded {
        max_size: 19,
        is_fixed_size: true,
    };
}

thread_local! {
    static TRANSFER_THROTTLES: RefCell<StableBTreeMap<WrappedNat, TransferThrottleState, VM>> =
        RefCell::new(StableBTreeMap::init(get_transfer_throttles_memory()));
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TransferThrottleError {
    Cooldown { retry_after_ns: u64 },
    BurstLimit { retry_after_ns: u64 },
}

impl TransferThrottleError {
    pub fn error_code(self) -> u64 {
        match self {
            Self::Cooldown { .. } => TRANSFER_COOLDOWN_ERROR_CODE,
            Self::BurstLimit { .. } => TRANSFER_BURST_ERROR_CODE,
        }
    }

    pub fn message(self) -> String {
        match self {
            Self::Cooldown { retry_after_ns } => {
                format!("TRANSFER_COOLDOWN retry_after_ns={retry_after_ns}")
            }
            Self::BurstLimit { retry_after_ns } => {
                format!("TRANSFER_BURST_LIMIT retry_after_ns={retry_after_ns}")
            }
        }
    }
}

/// Called only after the request has passed all ownership and recipient checks.
/// This prevents outsiders from allocating or poisoning per-token guard state.
pub fn check_transfer_allowed(token_id: &Nat, now: u64) -> Result<(), TransferThrottleError> {
    TRANSFER_THROTTLES.with(|throttles| {
        let mut throttles = throttles.borrow_mut();
        let key = WrappedNat::from(token_id.clone());
        let mut state = throttles.get(&key).unwrap_or_default();

        if state.cooldown_until_ns > now {
            return Err(TransferThrottleError::Cooldown {
                retry_after_ns: state.cooldown_until_ns,
            });
        }

        if state.cooldown_until_ns != 0 {
            state.successful_transfers = 0;
            state.cooldown_until_ns = 0;
        }

        if now.saturating_sub(state.burst_window_started_at_ns) >= BURST_WINDOW_NS {
            state.burst_window_started_at_ns = now;
            state.burst_calls = 0;
        }

        if state.burst_calls >= MAX_BURST_CALLS {
            return Err(TransferThrottleError::BurstLimit {
                retry_after_ns: state
                    .burst_window_started_at_ns
                    .saturating_add(BURST_WINDOW_NS),
            });
        }

        state.burst_calls = state.burst_calls.saturating_add(1);
        throttles.insert(key, state);
        Ok(())
    })
}

/// Must be called only after ICRC-3 transaction logging succeeds and ownership changes.
pub fn record_successful_transfer(token_id: &Nat, now: u64) {
    TRANSFER_THROTTLES.with(|throttles| {
        let mut throttles = throttles.borrow_mut();
        let key = WrappedNat::from(token_id.clone());
        let mut state = throttles.get(&key).unwrap_or_default();

        if state.cooldown_until_ns != 0 && state.cooldown_until_ns <= now {
            state.successful_transfers = 0;
            state.cooldown_until_ns = 0;
        }

        state.successful_transfers = state.successful_transfers.saturating_add(1);
        if state.successful_transfers >= MAX_SUCCESSFUL_TRANSFERS {
            state.cooldown_until_ns = now.saturating_add(COOLDOWN_NS);
        }
        throttles.insert(key, state);
    });
}

#[derive(candid::CandidType, serde::Serialize, serde::Deserialize, Clone, Debug, PartialEq, Eq)]
pub struct TransferThrottleStatus {
    pub successful_transfers: u16,
    pub cooldown_until_ns: Option<u64>,
}

pub fn status(token_id: &Nat, now: u64) -> TransferThrottleStatus {
    TRANSFER_THROTTLES.with(|throttles| {
        let state = throttles
            .borrow()
            .get(&WrappedNat::from(token_id.clone()))
            .unwrap_or_default();
        TransferThrottleStatus {
            successful_transfers: if state.cooldown_until_ns > now {
                state.successful_transfers
            } else {
                0
            },
            cooldown_until_ns: (state.cooldown_until_ns > now).then_some(state.cooldown_until_ns),
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn starts_cooldown_after_five_successful_transfers_and_resets_after_expiry() {
        let token_id = Nat::from(9_999_999u64);
        let mut now = BURST_WINDOW_NS + 1;

        TRANSFER_THROTTLES.with(|throttles| {
            throttles
                .borrow_mut()
                .remove(&WrappedNat::from(token_id.clone()));
        });

        for _ in 0..MAX_SUCCESSFUL_TRANSFERS {
            check_transfer_allowed(&token_id, now).unwrap();
            record_successful_transfer(&token_id, now);
            now = now.saturating_add(BURST_WINDOW_NS + 1);
        }

        let protected = status(&token_id, now);
        assert_eq!(protected.successful_transfers, MAX_SUCCESSFUL_TRANSFERS);
        let cooldown_until = protected.cooldown_until_ns.unwrap();
        assert_eq!(
            check_transfer_allowed(&token_id, now),
            Err(TransferThrottleError::Cooldown {
                retry_after_ns: cooldown_until
            })
        );

        check_transfer_allowed(&token_id, cooldown_until).unwrap();
        assert_eq!(status(&token_id, cooldown_until).successful_transfers, 0);

        TRANSFER_THROTTLES.with(|throttles| {
            throttles.borrow_mut().remove(&WrappedNat::from(token_id));
        });
    }

    #[test]
    fn blocks_a_short_burst_without_starting_a_cooldown() {
        let token_id = Nat::from(9_999_998u64);
        let now = BURST_WINDOW_NS + 1;

        TRANSFER_THROTTLES.with(|throttles| {
            throttles
                .borrow_mut()
                .remove(&WrappedNat::from(token_id.clone()));
        });

        for _ in 0..MAX_BURST_CALLS {
            check_transfer_allowed(&token_id, now).unwrap();
        }

        assert_eq!(
            check_transfer_allowed(&token_id, now),
            Err(TransferThrottleError::BurstLimit {
                retry_after_ns: now.saturating_add(BURST_WINDOW_NS)
            })
        );
        assert_eq!(status(&token_id, now).cooldown_until_ns, None);

        TRANSFER_THROTTLES.with(|throttles| {
            throttles.borrow_mut().remove(&WrappedNat::from(token_id));
        });
    }
}
