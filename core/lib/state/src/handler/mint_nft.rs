use anyhow::{bail, ensure, format_err};
use std::time::Instant;
use zksync_crypto::params;
use zksync_types::{
    operations::MintNFTOp, Account, AccountUpdate, AccountUpdates, Address, MintNFT, Nonce, Token,
    TokenId, ZkSyncOp,
};

use crate::{
    handler::TxHandler,
    state::{CollectedFee, OpSuccess, ZkSyncState},
};
use num::{BigUint, ToPrimitive, Zero};
use zksync_crypto::params::{
    MIN_NFT_TOKEN_ID, NFT_STORAGE_ACCOUNT_ADDRESS, NFT_STORAGE_ACCOUNT_ID, NFT_TOKEN_ID,
};
use zksync_types::tokens::NFT;

impl TxHandler<MintNFT> for ZkSyncState {
    type Op = MintNFTOp;

    fn create_op(&self, tx: MintNFT) -> Result<Self::Op, anyhow::Error> {
        ensure!(
            tx.fee_token <= params::max_token_id(),
            "Token id is not supported"
        );
        ensure!(
            tx.recipient != Address::zero(),
            "Transfer to Account with address 0 is not allowed"
        );
        let (recipient, _) = self
            .get_account_by_address(&tx.recipient)
            .ok_or_else(|| format_err!("Recipient account does not exist"))?;

        let op = MintNFTOp {
            creator_account_id: tx.creator_id,
            recipient_account_id: recipient,
            tx,
        };

        Ok(op)
    }

    fn apply_tx(&mut self, tx: MintNFT) -> Result<OpSuccess, anyhow::Error> {
        let op = self.create_op(tx)?;

        let (fee, updates) = <Self as TxHandler<MintNFT>>::apply_op(self, &op)?;
        let result = OpSuccess {
            fee,
            updates,
            executed_op: ZkSyncOp::MintNFTOp(Box::new(op)),
        };

        Ok(result)
    }

    fn apply_op(
        &mut self,
        op: &Self::Op,
    ) -> Result<(Option<CollectedFee>, AccountUpdates), anyhow::Error> {
        let start = Instant::now();
        let mut updates = Vec::new();

        let mut creator_account = self
            .get_account(op.creator_account_id)
            .ok_or(format_err!("Recipient account not found"))?;

        let mut recipient_account = self
            .get_account(op.recipient_account_id)
            .ok_or(format_err!("Recipient account not found"))?;

        // Generate token id
        let (mut nft_account, account_updates) = get_or_create_nft_account_token_id(self);
        updates.extend(account_updates);

        let last_token_id = nft_account.get_balance(NFT_TOKEN_ID);
        nft_account.add_balance(NFT_TOKEN_ID, &BigUint::from(1u32));
        let new_token_id = nft_account.get_balance(NFT_TOKEN_ID);
        updates.push((
            NFT_STORAGE_ACCOUNT_ID,
            AccountUpdate::UpdateBalance {
                balance_update: (NFT_TOKEN_ID, last_token_id, new_token_id.clone()),
                old_nonce: Nonce(0),
                new_nonce: Nonce(0),
            },
        ));

        let token_id = TokenId(new_token_id.to_u32().expect("Should be correct u32"));

        // Generate serial id
        let old_balance = creator_account.get_balance(NFT_TOKEN_ID);
        let old_nonce = creator_account.nonce;
        creator_account.add_balance(NFT_TOKEN_ID, &BigUint::from(1u32));
        *creator_account.nonce += 1;

        let new_balance = creator_account.get_balance(NFT_TOKEN_ID);

        updates.push((
            op.creator_account_id,
            AccountUpdate::UpdateBalance {
                balance_update: (NFT_TOKEN_ID, old_balance, new_balance.clone()),
                old_nonce,
                new_nonce: creator_account.nonce,
            },
        ));
        let serial_id = new_balance.to_u32().unwrap_or_default();

        let token_address = op.tx.calculate_address(serial_id);

        updates.push((
            op.creator_account_id,
            AccountUpdate::MintNFT {
                token: NFT::new(
                    token_id,
                    serial_id,
                    op.tx.creator_id,
                    token_address,
                    None,
                    op.tx.content_hash,
                ),
            },
        ));

        let old_amount = recipient_account.get_balance(token_id);
        if old_amount != BigUint::zero() {
            bail!("Token {} is already in account", token_id)
        }
        let old_nonce = recipient_account.nonce;
        recipient_account.add_balance(token_id, &BigUint::from(1u32));

        updates.push((
            op.recipient_account_id,
            AccountUpdate::UpdateBalance {
                balance_update: (token_id, BigUint::zero(), BigUint::from(1u32)),
                old_nonce,
                new_nonce: old_nonce,
            },
        ));

        let fee = CollectedFee {
            token: op.tx.fee_token,
            amount: op.tx.fee.clone(),
        };

        metrics::histogram!("state.mint_nft", start.elapsed());
        Ok((Some(fee), updates))
    }
}

/// Get or create special account with special balance for enforcing uniqueness of token_id
fn get_or_create_nft_account_token_id(state: &mut ZkSyncState) -> (Account, AccountUpdates) {
    let mut updates = vec![];
    let account = state
        .get_account(NFT_STORAGE_ACCOUNT_ID)
        .unwrap_or_else(|| {
            let balance = BigUint::from(MIN_NFT_TOKEN_ID);
            let (mut account, upd) =
                Account::create_account(NFT_STORAGE_ACCOUNT_ID, *NFT_STORAGE_ACCOUNT_ADDRESS);
            updates.extend(upd.into_iter());
            account.add_balance(NFT_TOKEN_ID, &BigUint::from(MIN_NFT_TOKEN_ID));

            state.insert_account(NFT_STORAGE_ACCOUNT_ID, account.clone());

            updates.push((
                NFT_STORAGE_ACCOUNT_ID,
                AccountUpdate::UpdateBalance {
                    balance_update: (NFT_TOKEN_ID, BigUint::zero(), balance),
                    old_nonce: Nonce(0),
                    new_nonce: Nonce(0),
                },
            ));
            account
        });
    (account, updates)
}
