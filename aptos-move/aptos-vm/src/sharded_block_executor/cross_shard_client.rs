// Copyright © Aptos Foundation

use crate::{
    block_executor::AptosTransactionOutput,
    sharded_block_executor::{
        cross_shard_state_view::CrossShardStateView,
        messages::{CrossShardMsg, CrossShardMsg::RemoteTxnWriteMsg, RemoteTxnWrite},
    },
};
use aptos_block_executor::{
    task::ExecutionStatus, txn_commit_listener::TransactionCommitListener,
    txn_last_input_output::TxnOutput,
};
use aptos_logger::{info, trace};
use aptos_mvhashmap::types::TxnIndex;
use aptos_state_view::StateView;
use aptos_types::{
    block_executor::partitioner::{ShardId, SubBlock},
    state_store::state_key::StateKey,
    transaction::Transaction,
    write_set::TransactionWrite,
};
use move_core_types::vm_status::VMStatus;
use std::{
    collections::{HashMap, HashSet},
    sync::{
        mpsc::{Receiver, Sender},
        Arc, Mutex,
    },
};
use aptos_crypto::hash::CryptoHash;

pub struct CrossShardCommitReceiver {}

impl CrossShardCommitReceiver {
    pub fn start<S: StateView + Sync + Send>(
        cross_shard_state_view: Arc<CrossShardStateView<S>>,
        message_rx: &Receiver<CrossShardMsg>,
    ) {
        loop {
            let msg = message_rx.recv().unwrap();
            match msg {
                CrossShardMsg::RemoteTxnWriteMsg(txn_commit_msg) => {
                    let (state_key, write_op) = txn_commit_msg.take();
                    cross_shard_state_view
                        .set_value(&state_key, write_op.and_then(|w| w.as_state_value()));
                },
                CrossShardMsg::StopMsg => {
                    break;
                },
            }
        }
    }
}

pub struct CrossShardCommitSender {
    shard_id: ShardId,
    // The senders of cross-shard messages to other shards.
    message_txs: Vec<Mutex<Sender<CrossShardMsg>>>,
    // The hashmap of source txn index to hashmap of conflicting storage location to the
    // list of target txn index and shard id. Please note that the transaction indices stored here is
    // global indices, so we need to convert the local index received from the parallel execution to
    // the global index.
    dependent_edges: HashMap<TxnIndex, HashMap<StateKey, HashSet<ShardId>>>,
    // The offset of the first transaction in the sub-block. This is used to convert the local index
    // in parallel execution to the global index.
    index_offset: TxnIndex,
}

impl CrossShardCommitSender {
    pub fn new(
        shard_id: ShardId,
        message_txs: Vec<Sender<CrossShardMsg>>,
        sub_block: &SubBlock<Transaction>,
    ) -> Self {
        let mut dependent_edges = HashMap::new();
        let mut num_dependent_edges = 0;
        for (txn_idx, txn_with_deps) in sub_block.txn_with_index_iter() {
            let mut storage_locations_to_target = HashMap::new();
            for (txn_id_with_shard, storage_locations) in txn_with_deps
                .cross_shard_dependencies
                .dependent_edges()
                .iter()
            {
                for storage_location in storage_locations {
                    storage_locations_to_target
                        .entry(storage_location.clone().into_state_key())
                        .or_insert_with(HashSet::new)
                        .insert(txn_id_with_shard.shard_id);
                    num_dependent_edges += 1;
                }
            }
            if !storage_locations_to_target.is_empty() {
                dependent_edges.insert(txn_idx as TxnIndex, storage_locations_to_target);
            }
        }

        trace!(
            "CrossShardCommitSender::new: shard_id: {:?}, num_dependent_edges: {:?}",
            shard_id,
            num_dependent_edges
        );

        Self {
            shard_id,
            message_txs: message_txs.into_iter().map(Mutex::new).collect(),
            dependent_edges,
            index_offset: sub_block.start_index as TxnIndex,
        }
    }

    fn send_remote_update_for_success(
        &self,
        txn_idx: TxnIndex,
        txn_output: &AptosTransactionOutput,
    ) {
        let edges = self.dependent_edges.get(&txn_idx).unwrap();
        let write_set = txn_output.committed_output().unwrap().write_set();

        for (state_key, write_op) in write_set.iter() {
            if let Some(dependent_shard_ids) = edges.get(state_key) {
                for dependent_shard_id in dependent_shard_ids.iter() {
                    // let key_str = state_key.hash().to_hex();
                    // info!("SRUFS, src_shard_id={}, src_txn_idx={}, dst_shard_id={}, key={}", self.shard_id, txn_idx, dependent_shard_id, key_str);
                    let message = RemoteTxnWriteMsg(RemoteTxnWrite::new(
                        state_key.clone(),
                        Some(write_op.clone()),
                    ));
                    self.message_txs[*dependent_shard_id]
                        .lock()
                        .unwrap()
                        .send(message)
                        .unwrap();
                }
            }
        }
    }
}

impl TransactionCommitListener for CrossShardCommitSender {
    type TxnOutput = TxnOutput<AptosTransactionOutput, VMStatus>;

    fn on_transaction_committed(&self, txn_idx: TxnIndex, txn_writes: &Self::TxnOutput) {
        let global_txn_idx = txn_idx + self.index_offset;
        if self.dependent_edges.contains_key(&global_txn_idx) {
            match txn_writes.output_status() {
                ExecutionStatus::Success(output) => {
                    self.send_remote_update_for_success(global_txn_idx, output);
                },
                ExecutionStatus::Abort(_) => {
                    todo!("Handle abort case")
                },
                ExecutionStatus::SkipRest(output) => {
                    self.send_remote_update_for_success(global_txn_idx, output);
                },
            }
        }
    }
}