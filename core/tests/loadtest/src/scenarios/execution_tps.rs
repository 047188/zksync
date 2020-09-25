//! Load test meant to run against running node.
//! Runs scenario of deposits, withdraws and transfers. Scenario details are
//! specified as input json file. Transactions are sent concurrently. Program exits
//! successfully if all transactions get verified within configured timeout.
//!
//! This scenario measures the execution TPS.
//! Unlike the `outgoing_tps` scenario, here we send all the transactions, and start
//! measuring TPS only when sent transactions become executed.

// Built-in import
use std::{
    ops::Mul,
    sync::Arc,
    time::{Duration, Instant},
};
// External uses
use num::BigUint;
use tokio::{runtime::Handle, time};
// Workspace uses
use models::node::tx::TxHash;
use zksync::{Network, Provider};
// Local uses
use crate::{
    scenarios::{
        configs::LoadTestConfig,
        utils::{deposit_single, rand_amount, wait_for_verify},
        ScenarioContext,
    },
    sent_transactions::SentTransactions,
    test_accounts::TestWallet,
    tps_counter::{run_tps_counter_printer, TPSCounter},
};

const TX_EXECUTION_TIMEOUT_SEC: u64 = 5 * 60;

/// Runs the execution TPS scenario:
/// sends the different types of transactions, and measures the TPS for the txs execution
/// (not including the verification).
pub fn run_scenario(mut ctx: ScenarioContext) {
    let provider = Provider::new(Network::Localhost);

    // Load config and construct test accounts
    let config = LoadTestConfig::load(&ctx.config_path);
    let test_wallets = ctx.rt.block_on(TestWallet::from_info_list(
        &config.input_accounts,
        provider.clone(),
        &ctx.options,
    ));

    let verify_timeout_sec = Duration::from_secs(config.verify_timeout_sec);

    // Obtain the Ethereum node JSON RPC address.
    log::info!("Starting the loadtest");

    // Spawn the TPS counter.
    ctx.rt
        .spawn(run_tps_counter_printer(ctx.tps_counter.clone()));

    // Send the transactions and block until all of them are sent.
    let sent_txs = ctx.rt.block_on(send_transactions(
        test_wallets,
        provider.clone(),
        config,
        ctx.rt.handle().clone(),
        ctx.tps_counter,
    ));

    // Wait until all the transactions are verified.
    log::info!("Waiting for all transactions to be verified");
    ctx.rt
        .block_on(wait_for_verify(sent_txs, verify_timeout_sec, &provider))
        .expect("Verifying failed");
    log::info!("Loadtest completed.");
}

// Sends the configured deposits, withdraws and transfers from each account concurrently.
async fn send_transactions(
    test_wallets: Vec<TestWallet>,
    provider: Provider,
    ctx: LoadTestConfig,
    rt_handle: Handle,
    tps_counter: Arc<TPSCounter>,
) -> SentTransactions {
    // Send transactions from every account.

    let join_handles: Vec<_> = test_wallets
        .into_iter()
        .map(|account| {
            rt_handle.spawn(send_transactions_from_acc(
                account,
                ctx.clone(),
                provider.clone(),
            ))
        })
        .collect();

    // Collect all the sent transactions (so we'll be able to wait for their confirmation).
    let mut merged_txs = SentTransactions::new();

    let mut txs_await_handles = Vec::new();

    // Await for the transaction send routines, and create the transaction execution routines
    // (which will measure the execution TPS).
    for j in join_handles {
        let sent_txs_result = j.await.expect("Join handle panicked");

        match sent_txs_result {
            Ok(sent_txs) => {
                let task_handle = rt_handle.spawn(await_txs_execution(
                    sent_txs.tx_hashes.clone(),
                    Arc::clone(&tps_counter),
                    provider.clone(),
                ));

                txs_await_handles.push(task_handle);

                merged_txs.merge(sent_txs);
            }
            Err(err) => log::warn!("Failed to send txs: {}", err),
        }
    }

    // Await transaction execution routines.
    for j in txs_await_handles {
        j.await.expect("Join handle panicked");
    }

    merged_txs
}

// Sends the configured deposits, withdraws and transfer from a single account concurrently.
async fn send_transactions_from_acc(
    mut test_wallet: TestWallet,
    ctx: LoadTestConfig,
    provider: Provider,
) -> Result<SentTransactions, failure::Error> {
    let mut sent_txs = SentTransactions::new();
    let addr_hex = hex::encode(test_wallet.address());
    let wei_in_gwei = BigUint::from(1_000_000_000u32);

    // Perform the deposit operation.
    let deposit_amount = BigUint::from(ctx.deposit_initial_gwei).mul(&wei_in_gwei);
    let op_id = deposit_single(&test_wallet, deposit_amount.clone(), &provider).await?;

    log::info!(
        "Account {}: initial deposit completed (amount: {})",
        addr_hex,
        deposit_amount
    );
    sent_txs.add_op_id(op_id);

    log::info!(
        "Account {}: performing {} deposit operations",
        addr_hex,
        ctx.n_deposits,
    );

    // Add the deposit operations.
    for _ in 0..ctx.n_deposits {
        let amount = rand_amount(ctx.deposit_from_amount_gwei, ctx.deposit_to_amount_gwei);
        let op_id = deposit_single(&test_wallet, amount.mul(&wei_in_gwei), &provider).await?;
        sent_txs.add_op_id(op_id);
    }

    // Now when deposits are done it is time to update account id.
    test_wallet.update_account_id().await?;

    // Create a queue for all the transactions to send.
    // First, we will create and sign all the transactions, and then we will send all the
    // prepared transactions.
    let n_change_pubkeys = 1;
    let txs_amount = (n_change_pubkeys + ctx.n_transfers + ctx.n_withdraws) as usize;
    let mut tx_queue = Vec::with_capacity(txs_amount);

    log::info!(
        "Account {}: preparing {} transactions to send",
        addr_hex,
        txs_amount,
    );

    // Add the `ChangePubKey` operation.
    tx_queue.push((test_wallet.sign_change_pubkey().await?, None));

    // Add the transfer operations.
    for _ in 0..ctx.n_transfers {
        let amount = rand_amount(ctx.transfer_from_amount_gwei, ctx.transfer_to_amount_gwei);
        let signed_transfer = test_wallet
            .sign_transfer_to_random(&ctx.input_accounts, amount.mul(&wei_in_gwei))
            .await?;
        tx_queue.push(signed_transfer);
    }
    // Add the withdraw operations.
    for _ in 0..ctx.n_withdraws {
        let amount = rand_amount(ctx.withdraw_from_amount_gwei, ctx.withdraw_to_amount_gwei);
        let signed_withdraw = test_wallet
            .sign_withdraw_single(amount.mul(&wei_in_gwei))
            .await?;
        tx_queue.push(signed_withdraw)
    }

    log::info!(
        "Account {}: preparing transactions completed, sending...",
        addr_hex
    );

    for (tx, eth_sign) in tx_queue {
        let tx_hash = provider.send_tx(tx, eth_sign).await?;
        sent_txs.add_tx_hash(tx_hash);
    }

    log::info!("Account: {}: all the transactions are sent", addr_hex);

    Ok(sent_txs)
}

/// Waits for the transactions to be executed and measures the execution TPS.
async fn await_txs_execution(
    tx_hashes: Vec<TxHash>,
    tps_counter: Arc<TPSCounter>,
    provider: Provider,
) {
    async fn await_tx(tx_hash: TxHash, provider: Provider, tps_counter: Arc<TPSCounter>) {
        let timeout = Duration::from_secs(TX_EXECUTION_TIMEOUT_SEC);
        let start = Instant::now();

        // Small polling interval, so we won't wait too long between confirmation
        // check attempts.
        let polling_interval = Duration::from_millis(100);
        let mut timer = time::interval(polling_interval);
        loop {
            let state = provider
                .tx_info(tx_hash.clone())
                .await
                .expect("[wait_for_verify] call tx_info");

            if state.executed {
                tps_counter.increment();
                break;
            }
            if start.elapsed() > timeout {
                panic!("[wait_for_verify] Timeout")
            }
            timer.tick().await;
        }
    }

    for hash in tx_hashes {
        await_tx(hash, provider.clone(), tps_counter.clone()).await;
    }
}
