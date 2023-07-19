// Copyright © Aptos Foundation
// Parts of the project are originally copyright © Meta Platforms, Inc.
// SPDX-License-Identifier: Apache-2.0

use crate::pipeline::CommitBlockMessage;
use aptos_block_executor::txn_commit_hook::NoOpTransactionCommitHook;
use aptos_crypto::hash::HashValue;
use aptos_executor::block_executor::{BlockExecutor, TransactionBlockExecutor};
use aptos_executor_types::BlockExecutorTrait;
use aptos_logger::info;
use aptos_types::{
    block_executor::partitioner::{ExecutableBlock, BlockExecutorTransactions},
    transaction::{Transaction, Version},
};
use aptos_vm::{block_executor::{BlockAptosVM, AptosTransactionOutput}, AptosVM};
use move_core_types::vm_status::VMStatus;
use std::{
    sync::{mpsc, Arc},
    time::{Duration, Instant},
};
use aptos_language_e2e_tests::data_store::FakeDataStore;
use once_cell::sync::Lazy;

pub static RAYON_EXEC_POOL: Lazy<Arc<rayon::ThreadPool>> = Lazy::new(|| {
    Arc::new(
        rayon::ThreadPoolBuilder::new()
            .num_threads(num_cpus::get())
            .thread_name(|index| format!("par_exec_{}", index))
            .build()
            .unwrap(),
    )
});

pub struct TransactionExecutor<V> {
    num_blocks_processed: usize,
    executor: Arc<BlockExecutor<V>>,
    parent_block_id: HashValue,
    maybe_first_block_start_time: Option<Instant>,
    version: Version,
    // If commit_sender is `None`, we will commit all the execution result immediately in this struct.
    commit_sender: Option<mpsc::SyncSender<CommitBlockMessage>>,
    allow_discards: bool,
    allow_aborts: bool,
    // Used for blockstm-only benchmark
    state_view: Arc<FakeDataStore>,
}

impl<V> TransactionExecutor<V>
where
    V: TransactionBlockExecutor,
{
    pub fn new(
        executor: Arc<BlockExecutor<V>>,
        parent_block_id: HashValue,
        version: Version,
        commit_sender: Option<mpsc::SyncSender<CommitBlockMessage>>,
        allow_discards: bool,
        allow_aborts: bool,
        state_view: Arc<FakeDataStore>,
    ) -> Self {
        Self {
            num_blocks_processed: 0,
            executor,
            parent_block_id,
            version,
            maybe_first_block_start_time: None,
            commit_sender,
            allow_discards,
            allow_aborts,
            state_view,
        }
    }

    pub fn execute_block(
        &mut self,
        current_block_start_time: Instant,
        partition_time: Duration,
        executable_block: ExecutableBlock<Transaction>,
    ) {
        let execution_start_time = Instant::now();
        if self.maybe_first_block_start_time.is_none() {
            self.maybe_first_block_start_time = Some(current_block_start_time);
        }
        let block_id = executable_block.block_id;
        info!(
            "In iteration {}, received block {}.",
            self.num_blocks_processed, block_id
        );
        let num_txns = executable_block.transactions.num_transactions();
        self.version += num_txns as Version;
        let output = self
            .executor
            .execute_block(executable_block, self.parent_block_id, None)
            .unwrap();

        assert_eq!(output.compute_status().len(), num_txns);
        let discards = output
            .compute_status()
            .iter()
            .flat_map(|status| match status.status() {
                Ok(_) => None,
                Err(error_code) => Some(format!("{:?}", error_code)),
            })
            .collect::<Vec<_>>();

        let aborts = output
            .compute_status()
            .iter()
            .flat_map(|status| match status.status() {
                Ok(execution_status) => {
                    if execution_status.is_success() {
                        None
                    } else {
                        Some(format!("{:?}", execution_status))
                    }
                },
                Err(_) => None,
            })
            .collect::<Vec<_>>();
        if !discards.is_empty() || !aborts.is_empty() {
            println!(
                "Some transactions were not successful: {} discards and {} aborts out of {}, examples: discards: {:?}, aborts: {:?}",
                discards.len(),
                aborts.len(),
                output.compute_status().len(),
                &discards[..(discards.len().min(3))],
                &aborts[..(aborts.len().min(3))]
            )
        }

        assert!(
            self.allow_discards || discards.is_empty(),
            "No discards allowed, {}, examples: {:?}",
            discards.len(),
            &discards[..(discards.len().min(3))]
        );
        assert!(
            self.allow_aborts || aborts.is_empty(),
            "No aborts allowed, {}, examples: {:?}",
            aborts.len(),
            &aborts[..(aborts.len().min(3))]
        );

        if let Some(commit_sender) = &self.commit_sender {
            let msg = CommitBlockMessage {
                block_id,
                root_hash: output.root_hash(),
                first_block_start_time: *self.maybe_first_block_start_time.as_ref().unwrap(),
                current_block_start_time,
                partition_time,
                execution_time: Instant::now().duration_since(execution_start_time),
                num_txns: num_txns - discards.len(),
            };
            commit_sender.send(msg).unwrap();
        } else {
            let ledger_info_with_sigs = super::transaction_committer::gen_li_with_sigs(
                block_id,
                output.root_hash(),
                self.version,
            );
            self.executor
                .commit_blocks(vec![block_id], ledger_info_with_sigs)
                .unwrap();
        }
        self.parent_block_id = block_id;
        self.num_blocks_processed += 1;
    }

    pub fn blockstm_only_execute_block(&mut self, executable_block: ExecutableBlock<Transaction>) {
        BlockAptosVM::execute_block::<
                _,
                NoOpTransactionCommitHook<AptosTransactionOutput, VMStatus>,
            >(
                Arc::clone(&RAYON_EXEC_POOL),
                BlockExecutorTransactions::Unsharded(executable_block.into_txns()),
                self.state_view.as_ref(),
                AptosVM::get_concurrency_level(),
                None,
                None,
            )
            .expect("VM should not fail to start");
    }
}
