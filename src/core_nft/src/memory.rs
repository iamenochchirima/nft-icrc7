use ic_stable_structures::{
    memory_manager::{MemoryId, MemoryManager, VirtualMemory},
    DefaultMemoryImpl,
};

// =============================================================================
// MEMORY CONFIGURATION
// =============================================================================
//
// All stable memory is managed through a SINGLE MemoryManager to prevent
// bucket allocation conflicts. The ICRC3 library is configured to use
// this shared MemoryManager via set_memory_getter().
//
// MemoryId assignments:
// - 2: Upgrades (canister state serialization)
// - 3: Token approvals (ICRC37)
// - 4: Collection approvals (ICRC37)
// - 5: NFT Metadata
// - 6: ICRC3 block log data
// =============================================================================

const UPGRADES: MemoryId = MemoryId::new(2);

pub const TOKEN_APPROVALS: MemoryId = MemoryId::new(3);
pub const COLLECTION_APPROVALS: MemoryId = MemoryId::new(4);
pub const METADATA: MemoryId = MemoryId::new(5);
pub const ICRC3_BLOCK_LOG: MemoryId = MemoryId::new(6);

pub type VM = VirtualMemory<DefaultMemoryImpl>;

thread_local! {
    static MEMORY_MANAGER: MemoryManager<DefaultMemoryImpl> = MemoryManager::init(
        DefaultMemoryImpl::default()
    );
}

pub fn get_token_approvals_memory() -> VM {
    get_memory(TOKEN_APPROVALS)
}

pub fn get_collection_approvals_memory() -> VM {
    get_memory(COLLECTION_APPROVALS)
}

pub fn get_upgrades_memory() -> VM {
    get_memory(UPGRADES)
}

pub fn get_metadata_memory() -> VM {
    get_memory(METADATA)
}

/// Get the memory region for ICRC3 block log data.
/// This is passed to the ICRC3 library via set_memory_getter() to ensure
/// ICRC3 uses our shared MemoryManager instead of creating its own.
pub fn get_icrc3_memory() -> VM {
    get_memory(ICRC3_BLOCK_LOG)
}

fn get_memory(id: MemoryId) -> VM {
    MEMORY_MANAGER.with(|m| m.get(id))
}
