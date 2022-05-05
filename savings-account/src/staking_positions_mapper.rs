elrond_wasm::imports!();
elrond_wasm::derive_imports!();

use core::marker::PhantomData;
use elrond_wasm::{
    api::StorageMapperApi,
    storage::{mappers::StorageMapper, StorageKey},
    storage_clear, storage_get, storage_get_len, storage_set,
};

static STAKING_NONCE_TO_POS_KEY_SUFFIX: &[u8] = b"NonceToId";
static LAST_VALID_ID_KEY_SUFFIX: &[u8] = b"lastValidId";
static INVALID_POS_ID_ERR_MSG: &[u8] = b"Invalid staking position ID";

const LIST_HEAD_POS_ID: u64 = 0;

pub type StakingPositionId = u64;
pub type LiquidStakingTokenNonce = u64;

#[derive(TypeAbi, TopEncode, TopDecode)]
pub struct StakingPosition {
    pub prev_pos_id: StakingPositionId,
    pub next_pos_id: StakingPositionId,
    pub liquid_staking_nonce: LiquidStakingTokenNonce,
}

pub struct StakingPositionsMapper<SA>
where
    SA: StorageMapperApi,
{
    base_key: StorageKey<SA>,
    _phantom: PhantomData<SA>,
}

impl<SA> StorageMapper<SA> for StakingPositionsMapper<SA>
where
    SA: StorageMapperApi,
{
    #[inline]
    fn new(base_key: StorageKey<SA>) -> Self {
        StakingPositionsMapper {
            base_key,
            _phantom: PhantomData,
        }
    }
}

impl<SA> StakingPositionsMapper<SA>
where
    SA: StorageMapperApi,
{
    pub fn init_mapper(&mut self) {
        let key = self.build_staking_pos_key(LIST_HEAD_POS_ID);
        if storage_get_len(key.as_ref()) > 0 {
            return;
        }

        let first_pos = StakingPosition {
            liquid_staking_nonce: 0,
            next_pos_id: 0,
            prev_pos_id: 0,
        };

        storage_set(key.as_ref(), &first_pos);
    }

    pub fn get_staking_position(&self, pos_id: StakingPositionId) -> StakingPosition {
        if pos_id == 0 {
            SA::error_api_impl().signal_error(INVALID_POS_ID_ERR_MSG);
        }

        let key = self.build_staking_pos_key(pos_id);
        if storage_get_len(key.as_ref()) == 0 {
            SA::error_api_impl().signal_error(INVALID_POS_ID_ERR_MSG);
        }

        storage_get(key.as_ref())
    }

    pub fn update_staking_position<R, F: FnOnce(&mut StakingPosition) -> R>(
        &mut self,
        pos_id: StakingPositionId,
        f: F,
    ) -> R {
        let key = self.build_staking_pos_key(pos_id);
        let mut staking_pos: StakingPosition = storage_get(key.as_ref());
        let result = f(&mut staking_pos);
        storage_set(key.as_ref(), &staking_pos);

        result
    }

    pub fn add_staking_position(
        &mut self,
        liquid_staking_nonce: LiquidStakingTokenNonce,
    ) -> StakingPositionId {
        let nonce_to_id_key = self.build_staking_nonce_to_pos_id_key(liquid_staking_nonce);
        let existing_id: StakingPositionId = storage_get(nonce_to_id_key.as_ref());
        if existing_id != 0 {
            return existing_id;
        }

        let last_valid_id_key = self.build_last_valid_id_key();
        let prev_last_id: StakingPositionId = storage_get(last_valid_id_key.as_ref());
        let new_last_id = prev_last_id + 1;

        let prev_pos_key = self.build_staking_pos_key(prev_last_id);
        let mut prev_pos: StakingPosition = storage_get(prev_pos_key.as_ref());
        prev_pos.next_pos_id = new_last_id;
        storage_set(prev_pos_key.as_ref(), &prev_pos);

        let new_last_pos_key = self.build_staking_pos_key(new_last_id);
        storage_set(
            new_last_pos_key.as_ref(),
            &StakingPosition {
                next_pos_id: 0,
                prev_pos_id: prev_last_id,
                liquid_staking_nonce,
            },
        );

        storage_set(nonce_to_id_key.as_ref(), &new_last_id);
        storage_set(last_valid_id_key.as_ref(), &new_last_id);

        new_last_id
    }

    pub fn remove_staking_position(&mut self, pos_id: StakingPositionId) {
        if pos_id == 0 {
            SA::error_api_impl().signal_error(INVALID_POS_ID_ERR_MSG);
        }

        let current_pos_key = self.build_staking_pos_key(pos_id);
        if storage_get_len(current_pos_key.as_ref()) == 0 {
            SA::error_api_impl().signal_error(INVALID_POS_ID_ERR_MSG);
        }

        let pos: StakingPosition = storage_get(current_pos_key.as_ref());

        // re-connect nodes
        let prev_pos_key = self.build_staking_pos_key(pos.prev_pos_id);
        let mut prev_pos: StakingPosition = storage_get(prev_pos_key.as_ref());
        prev_pos.next_pos_id = pos.next_pos_id;
        storage_set(prev_pos_key.as_ref(), &prev_pos);

        if pos.next_pos_id != 0 {
            let next_pos_key = self.build_staking_pos_key(pos.next_pos_id);
            let mut next_pos: StakingPosition = storage_get(next_pos_key.as_ref());
            next_pos.prev_pos_id = pos.prev_pos_id;
            storage_set(next_pos_key.as_ref(), &next_pos);
        }

        let last_valid_id_key = self.build_last_valid_id_key();
        let last_valid_pos_id: StakingPositionId = storage_get(last_valid_id_key.as_ref());
        if pos_id == last_valid_pos_id {
            storage_set(last_valid_id_key.as_ref(), &pos.prev_pos_id);
        }

        storage_clear(current_pos_key.as_ref());
    }

    pub fn get_first_staking_position_id(&self) -> StakingPositionId {
        let key = self.build_staking_pos_key(LIST_HEAD_POS_ID);
        let pos: StakingPosition = storage_get(key.as_ref());

        pos.next_pos_id
    }

    pub fn get_last_valid_staking_pos_id(&self) -> StakingPositionId {
        let key = self.build_last_valid_id_key();
        storage_get(key.as_ref())
    }

    fn build_staking_pos_key(&self, pos_id: StakingPositionId) -> StorageKey<SA> {
        let mut key = self.base_key.clone();
        key.append_item(&pos_id);

        key
    }

    fn build_staking_nonce_to_pos_id_key(
        &self,
        liquid_staking_nonce: LiquidStakingTokenNonce,
    ) -> StorageKey<SA> {
        let mut key = self.base_key.clone();
        key.append_bytes(STAKING_NONCE_TO_POS_KEY_SUFFIX);
        key.append_item(&liquid_staking_nonce);

        key
    }

    fn build_last_valid_id_key(&self) -> StorageKey<SA> {
        let mut key = self.base_key.clone();
        key.append_bytes(LAST_VALID_ID_KEY_SUFFIX);

        key
    }
}
