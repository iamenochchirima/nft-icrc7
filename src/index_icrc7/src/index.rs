use crate::memory::{get_index_memory, VM};
use crate::wrapped_values::{WrappedAccount, WrappedNat};

use crate::blocks::{get_block_instance, BlockType};
use candid::CandidType;
use ic_stable_structures::{storable::Bound, StableBTreeMap, Storable};
use icrc_ledger_types::icrc::generic_value::ICRC3Value;
use icrc_ledger_types::icrc3::blocks::BlockWithId;
use minicbor::{Decode, Encode};
use serde::{Deserialize, Serialize};
use std::borrow::Cow;
use std::str::FromStr;

#[derive(
    CandidType, Deserialize, Serialize, Ord, PartialOrd, Eq, PartialEq, Clone, Encode, Decode, Debug,
)]
pub enum SortBy {
    #[n(0)]
    Ascending,
    #[n(1)]
    Descending,
}

#[derive(
    CandidType, Deserialize, Serialize, Ord, PartialOrd, Eq, PartialEq, Clone, Encode, Decode, Debug,
)]
pub enum IndexType {
    #[n(0)]
    Account(#[n(0)] WrappedAccount),
    #[n(1)]
    BlockType(#[n(0)] String),
    #[n(2)]
    TokenId(#[n(0)] WrappedNat),
    // ....
}

#[derive(Debug)]
pub struct IndexValue(pub Vec<u64>);

thread_local! {
pub static __INDEX: std::cell::RefCell<Index> = std::cell::RefCell::new(init_index());
}

pub type Index = StableBTreeMap<IndexType, IndexValue, VM>;

pub fn init_index() -> Index {
    let memory = get_index_memory();
    StableBTreeMap::init(memory)
}

impl Storable for IndexType {
    fn to_bytes(&self) -> Cow<'_, [u8]> {
        let mut buffer = Vec::new();
        minicbor::encode(self, &mut buffer).expect("failed to encode IndexType");
        Cow::Owned(buffer)
    }

    fn into_bytes(self) -> Vec<u8> {
        let mut buffer = Vec::new();
        minicbor::encode(self, &mut buffer).expect("failed to encode IndexType");
        buffer
    }

    fn from_bytes(bytes: Cow<[u8]>) -> Self {
        minicbor::decode(&bytes).expect("failed to decode IndexType")
    }
    const BOUND: Bound = Bound::Unbounded;
}

impl Storable for IndexValue {
    fn to_bytes(&self) -> Cow<'_, [u8]> {
        let mut buffer = Vec::new();
        minicbor::encode(&self.0, &mut buffer).expect("failed to encode IndexValue");
        Cow::Owned(buffer)
    }

    fn into_bytes(self) -> Vec<u8> {
        let mut buffer = Vec::new();
        minicbor::encode(&self.0, &mut buffer).expect("failed to encode IndexValue");
        buffer
    }

    fn from_bytes(bytes: Cow<[u8]>) -> Self {
        let index_value = minicbor::decode(&bytes).expect("failed to decode IndexValue");
        IndexValue(index_value)
    }

    const BOUND: Bound = Bound::Unbounded;
}

pub fn add_block_to_index(block: &BlockWithId) -> Result<(), String> {
    let data = &block.block;

    let block_type = if let ICRC3Value::Map(map) = data {
        if let Some(ICRC3Value::Text(btype_str)) = map.get("btype") {
            BlockType::from_str(btype_str)?
        } else {
            return Err("Missing or invalid block type field".to_string());
        }
    } else {
        return Err("Invalid block data".to_string());
    };

    let block_instance = get_block_instance(&block_type);
    let accounts = block_instance.extract_accounts(data).unwrap_or_default();
    let token_id = block_instance.extract_token_id(data).unwrap_or_default();
    let _timestamp = block_instance.extract_timestamp(data).unwrap_or_default();

    let block_id = block.id.0.clone().try_into().unwrap();

    __INDEX.with(|index| {
        let mut index_mut = index.borrow_mut();

        for account in &accounts {
            let account_key = IndexType::Account(account.clone());
            let mut values = index_mut
                .get(&account_key)
                .map(|v| v.0)
                .unwrap_or_default();
            if !values.contains(&block_id) {
                values.insert(0, block_id);
                index_mut.insert(account_key, IndexValue(values));
            }
        }

        let block_type_key = IndexType::BlockType(block_type.to_string());
        let mut values = index_mut
            .get(&block_type_key)
            .map(|v| v.0)
            .unwrap_or_default();
        if !values.contains(&block_id) {
            values.insert(0, block_id);
            index_mut.insert(block_type_key, IndexValue(values));
        }

        if let Some(token_id) = token_id {
            let token_id_key = IndexType::TokenId(token_id);
            let mut values = index_mut
                .get(&token_id_key)
                .map(|v| v.0)
                .unwrap_or_default();
            if !values.contains(&block_id) {
                values.insert(0, block_id);
                index_mut.insert(token_id_key, IndexValue(values));
            }
        }
    });

    Ok(())
}
