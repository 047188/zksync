use serde::{Deserialize, Serialize};

use num::{BigUint, Zero};

use zksync_crypto::{
    franklin_crypto::eddsa::PrivateKey,
    params::{max_account_id, max_token_id},
    Engine,
};
use zksync_utils::{format_units, BigUintSerdeAsRadix10Str};

use crate::{
    helpers::{is_fee_amount_packable, pack_fee_amount},
    tx::{TimeRange, TxSignature, VerifiedSignatureCache},
    AccountId, Address, Nonce, PubKeyHash, TokenId, H256,
};
use parity_crypto::Keccak256;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MintNFT {
    pub creator_id: AccountId,
    /// id of nft creator
    pub creator_address: Address,
    /// hash of data in nft token
    pub content_hash: H256,
    /// recipient account
    pub recipient: Address,
    #[serde(with = "BigUintSerdeAsRadix10Str")]
    pub fee: BigUint,
    /// Token to be used for fee.
    #[serde(default)]
    pub fee_token: TokenId,
    /// Current account nonce.
    pub nonce: Nonce,
    /// Time range when the transaction is valid
    #[serde(flatten)]
    pub time_range: TimeRange,
    /// Transaction zkSync signature.
    pub signature: TxSignature,
    #[serde(skip)]
    cached_signer: VerifiedSignatureCache,
}

impl MintNFT {
    /// Unique identifier of the transaction type in zkSync network.
    pub const TX_TYPE: u8 = 9;

    /// Creates transaction from all the required fields.
    ///
    /// While `signature` field is mandatory for new transactions, it may be `None`
    /// in some cases (e.g. when restoring the network state from the L1 contract data).
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        creator_id: AccountId,
        creator_address: Address,
        content_hash: H256,
        recipient: Address,
        fee: BigUint,
        fee_token: TokenId,
        nonce: Nonce,
        time_range: TimeRange,
        signature: Option<TxSignature>,
    ) -> Self {
        let mut tx = Self {
            creator_id,
            creator_address,
            content_hash,
            recipient,
            fee,
            fee_token,
            nonce,
            time_range,
            signature: signature.clone().unwrap_or_default(),
            cached_signer: VerifiedSignatureCache::NotCached,
        };
        if signature.is_some() {
            tx.cached_signer = VerifiedSignatureCache::Cached(tx.verify_signature());
        }
        tx
    }

    /// Creates a signed transaction using private key and
    /// checks for the transaction correcteness.
    #[allow(clippy::too_many_arguments)]
    pub fn new_signed(
        creator_id: AccountId,
        creator_address: Address,
        content_hash: H256,
        recipient: Address,
        fee: BigUint,
        fee_token: TokenId,
        nonce: Nonce,
        time_range: TimeRange,
        private_key: &PrivateKey<Engine>,
    ) -> Result<Self, anyhow::Error> {
        let mut tx = Self::new(
            creator_id,
            creator_address,
            content_hash,
            recipient,
            fee,
            fee_token,
            nonce,
            time_range,
            None,
        );
        tx.signature = TxSignature::sign_musig(private_key, &tx.get_bytes());
        if !tx.check_correctness() {
            anyhow::bail!(crate::tx::TRANSACTION_SIGNATURE_ERROR);
        }
        Ok(tx)
    }

    /// Encodes the transaction data as the byte sequence according to the zkSync protocol.
    pub fn get_bytes(&self) -> Vec<u8> {
        let mut out = Vec::new();
        out.extend_from_slice(&[Self::TX_TYPE]);
        out.extend_from_slice(&self.creator_id.to_be_bytes());
        out.extend_from_slice(&self.creator_address.as_bytes());
        out.extend_from_slice(&self.content_hash.as_bytes());
        out.extend_from_slice(&self.recipient.as_bytes());
        out.extend_from_slice(&pack_fee_amount(&self.fee));
        out.extend_from_slice(&self.fee_token.to_be_bytes());
        out.extend_from_slice(&self.nonce.to_be_bytes());
        out.extend_from_slice(&self.time_range.to_be_bytes());
        out
    }

    /// Verifies the transaction correctness:
    ///
    /// - `account_id` field must be within supported range.
    /// - `token` field must be within supported range.
    /// - `amount` field must represent a packable value.
    /// - `fee` field must represent a packable value.
    /// - transfer recipient must not be `Adddress::zero()`.
    /// - zkSync signature must correspond to the PubKeyHash of the account.
    pub fn check_correctness(&mut self) -> bool {
        let mut valid = self.fee <= BigUint::from(u128::max_value())
            && is_fee_amount_packable(&self.fee)
            && self.creator_id <= max_account_id()
            && self.fee_token <= max_token_id()
            && self.time_range.check_correctness();
        if valid {
            let signer = self.verify_signature();
            valid = valid && signer.is_some();
            self.cached_signer = VerifiedSignatureCache::Cached(signer);
        };
        valid
    }

    /// Restores the `PubKeyHash` from the transaction signature.
    pub fn verify_signature(&self) -> Option<PubKeyHash> {
        if let VerifiedSignatureCache::Cached(cached_signer) = &self.cached_signer {
            *cached_signer
        } else {
            self.signature
                .verify_musig(&self.get_bytes())
                .map(|pub_key| PubKeyHash::from_pubkey(&pub_key))
        }
    }

    /// Get the first part of the message we expect to be signed by Ethereum account key.
    /// The only difference is the missing `nonce` since it's added at the end of the transactions
    /// batch message.
    pub fn get_ethereum_sign_message_part(&self, token_symbol: &str, decimals: u8) -> String {
        let mut message = format!(
            "MintNFT {content} for: {recipient}",
            content = self.content_hash,
            recipient = self.recipient
        );
        if !self.fee.is_zero() {
            message.push('\n');
            message.push_str(
                format!(
                    "Fee: {fee} {token}",
                    fee = format_units(self.fee.clone(), decimals),
                    token = token_symbol
                )
                .as_str(),
            );
        }
        message
    }

    /// Gets message that should be signed by Ethereum keys of the account for 2-Factor authentication.
    pub fn get_ethereum_sign_message(&self, token_symbol: &str, decimals: u8) -> String {
        let mut message = self.get_ethereum_sign_message_part(token_symbol, decimals);
        if !message.is_empty() {
            message.push('\n');
        }
        message.push_str(format!("Nonce: {}", self.nonce).as_str());
        message
    }

    pub fn calculate_address(&self, serial_id: u32) -> Address {
        let mut data = vec![];
        data.extend_from_slice(&self.creator_id.0.to_be_bytes());
        data.extend_from_slice(&serial_id.to_be_bytes());
        data.extend_from_slice(self.content_hash.as_bytes());
        Address::from_slice(&data.keccak256()[12..])
    }
}
