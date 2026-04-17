use crate::guards::{
    caller_has_manage_authorities_permission, caller_has_minting_permission,
    caller_has_read_uploads_permission, caller_has_update_collection_metadata_permission,
    caller_has_update_metadata_permission, caller_has_update_uploads_permission, GuardManagement,
};
use crate::state::{icrc3_add_transaction, mutate_state, read_state, InternalFilestorageData};
use crate::types::http::add_redirection;
use crate::types::metadata::__METADATA;
use crate::types::sub_canister::StorageCanister;
use crate::types::{icrc7, management, nft};
use crate::utils::{check_memo, trace};

pub use crate::types::management::{
    batch_finalize_upload, batch_init_upload, batch_store_chunks, cancel_upload, finalize_upload,
    get_user_permissions, grant_permission, has_permission, init_upload,
    migration_icrc3_add_transaction, revoke_permission, store_chunk,
};
pub use crate::types::permissions::Permission;
use bity_ic_icrc3::transaction::{ICRC7Transaction, ICRC7TransactionData};
use bity_ic_storage_canister_api::types::storage::UploadState;
pub use candid::{Nat, Principal};
pub use ic_cdk::call::RejectCode;
use ic_cdk_macros::{query, update};
use icrc_ledger_types::icrc::generic_value::ICRC3Value as Icrc3Value;
use icrc_ledger_types::icrc1::account::Account;
use std::collections::BTreeMap;
use std::collections::HashMap;

fn normalize_media_path(path: &str) -> String {
    if path.starts_with('/') {
        path.to_string()
    } else {
        format!("/{}", path)
    }
}

fn build_asset_url(canister_id: Principal, path: &str) -> String {
    let is_prod = read_state(|state| state.data.is_prod);
    if is_prod {
        format!("https://{canister_id}.raw.icp0.io{path}")
    } else {
        format!("http://{canister_id}.raw.localhost:4943{path}")
    }
}

#[update(guard = "caller_has_update_collection_metadata_permission")]
pub async fn update_collection_metadata(
    req: management::update_collection_metadata::Args,
) -> management::update_collection_metadata::Response {
    let caller = ic_cdk::api::msg_caller();
    let _guard_principal = GuardManagement::new(caller)
        .map_err(|_| management::update_collection_metadata::UpdateCollectionMetadataError::ConcurrentManagementCall)?;

    if let Some(description) = req.description {
        mutate_state(|state| {
            state.data.description = Some(description);
        });
    }

    if let Some(symbol) = req.symbol {
        mutate_state(|state| {
            state.data.symbol = symbol;
        });
    }

    if let Some(name) = req.name {
        mutate_state(|state| {
            state.data.name = name;
        });
    }

    if let Some(logo) = req.logo {
        mutate_state(|state| {
            state.data.logo = Some(logo);
        });
    }

    if let Some(supply_cap) = req.supply_cap {
        mutate_state(|state| {
            state.data.supply_cap = Some(supply_cap);
        });
    }

    if let Some(max_query_batch_size) = req.max_query_batch_size {
        mutate_state(|state| {
            state.data.max_query_batch_size = Some(max_query_batch_size);
        });
    }

    if let Some(max_update_batch_size) = req.max_update_batch_size {
        mutate_state(|state| {
            state.data.max_update_batch_size = Some(max_update_batch_size);
        });
    }

    if let Some(max_take_value) = req.max_take_value {
        mutate_state(|state| {
            state.data.max_take_value = Some(max_take_value);
        });
    }

    if let Some(default_take_value) = req.default_take_value {
        mutate_state(|state| {
            state.data.default_take_value = Some(default_take_value);
        });
    }

    if let Some(max_memo_size) = req.max_memo_size {
        mutate_state(|state| {
            state.data.max_memo_size = Some(max_memo_size);
        });
    }

    if let Some(atomic_batch_transfers) = req.atomic_batch_transfers {
        mutate_state(|state| {
            state.data.atomic_batch_transfers = Some(atomic_batch_transfers);
        });
    }

    if let Some(tx_window) = req.tx_window {
        mutate_state(|state| {
            state.data.tx_window = Some(tx_window);
        });
    }

    if let Some(permitted_drift) = req.permitted_drift {
        mutate_state(|state| {
            state.data.permitted_drift = Some(permitted_drift);
        });
    }

    if let Some(max_canister_storage_threshold) = req.max_canister_storage_threshold {
        mutate_state(|state| {
            state.data.max_canister_storage_threshold = Some(max_canister_storage_threshold);
        });
    }

    if let Some(is_prod) = req.is_prod {
        mutate_state(|state| {
            state.data.is_prod = is_prod;
        });
    }

    Ok(())
}

#[update(guard = "caller_has_minting_permission")]
pub fn mint(req: management::mint::Args) -> management::mint::Response {
    trace("Minting NFT batch");
    trace(&format!("timestamp: {:?}", ic_cdk::api::time()));
    let caller = ic_cdk::api::msg_caller();
    let _guard_principal = GuardManagement::new(caller)
        .map_err(|_| management::mint::MintError::ConcurrentManagementCall)?;

    let max_batch_size = read_state(|state| {
        state
            .data
            .max_update_batch_size
            .clone()
            .unwrap_or(Nat::from(100u64))
    });

    if req.mint_requests.len() > max_batch_size {
        return Err(management::mint::MintError::ExceedMaxAllowedSupplyCap);
    }

    let (current_token_id, token_count, supply_cap) = read_state(|state| {
        (
            state.data.last_token_id.clone(),
            state.data.tokens_list.len(),
            state
                .data
                .supply_cap
                .clone()
            .unwrap_or(Nat::from(icrc7::DEFAULT_MAX_SUPPLY_CAP)),
        )
    });

    let mint_start = ic_cdk::api::time();
    ic_cdk::println!(
        "mint:start batch_size={} token_count={} last_token_id={} supply_cap={}",
        req.mint_requests.len(),
        token_count,
        current_token_id,
        supply_cap,
    );

    if token_count + req.mint_requests.len() > supply_cap {
        return Err(management::mint::MintError::ExceedMaxAllowedSupplyCap);
    }

    for mint_request in &req.mint_requests {
        match check_memo(mint_request.memo.clone()) {
            Ok(_) => {}
            Err(_) => {
                return Err(management::mint::MintError::InvalidMemo);
            }
        }
    }

    ic_cdk::println!(
        "mint:memo_validation_done elapsed_ns={}",
        ic_cdk::api::time().saturating_sub(mint_start)
    );

    let mut new_tokens = Vec::new();
    let mut transactions = Vec::new();
    let timestamp = ic_cdk::api::time();
    let prepare_start = ic_cdk::api::time();

    for (i, mint_request) in req.mint_requests.iter().enumerate() {
        let token_id = current_token_id.clone() + Nat::from(i as u64);

        let mut new_token =
            nft::Icrc7Token::new(token_id.clone(), mint_request.token_owner.clone());
        __METADATA.with_borrow_mut(|m| new_token.add_metadata(m, mint_request.metadata.clone()));

        let transaction = ICRC7Transaction::new(
            "7mint".to_string(),
            timestamp,
            ICRC7TransactionData {
                op: "7mint".to_string(),
                tid: Some(token_id.clone()),
                from: None,
                to: Some(mint_request.token_owner.clone()),
                meta: None,
                memo: mint_request.memo.clone(),
                created_at_time: Some(Nat::from(timestamp)),
            },
        );

        new_tokens.push((token_id, new_token, mint_request.token_owner.clone()));
        transactions.push(transaction);

        let prepared = i + 1;
        if prepared % 10 == 0 || prepared == req.mint_requests.len() {
            ic_cdk::println!(
                "mint:prepare_progress prepared={}/{} elapsed_ns={}",
                prepared,
                req.mint_requests.len(),
                ic_cdk::api::time().saturating_sub(prepare_start)
            );
        }
    }

    ic_cdk::println!(
        "mint:prepare_done total_tokens={} elapsed_ns={}",
        req.mint_requests.len(),
        ic_cdk::api::time().saturating_sub(prepare_start)
    );

    let icrc3_start = ic_cdk::api::time();
    let total_transactions = transactions.len();
    ic_cdk::println!(
        "mint:icrc3_start transaction_count={}",
        total_transactions
    );

    for (index, transaction) in transactions.into_iter().enumerate() {
        match icrc3_add_transaction(transaction) {
            Ok(_) => {
                let completed = index + 1;
                if completed % 10 == 0 || completed == total_transactions {
                    ic_cdk::println!(
                        "mint:icrc3_progress completed={}/{} elapsed_ns={}",
                        completed,
                        total_transactions,
                        ic_cdk::api::time().saturating_sub(icrc3_start)
                    );
                }
            }
            Err(e) => {
                ic_cdk::println!(
                    "mint:icrc3_error index={} elapsed_ns={} error={}",
                    index,
                    ic_cdk::api::time().saturating_sub(icrc3_start),
                    e
                );
                return Err(management::mint::MintError::StorageCanisterError(
                    e.to_string(),
                ));
            }
        }
    }

    ic_cdk::println!(
        "mint:icrc3_done transaction_count={} elapsed_ns={}",
        total_transactions,
        ic_cdk::api::time().saturating_sub(icrc3_start)
    );

    let state_insert_start = ic_cdk::api::time();
    ic_cdk::println!(
        "mint:state_insert_start token_count_before={} batch_size={}",
        token_count,
        req.mint_requests.len()
    );

    mutate_state(|state| {
        let new_last_token_id =
            current_token_id.clone() + Nat::from(req.mint_requests.len() as u64);
        state.data.last_token_id = new_last_token_id;

        for (token_id, new_token, token_owner) in new_tokens {
            state.data.tokens_list.insert(token_id.clone(), new_token);
            state
                .data
                .tokens_list_by_owner
                .entry(token_owner)
                .or_insert(vec![])
                .push(token_id);
        }
    });

    ic_cdk::println!(
        "mint:state_insert_done elapsed_ns={} total_elapsed_ns={}",
        ic_cdk::api::time().saturating_sub(state_insert_start),
        ic_cdk::api::time().saturating_sub(mint_start)
    );

    trace(&format!(
        "Successfully minted {} NFTs",
        req.mint_requests.len()
    ));
    Ok(current_token_id.clone())
}

#[update(guard = "caller_has_update_metadata_permission")]
pub fn update_nft_metadata(
    req: management::update_nft_metadata::Args,
) -> management::update_nft_metadata::Response {
    trace("Updating NFT metadata");
    let caller = ic_cdk::api::msg_caller();
    let _guard_principal = GuardManagement::new(caller).map_err(|_| {
        management::update_nft_metadata::UpdateNftMetadataError::ConcurrentManagementCall
    })?;

    let token_name_hash = req.token_id;

    let token_list = read_state(|state| state.data.tokens_list.clone());

    match token_list.contains_key(&token_name_hash.clone()) {
        true => {
            let mut token = token_list.get(&token_name_hash.clone()).unwrap().clone();

            let previous_metadata =
                __METADATA.with_borrow(|m| m.get_all_data(Some(token_name_hash.clone())));

            __METADATA.with_borrow_mut(|m| token.replace_metadata(m, req.metadata));

            let new_metadata =
                __METADATA.with_borrow(|m| m.get_all_data(Some(token_name_hash.clone())));

            let mut metadata_map = BTreeMap::new();

            metadata_map.insert(
                "icrc7:previous_metadata".to_string(),
                Icrc3Value::Map(
                    previous_metadata
                        .unwrap_or_default()
                        .into_iter()
                        .map(|(k, v)| (k, v.0))
                        .collect(),
                ),
            );

            metadata_map.insert(
                "icrc7:new_metadata".to_string(),
                Icrc3Value::Map(
                    new_metadata
                        .unwrap_or_default()
                        .into_iter()
                        .map(|(k, v)| (k, v.0))
                        .collect(),
                ),
            );

            let meta = Icrc3Value::Map(metadata_map);

            let transaction = ICRC7Transaction::new(
                "7update_token".to_string(),
                ic_cdk::api::time(),
                ICRC7TransactionData {
                    op: "7update_token".to_string(),
                    tid: Some(token_name_hash.clone()),
                    from: Some(Account {
                        owner: ic_cdk::api::msg_caller(),
                        subaccount: None,
                    }),
                    to: None,
                    meta: Some(meta),
                    memo: None,
                    created_at_time: Some(Nat::from(ic_cdk::api::time())),
                },
            );

            match icrc3_add_transaction(transaction) {
                Ok(_) => {}
                Err(e) => {
                    return Err(management::update_nft_metadata::UpdateNftMetadataError::StorageCanisterError(
                        e.to_string(),
                    ));
                }
            };

            mutate_state(|state| {
                state
                    .data
                    .tokens_list
                    .insert(token_name_hash.clone(), token);
            });
            trace(&format!(
                "Updated NFT metadata for token: {:?}",
                token_name_hash.clone()
            ));
        }
        false => {
            return Err(management::update_nft_metadata::UpdateNftMetadataError::TokenDoesNotExist);
        }
    }

    Ok(token_name_hash.clone())
}

#[update]
pub fn burn_nft(token_id: Nat) -> management::burn_nft::Response {
    let caller = ic_cdk::api::msg_caller();
    let _guard_principal = GuardManagement::new(caller)
        .map_err(|_| management::burn_nft::BurnNftError::ConcurrentManagementCall)?;

    let token = match read_state(|state| state.data.tokens_list.get(&token_id).cloned()) {
        Some(token) => token,
        None => {
            return Err(management::burn_nft::BurnNftError::TokenDoesNotExist);
        }
    };

    if caller != token.token_owner.owner {
        return Err(management::burn_nft::BurnNftError::NotTokenOwner);
    }

    let transaction = ICRC7Transaction::new(
        "7burn".to_string(),
        ic_cdk::api::time(),
        ICRC7TransactionData {
            op: "7burn".to_string(),
            tid: Some(token_id.clone()),
            from: Some(token.token_owner.clone()),
            to: None,
            meta: None,
            memo: None,
            created_at_time: Some(Nat::from(ic_cdk::api::time())),
        },
    );

    match icrc3_add_transaction(transaction) {
        Ok(_) => {}
        Err(e) => {
            return Err(management::burn_nft::BurnNftError::StorageCanisterError(
                e.to_string(),
            ));
        }
    }

    mutate_state(|state| {
        state.data.tokens_list.remove(&token_id);
    });

    Ok(())
}

#[update(guard = "caller_has_update_uploads_permission")]
pub async fn init_upload(data: init_upload::Args) -> init_upload::Response {
    let caller = ic_cdk::api::msg_caller();
    let _guard_principal = GuardManagement::new(caller)
        .map_err(|_| init_upload::InitUploadError::ConcurrentManagementCall)?;

    if read_state(|state| state.internal_filestorage.contains_path(&data.file_path)) {
        return Err(init_upload::InitUploadError::FileAlreadyExists);
    }

    let mut sub_canister_manager = read_state(|state| state.data.sub_canister_manager.clone());

    let canister = match sub_canister_manager.init_upload(data.clone()).await {
        Ok((_, canister)) => canister,
        Err(e) => {
            trace(&format!("Error inserting data: {:?}", e));
            return Err(init_upload::InitUploadError::StorageCanisterError(e));
        }
    };

    mutate_state(|state| {
        state.data.sub_canister_manager = sub_canister_manager;
        state.internal_filestorage.insert(
            data.file_path.clone(),
            InternalFilestorageData {
                init_timestamp: ic_cdk::api::time(),
                state: UploadState::Init,
                canister: canister,
                path: data.file_path,
            },
        );
    });

    Ok(init_upload::InitUploadResp {})
}

#[update(guard = "caller_has_update_uploads_permission")]
pub async fn store_chunk(data: store_chunk::Args) -> store_chunk::Response {
    let caller = ic_cdk::api::msg_caller();
    let _guard_principal = GuardManagement::new(caller)
        .map_err(|_| store_chunk::StoreChunkError::ConcurrentManagementCall)?;

    let (init_timestamp, canister_id, file_path) =
        match read_state(|state| state.internal_filestorage.get(&data.file_path).cloned()) {
            Some(data) => match data.state {
                UploadState::Init => (data.init_timestamp, data.canister, data.path),
                UploadState::InProgress => (data.init_timestamp, data.canister, data.path),
                UploadState::Finalized => {
                    return Err(store_chunk::StoreChunkError::UploadAlreadyFinalized);
                }
            },
            None => {
                return Err(store_chunk::StoreChunkError::UploadNotInitialized);
            }
        };

    let canister: StorageCanister = match read_state(|state| {
        state
            .data
            .sub_canister_manager
            .get_canister(canister_id.clone())
    }) {
        Some(canister) => canister,
        None => {
            mutate_state(|state| {
                state.internal_filestorage.remove(&data.file_path);
            });
            return Err(store_chunk::StoreChunkError::StorageCanisterError(
                "Storage canister not found. Cancelling the upload.".to_string(),
            ));
        }
    };

    match canister.store_chunk(data.clone()).await {
        Ok(_) => {}
        Err(e) => {
            trace(&format!("Error storing chunk: {:?}", e));
            return Err(e);
        }
    }
    mutate_state(|state| {
        state.internal_filestorage.insert(
            data.file_path.clone(),
            InternalFilestorageData {
                init_timestamp: init_timestamp,
                state: UploadState::InProgress,
                canister: canister_id,
                path: file_path,
            },
        );
    });

    Ok(store_chunk::StoreChunkResp {})
}

#[update(guard = "caller_has_update_uploads_permission")]
pub async fn finalize_upload(data: finalize_upload::Args) -> finalize_upload::Response {
    trace(&format!("Finalizing upload: {:?}", data));
    let caller = ic_cdk::api::msg_caller();
    let _guard_principal = GuardManagement::new(caller)
        .map_err(|_| finalize_upload::FinalizeUploadError::ConcurrentManagementCall)?;

    let (init_timestamp, media_path, canister_id) =
        match read_state(|state| state.internal_filestorage.get(&data.file_path).cloned()) {
            Some(data) => match data.state {
                UploadState::Init => {
                    return Err(finalize_upload::FinalizeUploadError::UploadNotStarted);
                }
                UploadState::InProgress => (data.init_timestamp, data.path, data.canister),
                UploadState::Finalized => {
                    return Err(finalize_upload::FinalizeUploadError::UploadAlreadyFinalized);
                }
            },
            None => {
                return Err(finalize_upload::FinalizeUploadError::UploadNotStarted);
            }
        };

    let canister: StorageCanister = match read_state(|state| {
        state
            .data
            .sub_canister_manager
            .get_canister(canister_id.clone())
    }) {
        Some(canister) => canister,
        None => {
            mutate_state(|state| {
                state.internal_filestorage.remove(&data.file_path);
            });
            return Err(finalize_upload::FinalizeUploadError::StorageCanisterError(
                "Storage canister not found. Cancelling the upload.".to_string(),
            ));
        }
    };

    match canister.finalize_upload(data.clone()).await {
        Ok(_) => {}
        Err(e) => {
            trace(&format!("Error storing chunk: {:?}", e));
            // TODO shall we automaticly cleanup or add a cleanup and let user retry?
            return Err(e);
        }
    }

    let path = normalize_media_path(&media_path);

    let redirection_url = build_asset_url(canister_id, &path);

    add_redirection(path.clone(), redirection_url.clone());

    mutate_state(|state| {
        state
            .data
            .media_redirections
            .insert(path.clone(), redirection_url);

        state.internal_filestorage.insert(
            data.file_path.clone(),
            InternalFilestorageData {
                init_timestamp: init_timestamp,
                state: UploadState::Finalized,
                canister: canister_id,
                path: media_path,
            },
        );
    });

    let url = build_asset_url(ic_cdk::api::canister_self(), &path);

    return Ok(finalize_upload::FinalizeUploadResp { url: url });
}

#[query(guard = "caller_has_manage_authorities_permission")]
pub fn get_all_storage_subcanisters() -> Vec<candid::Principal> {
    read_state(|state| state.data.sub_canister_manager.list_canisters_ids())
}

#[query(guard = "caller_has_read_uploads_permission")]
pub fn get_upload_status(file_path: String) -> management::get_upload_status::Response {
    let upload_status = read_state(|state| state.internal_filestorage.get(&file_path).cloned());
    match upload_status {
        Some(status) => Ok(status.state),
        None => Err(management::get_upload_status::GetUploadStatusError::UploadNotFound),
    }
}

#[query(guard = "caller_has_read_uploads_permission")]
pub fn get_all_uploads(
    prev: Option<Nat>,
    take: Option<Nat>,
) -> management::get_all_uploads::Response {
    trace(&format!("prev: {:?}, take: {:?}", prev, take));
    let all_uploads = read_state(|state| state.internal_filestorage.clone());
    let start: usize = usize::try_from(prev.unwrap_or(Nat::from(0u64)).0).unwrap_or(0);
    let end: usize = usize::try_from(take.unwrap_or(Nat::from(100u64)).0).unwrap_or(100);
    trace(&format!("start: {:?}, end: {:?}", start, end));
    let filtered_uploads: HashMap<String, UploadState> = all_uploads
        .map
        .iter()
        .skip(start)
        .take(end)
        .map(|(path, status)| (path.clone(), status.state.clone()))
        .collect();

    Ok(filtered_uploads)
}

#[update(guard = "caller_has_update_uploads_permission")]
pub async fn cancel_upload(data: cancel_upload::Args) -> cancel_upload::Response {
    let caller = ic_cdk::api::msg_caller();
    let _guard_principal = GuardManagement::new(caller)
        .map_err(|_| cancel_upload::CancelUploadError::ConcurrentManagementCall)?;

    let canister_id =
        match read_state(|state| state.internal_filestorage.get(&data.file_path).cloned()) {
            Some(data) => match data.state {
                UploadState::Init => data.canister,
                UploadState::InProgress => data.canister,
                UploadState::Finalized => {
                    return Err(cancel_upload::CancelUploadError::UploadAlreadyFinalized);
                }
            },
            None => {
                return Err(cancel_upload::CancelUploadError::UploadNotInitialized);
            }
        };

    let canister: StorageCanister = match read_state(|state| {
        state
            .data
            .sub_canister_manager
            .get_canister(canister_id.clone())
    }) {
        Some(canister) => canister,
        None => {
            mutate_state(|state| {
                state.internal_filestorage.remove(&data.file_path);
            });
            return Err(cancel_upload::CancelUploadError::StorageCanisterError(
                "Storage canister not found. Cancelling the upload.".to_string(),
            ));
        }
    };

    match canister.cancel_upload(data.clone()).await {
        Ok(_) => {}
        Err(e) => {
            trace(&format!("Error storing chunk: {:?}", e));
            return Err(e);
        }
    }

    mutate_state(|state| {
        state.internal_filestorage.remove(&data.file_path);
    });

    Ok(cancel_upload::CancelUploadResp {})
}

#[update(guard = "caller_has_manage_authorities_permission")]
pub fn grant_permission(args: grant_permission::Args) -> grant_permission::Response {
    let caller = ic_cdk::api::msg_caller();
    let _guard_principal = GuardManagement::new(caller)
        .map_err(|_| grant_permission::GrantPermissionError::ConcurrentManagementCall)?;

    mutate_state(|state| {
        state
            .data
            .permissions
            .grant_permission(args.principal, args.permission);
    });

    Ok(())
}

#[update(guard = "caller_has_manage_authorities_permission")]
pub fn revoke_permission(args: revoke_permission::Args) -> revoke_permission::Response {
    let caller = ic_cdk::api::msg_caller();
    let _guard_principal = GuardManagement::new(caller)
        .map_err(|_| revoke_permission::RevokePermissionError::ConcurrentManagementCall)?;

    mutate_state(|state| {
        state
            .data
            .permissions
            .revoke_permission(&args.principal, &args.permission);
    });

    Ok(())
}

#[query(guard = "caller_has_manage_authorities_permission")]
pub fn get_user_permissions(args: get_user_permissions::Args) -> get_user_permissions::Response {
    let permissions = read_state(|state| {
        state
            .data
            .permissions
            .get_permissions(&args.principal)
            .cloned()
    });
    match permissions {
        Some(permissions) => Ok(permissions),
        None => Err(get_user_permissions::GetUserPermissionsError::UserNotFound),
    }
}

#[query(guard = "caller_has_manage_authorities_permission")]
pub fn has_permission(args: has_permission::Args) -> has_permission::Response {
    let has_permission = read_state(|state| {
        state
            .data
            .permissions
            .has_permission(&args.principal, &args.permission)
    });

    Ok(has_permission)
}

#[update(guard = "caller_has_update_uploads_permission")]
pub async fn batch_init_upload(data: batch_init_upload::Args) -> batch_init_upload::Response {
    let caller = ic_cdk::api::msg_caller();
    let _guard_principal = GuardManagement::new(caller)
        .map_err(|_| batch_init_upload::BatchInitUploadError::ConcurrentManagementCall)?;

    let mut sub_canister_manager = read_state(|state| state.data.sub_canister_manager.clone());

    let (resp, canister_id) = match sub_canister_manager.batch_init_upload(data.clone()).await {
        Ok((resp, canister_id)) => (resp, canister_id),
        Err(e) => {
            return Err(batch_init_upload::BatchInitUploadError::StorageCanisterError(e));
        }
    };

    for (idx, result) in resp.results.iter().enumerate() {
        if result.is_ok() {
            let file_data = &data.files[idx];
            mutate_state(|state| {
                state.data.sub_canister_manager = sub_canister_manager.clone();
                state.internal_filestorage.insert(
                    file_data.file_path.clone(),
                    InternalFilestorageData {
                        init_timestamp: ic_cdk::api::time(),
                        state: UploadState::Init,
                        canister: canister_id,
                        path: file_data.file_path.clone(),
                    },
                );
            });
        }
    }

    Ok(resp)
}

#[update(guard = "caller_has_update_uploads_permission")]
pub async fn batch_store_chunks(data: batch_store_chunks::Args) -> batch_store_chunks::Response {
    let caller = ic_cdk::api::msg_caller();
    let _guard_principal = GuardManagement::new(caller)
        .map_err(|_| batch_store_chunks::BatchStoreChunksError::ConcurrentManagementCall)?;

    if data.chunks.is_empty() {
        return Ok(batch_store_chunks::BatchStoreChunksResp { results: vec![] });
    }

    let first_file_path = &data.chunks[0].file_path;
    let canister_id = match read_state(|state| state.internal_filestorage.get(first_file_path).cloned()) {
        Some(info) => info.canister,
        None => {
            return Err(batch_store_chunks::BatchStoreChunksError::StorageCanisterError(
                "Upload not initialized".to_string(),
            ));
        }
    };

    let canister: StorageCanister = match read_state(|state| {
        state.data.sub_canister_manager.get_canister(canister_id)
    }) {
        Some(canister) => canister,
        None => {
            return Err(batch_store_chunks::BatchStoreChunksError::StorageCanisterError(
                "Storage canister not found".to_string(),
            ));
        }
    };

    let resp = canister.batch_store_chunks(data.clone()).await?;

    for (idx, result) in resp.results.iter().enumerate() {
        if result.is_ok() {
            let chunk_data = &data.chunks[idx];
            if let Some(info) = read_state(|state| state.internal_filestorage.get(&chunk_data.file_path).cloned()) {
                mutate_state(|state| {
                    state.internal_filestorage.insert(
                        chunk_data.file_path.clone(),
                        InternalFilestorageData {
                            init_timestamp: info.init_timestamp,
                            state: UploadState::InProgress,
                            canister: info.canister,
                            path: info.path,
                        },
                    );
                });
            }
        }
    }

    Ok(resp)
}

#[update(guard = "caller_has_update_uploads_permission")]
pub async fn batch_finalize_upload(
    data: batch_finalize_upload::Args,
) -> batch_finalize_upload::Response {
    let caller = ic_cdk::api::msg_caller();
    let _guard_principal = GuardManagement::new(caller)
        .map_err(|_| batch_finalize_upload::BatchFinalizeUploadError::ConcurrentManagementCall)?;

    if data.files.is_empty() {
        return Ok(batch_finalize_upload::BatchFinalizeUploadResp { results: vec![] });
    }

    let first_file_path = &data.files[0].file_path;
    let canister_id = match read_state(|state| state.internal_filestorage.get(first_file_path).cloned()) {
        Some(info) => info.canister,
        None => {
            return Err(batch_finalize_upload::BatchFinalizeUploadError::StorageCanisterError(
                "Upload not initialized".to_string(),
            ));
        }
    };

    let canister: StorageCanister = match read_state(|state| {
        state.data.sub_canister_manager.get_canister(canister_id)
    }) {
        Some(canister) => canister,
        None => {
            return Err(batch_finalize_upload::BatchFinalizeUploadError::StorageCanisterError(
                "Storage canister not found".to_string(),
            ));
        }
    };

    let resp = canister.batch_finalize_upload(data.clone()).await?;

    for (idx, result) in resp.results.iter().enumerate() {
        if let Ok(_url) = result {
            let file_data = &data.files[idx];
            let media_path = normalize_media_path(&file_data.file_path);
            let redirection_url = build_asset_url(canister_id, &media_path);
            add_redirection(media_path.clone(), redirection_url.clone());

            mutate_state(|state| {
                state.data.media_redirections.insert(media_path.clone(), redirection_url);
                if let Some(info) = state.internal_filestorage.get(&file_data.file_path).cloned() {
                    state.internal_filestorage.insert(
                        file_data.file_path.clone(),
                        InternalFilestorageData {
                            init_timestamp: info.init_timestamp,
                            state: UploadState::Finalized,
                            canister: info.canister,
                            path: info.path,
                        },
                    );
                }
            });
        }
    }

    Ok(resp)
}
