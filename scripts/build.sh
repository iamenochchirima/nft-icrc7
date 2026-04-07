#!/bin/bash

BASE_CANISTER_PATH="./src"
CANISTERS=("core_nft" "index_icrc7")
LOCAL_STORAGE_PATH="../ic-storage-canister"

mkdir -p "./wasm"

echo "Building local storage canister artifact"
(
    cd "$LOCAL_STORAGE_PATH" &&
    ./scripts/build.sh
)

cp "$LOCAL_STORAGE_PATH/wasm/storage_canister.wasm" "./wasm/storage_canister.wasm"
cp "$LOCAL_STORAGE_PATH/wasm/storage_canister.wasm.gz" "./wasm/storage_canister.wasm.gz"

# Build each canister
for CANISTER in "${CANISTERS[@]}"; do
    echo "Building canister: $CANISTER"
    
    cargo rustc --crate-type=cdylib --target wasm32-unknown-unknown --target-dir "$BASE_CANISTER_PATH/$CANISTER/target" --release --locked -p $CANISTER &&
    ic-wasm "$BASE_CANISTER_PATH/$CANISTER/target/wasm32-unknown-unknown/release/$CANISTER.wasm" -o "$BASE_CANISTER_PATH/$CANISTER/target/wasm32-unknown-unknown/release/$CANISTER.wasm" shrink &&
    ic-wasm "$BASE_CANISTER_PATH/$CANISTER/target/wasm32-unknown-unknown/release/$CANISTER.wasm" -o "$BASE_CANISTER_PATH/$CANISTER/target/wasm32-unknown-unknown/release/$CANISTER.wasm" optimize --inline-functions-with-loops O3 &&
    gzip --no-name -9 -v -c "$BASE_CANISTER_PATH/$CANISTER/target/wasm32-unknown-unknown/release/$CANISTER.wasm" > "$BASE_CANISTER_PATH/$CANISTER/target/wasm32-unknown-unknown/release/${CANISTER}_canister.wasm.gz" &&
    gzip -v -t "$BASE_CANISTER_PATH/$CANISTER/target/wasm32-unknown-unknown/release/${CANISTER}_canister.wasm.gz" &&
    cp "$BASE_CANISTER_PATH/$CANISTER/target/wasm32-unknown-unknown/release/$CANISTER.wasm" "$BASE_CANISTER_PATH/$CANISTER/wasm/$CANISTER.wasm" &&
    candid-extractor "$BASE_CANISTER_PATH/$CANISTER/wasm/$CANISTER.wasm" > "$BASE_CANISTER_PATH/$CANISTER/wasm/can.did" &&
    cp "$BASE_CANISTER_PATH/$CANISTER/target/wasm32-unknown-unknown/release/${CANISTER}_canister.wasm.gz" "./integrations_tests/wasm" &&
    mv "$BASE_CANISTER_PATH/$CANISTER/target/wasm32-unknown-unknown/release/${CANISTER}_canister.wasm.gz" "$BASE_CANISTER_PATH/$CANISTER/wasm/${CANISTER}_canister.wasm.gz"
    
    echo "Finished building canister: $CANISTER"
done

echo "All canisters built successfully!"
