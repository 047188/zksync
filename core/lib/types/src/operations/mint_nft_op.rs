use crate::{AccountId, Address, Nonce};
use crate::{MintNFT, H256};

use crate::helpers::{pack_fee_amount, unpack_fee_amount};
use serde::{Deserialize, Serialize};
use thiserror::Error;
use zksync_basic_types::TokenId;
use zksync_crypto::params::{
    ACCOUNT_ID_BIT_WIDTH, ADDRESS_WIDTH, CHUNK_BYTES, CONTENT_HASH_WIDTH, FEE_EXPONENT_BIT_WIDTH,
    FEE_MANTISSA_BIT_WIDTH, NFT_STORAGE_ACCOUNT_ID, SERIAL_ID_BIT_WIDTH, TOKEN_BIT_WIDTH,
};
use zksync_crypto::primitives::FromBytes;

#[derive(Error, Debug)]
pub enum MintNFTParsingError {
    #[error("Wrong number of types")]
    WrongNumberOfBytes,
    #[error("Cannot parse creator account id")]
    CreatorAccountId,
    #[error("Cannot parse token id")]
    TokenId,
    #[error("Cannot parse fee token id")]
    FeeTokenId,
    #[error("Cannot parse token account id")]
    AccountId,
    #[error("Cannot parse serial id")]
    SerialId,
    #[error("Cannot parse recipient account id")]
    RecipientAccountId,
    #[error("Cannot parse fee")]
    Fee,
}

/// Deposit operation. For details, see the documentation of [`ZkSyncOp`](./operations/enum.ZkSyncOp.html).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MintNFTOp {
    pub tx: MintNFT,
    pub creator_account_id: AccountId,
    pub recipient_account_id: AccountId,
}

impl MintNFTOp {
    pub const CHUNKS: usize = 6;
    pub const OP_CODE: u8 = 0x09;

    pub fn get_public_data(&self) -> Vec<u8> {
        let mut data = vec![Self::OP_CODE];
        data.extend_from_slice(&self.creator_account_id.to_be_bytes());
        data.extend_from_slice(&self.recipient_account_id.to_be_bytes());
        data.extend_from_slice(&self.tx.creator_address.as_bytes());
        data.extend_from_slice(&self.tx.content_hash.as_bytes());
        data.extend_from_slice(&self.tx.recipient.as_bytes());
        data.extend_from_slice(&pack_fee_amount(&self.tx.fee));
        data.extend_from_slice(&self.tx.fee_token.as_bytes());
        data.resize(Self::CHUNKS * CHUNK_BYTES, 0x00);
        data
    }

    pub fn from_public_data(bytes: &[u8]) -> Result<Self, MintNFTParsingError> {
        if bytes.len() != Self::CHUNKS * CHUNK_BYTES {
            return Err(MintNFTParsingError::WrongNumberOfBytes);
        }

        let creator_account_id_offset = 1;
        let recipient_account_id_offset = creator_account_id_offset + ACCOUNT_ID_BIT_WIDTH / 8;
        let creator_address_offset = recipient_account_id_offset + ACCOUNT_ID_BIT_WIDTH / 8;
        let content_hash_offset = creator_address_offset + ADDRESS_WIDTH / 8;
        let recipient_address_offset = content_hash_offset + CONTENT_HASH_WIDTH / 8;
        let fee_offset = recipient_address_offset + ADDRESS_WIDTH / 8;
        let fee_token_offset = fee_offset + (FEE_EXPONENT_BIT_WIDTH + FEE_MANTISSA_BIT_WIDTH) / 8;

        let creator_account_id = u32::from_bytes(
            &bytes[creator_account_id_offset..creator_account_id_offset + ACCOUNT_ID_BIT_WIDTH / 8],
        )
        .ok_or(MintNFTParsingError::CreatorAccountId)?;

        let recipient_account_id = u32::from_bytes(
            &bytes[recipient_account_id_offset
                ..recipient_account_id_offset + ACCOUNT_ID_BIT_WIDTH / 8],
        )
        .ok_or(MintNFTParsingError::RecipientAccountId)?;

        let creator_address = Address::from_slice(
            &bytes[creator_address_offset..creator_address_offset + ADDRESS_WIDTH / 8],
        );

        let content_hash = H256::from_slice(
            &bytes[content_hash_offset..content_hash_offset + CONTENT_HASH_WIDTH / 8],
        );

        let recipient_address = Address::from_slice(
            &bytes[recipient_address_offset..recipient_address_offset + ADDRESS_WIDTH / 8],
        );

        let fee = unpack_fee_amount(
            &bytes[fee_offset..fee_offset + (FEE_EXPONENT_BIT_WIDTH + FEE_MANTISSA_BIT_WIDTH) / 8],
        )
        .ok_or(MintNFTParsingError::Fee)?;

        let fee_token_id =
            u32::from_bytes(&bytes[fee_token_offset..fee_token_offset + TOKEN_BIT_WIDTH / 8])
                .ok_or(MintNFTParsingError::FeeTokenId)?;

        let nonce = 0; // It is unknown from pubdata

        let time_range = Default::default();
        Ok(Self {
            tx: MintNFT::new(
                AccountId(creator_account_id),
                creator_address,
                content_hash,
                recipient_address,
                fee,
                TokenId(fee_token_id),
                Nonce(nonce),
                time_range,
                None,
            ),
            creator_account_id: AccountId(creator_account_id),
            recipient_account_id: AccountId(recipient_account_id),
        })
    }

    pub fn get_updated_account_ids(&self) -> Vec<AccountId> {
        vec![
            self.recipient_account_id,
            self.creator_account_id,
            NFT_STORAGE_ACCOUNT_ID,
        ]
    }
}
