use crate::error::Error;
use alloc::{collections::BTreeSet, vec, vec::Vec};
use blake2b_ref::Blake2bBuilder;
use ckb_std::{
    ckb_constants::Source,
    ckb_types::{
        core::ScriptHashType,
        packed::{Byte32, Script},
        prelude::Pack,
    },
    high_level::{
        load_cell_data, load_cell_lock_hash, load_cell_type, load_cell_type_hash, QueryIter,
    },
};
use core::{cmp::Ordering, result::Result};
use protocol::{
    prelude::{Builder, Entity},
    reader, writer, Cursor,
};

pub enum FILTER {
    APPLIED,
    APPLYING,
    NOTAPPLY,
}

pub enum MODE {
    UPDATE,
    BURN,
    ADMIN,
    COMPANION,
}

#[derive(Clone, Default, PartialEq, Eq, PartialOrd)]
pub struct StakeInfoObject {
    pub identity: [u8; 21],
    pub stake_amount: u128,
    pub inauguration_era: u64,
}

impl StakeInfoObject {
    pub fn new(stake_info: &reader::StakeInfo) -> Self {
        let mut object = Self::default();
        let mut identity = vec![stake_info.identity().flag()];
        identity.append(&mut stake_info.identity().content());
        object.identity.copy_from_slice(identity.as_slice());
        object.stake_amount = bytes_to_u128(&stake_info.stake_amount());
        object.inauguration_era = bytes_to_u64(&stake_info.inauguration_era());
        object
    }
}

impl Ord for StakeInfoObject {
    fn cmp(&self, other: &Self) -> Ordering {
        other.stake_amount.cmp(&self.stake_amount)
    }
}

pub fn get_stake_data_by_type_hash(
    cell_type_hash: &[u8; 32],
    source: Source,
) -> Result<reader::StakeLockCellData, Error> {
    let mut stake_data = None;
    QueryIter::new(load_cell_type_hash, source)
        .enumerate()
        .for_each(|(i, type_hash)| {
            if &type_hash.unwrap_or([0u8; 32]) == cell_type_hash {
                assert!(stake_data.is_none());
                stake_data = {
                    let data = load_cell_data(i, source).unwrap();
                    let stake_data: reader::StakeLockCellData = Cursor::from(data).into();
                    Some(stake_data)
                };
            }
        });
    if stake_data.is_none() {
        return Err(Error::StakeDataEmpty);
    }
    Ok(stake_data.unwrap())
}

pub fn get_total_sudt_by_type_hash(
    sudt_type_hash: &Vec<u8>,
    identity: &[u8; 21],
    source: Source,
) -> Result<u128, Error> {
    let mut sudt = 0;
    QueryIter::new(load_cell_type_hash, source)
        .enumerate()
        .map(|(i, type_hash)| {
            if type_hash.unwrap_or([0u8; 32]) == sudt_type_hash.as_slice() {
                let cell_type = load_cell_type(i, source).unwrap();
                if cell_type.is_none() {
                    return Err(Error::SudtTypeArgsMissing);
                }
                if cell_type.unwrap().args().raw_data().to_vec()[..21] == identity[..] {
                    let data = load_cell_data(i, source).unwrap();
                    sudt += bytes_to_u128(&data[..16].to_vec());
                }
            }
            Ok(())
        })
        .collect::<Result<Vec<_>, _>>()?;
    Ok(sudt)
}

pub fn get_checkpoint_from_celldeps(
    checkpoint_type_hash: &Vec<u8>,
) -> Result<reader::CheckpointLockCellData, Error> {
    let mut checkpoint_data = None;
    QueryIter::new(load_cell_type_hash, Source::CellDep)
        .enumerate()
        .for_each(|(i, type_hash)| {
            if type_hash.unwrap_or([0u8; 32]) == checkpoint_type_hash.as_slice() {
                assert!(checkpoint_data.is_none());
                checkpoint_data = {
                    let data = load_cell_data(i, Source::CellDep).unwrap();
                    let checkpoint_data: reader::CheckpointLockCellData = Cursor::from(data).into();
                    Some(checkpoint_data)
                };
            }
        });
    if checkpoint_data.is_none() {
        return Err(Error::CheckpointDataEmpty);
    }
    Ok(checkpoint_data.unwrap())
}

pub fn bytes_to_u64(bytes: &Vec<u8>) -> u64 {
    let mut array: [u8; 8] = [0u8; 8];
    array.copy_from_slice(bytes.as_slice());
    u64::from_le_bytes(array)
}

pub fn bytes_to_u32(bytes: &Vec<u8>) -> u32 {
    let mut array: [u8; 4] = [0u8; 4];
    array.copy_from_slice(bytes.as_slice());
    u32::from_le_bytes(array)
}

pub fn bytes_to_u128(bytes: &Vec<u8>) -> u128 {
    let mut array: [u8; 16] = [0u8; 16];
    array.copy_from_slice(bytes.as_slice());
    u128::from_le_bytes(array)
}

pub fn filter_stakeinfos(
    era: u64,
    quorum: u8,
    stake_infos: &BTreeSet<StakeInfoObject>,
    filter_type: FILTER,
) -> Result<BTreeSet<StakeInfoObject>, Error> {
    let mut filtered_stake_infos = BTreeSet::new();
    match filter_type {
        FILTER::APPLIED => {
            let mut minimal_era = u64::MAX;
            for stake_info in stake_infos {
                if stake_info.inauguration_era <= era {
                    if stake_info.inauguration_era < minimal_era {
                        minimal_era = stake_info.inauguration_era;
                    }
                    if !filtered_stake_infos.insert(stake_info.clone()) {
                        return Err(Error::StakeInfoDumplicateError);
                    }
                }
            }
            filtered_stake_infos = filtered_stake_infos
                .into_iter()
                .filter(|info| info.inauguration_era == minimal_era)
                .collect::<Vec<_>>()[..quorum as usize]
                .to_vec()
                .into_iter()
                .collect();
        }
        FILTER::APPLYING => {
            let mut minimal_era = u64::MAX;
            for stake_info in stake_infos {
                if stake_info.inauguration_era <= era + 1 {
                    if stake_info.inauguration_era < minimal_era {
                        minimal_era = stake_info.inauguration_era;
                    }
                    if !filtered_stake_infos.insert(stake_info.clone()) {
                        return Err(Error::StakeInfoDumplicateError);
                    }
                }
            }
            filtered_stake_infos = filtered_stake_infos
                .into_iter()
                .filter(|info| info.inauguration_era == minimal_era)
                .collect::<Vec<_>>()[..quorum as usize]
                .to_vec()
                .into_iter()
                .collect();
        }
        FILTER::NOTAPPLY => {
            for stake_info in stake_infos {
                if stake_info.inauguration_era > era + 1 {
                    if !filtered_stake_infos.insert(stake_info.clone()) {
                        return Err(Error::StakeInfoDumplicateError);
                    }
                }
            }
            if filtered_stake_infos.len() as u8 >= quorum {
                return Err(Error::StakeInfoQuorumError);
            }
        }
    }
    Ok(filtered_stake_infos)
}

pub fn stakeinfos_into_set(
    stake_infos: &reader::StakeInfoVec,
) -> Result<BTreeSet<StakeInfoObject>, Error> {
    let mut btree_set = BTreeSet::new();
    for i in 0..stake_infos.len() {
        if btree_set.insert(StakeInfoObject::new(&stake_infos.get(i))) {
            return Err(Error::StakeInfoDumplicateError);
        }
    }
    Ok(btree_set)
}

pub fn calc_withdrawal_lock_hash(
    withdrawal_code_hash: &Vec<u8>,
    admin_identity: reader::Identity,
    checkpoint_type_hash: &Vec<u8>,
    node_identity: &[u8; 21],
) -> [u8; 32] {
    let node_identity = {
        let identity = writer::Identity::new_builder()
            .flag(node_identity[0].into())
            .content(writer::Byte20::new_unchecked(node_identity[1..20].into()))
            .build();
        writer::IdentityOpt::new_builder()
            .set(Some(identity))
            .build()
    };
    let admin_identity = writer::Identity::new_builder()
        .flag(admin_identity.flag().into())
        .content(writer::Byte20::new_unchecked(
            admin_identity.content().into(),
        ))
        .build();
    let withdrawal_lock_args = writer::WithdrawalLockArgs::new_builder()
        .admin_identity(admin_identity)
        .checkpoint_cell_type_hash(writer::Byte32::new_unchecked(
            checkpoint_type_hash.as_slice().into(),
        ))
        .node_identity(node_identity)
        .build();
    let withdrawal_lock = Script::new_builder()
        .code_hash(Byte32::new_unchecked(
            withdrawal_code_hash.as_slice().into(),
        ))
        .hash_type(ScriptHashType::Data.into())
        .args(withdrawal_lock_args.as_slice().pack())
        .build();
    let mut lock_hash = [0u8; 32];
    let mut blake2b = Blake2bBuilder::new(32)
        .personal(b"ckb-default-hash")
        .build();
    blake2b.update(withdrawal_lock.as_slice());
    blake2b.finalize(&mut lock_hash);
    lock_hash
}

pub fn get_withdrawal_total_sudt_amount(
    withdrawal_lock_hash: &[u8; 32],
    sudt_type_hash: &Vec<u8>,
    period: u64,
    source: Source,
) -> Result<u128, Error> {
    let mut total_sudt = 0;
    QueryIter::new(load_cell_lock_hash, source)
        .enumerate()
        .map(|(i, lock_hash)| {
            if &lock_hash == withdrawal_lock_hash {
                let type_hash = load_cell_type_hash(i, source).unwrap();
                if type_hash.unwrap_or([0u8; 32]) == sudt_type_hash.as_slice() {
                    let data = load_cell_data(i, source).unwrap();
                    if data.len() < 24 {
                        return Err(Error::WithdrawCellError);
                    }
                    if period != bytes_to_u64(&data[16..24].to_vec()) {
                        return Err(Error::WithdrawCellError);
                    }
                    total_sudt += bytes_to_u128(&data[..16].to_vec());
                }
            }
            Ok(())
        })
        .collect::<Result<Vec<_>, _>>()?;
    Ok(total_sudt)
}
