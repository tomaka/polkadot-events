// Smoldot
// Copyright (C) 2019-2021  Parity Technologies (UK) Ltd.
// SPDX-License-Identifier: GPL-3.0-or-later WITH Classpath-exception-2.0

// This program is free software: you can redistribute it and/or modify
// it under the terms of the GNU General Public License as published by
// the Free Software Foundation, either version 3 of the License, or
// (at your option) any later version.

// This program is distributed in the hope that it will be useful,
// but WITHOUT ANY WARRANTY; without even the implied warranty of
// MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE.  See the
// GNU General Public License for more details.

// You should have received a copy of the GNU General Public License
// along with this program.  If not, see <http://www.gnu.org/licenses/>.

use crate::{ffi, network_service};

use core::{num::NonZeroU32, pin::Pin};
use futures::{channel::mpsc, prelude::*};
use smoldot::{
    chain::chain_information, database::finalized_serialize, executor, libp2p, network,
    sync::optimistic,
};
use std::{
    collections::{BTreeMap, HashMap},
    sync::Arc,
};

/// Configuration for a [`SyncService`].
pub struct Config {
    /// Closure that spawns background tasks.
    pub tasks_executor: Box<dyn FnMut(Pin<Box<dyn Future<Output = ()> + Send>>) + Send>,

    /// Initial state of the chain.
    pub chain_information: chain_information::ChainInformation,

    /// Initial storage of the finalized block. Must match [`Config::chain_information`].
    pub finalized_storage: BTreeMap<Vec<u8>, Vec<u8>>,

    /// Access to the network, and index of the chain to sync from the point of view of the
    /// network service.
    pub network_service: (Arc<network_service::NetworkService>, usize),

    /// Receiver for events coming from the network, as returned by
    /// [`network_service::NetworkService::new`].
    pub network_events_receiver: mpsc::Receiver<network_service::Event>,
}

/// Background task that verifies blocks and emits requests.
// TODO: remove or something?
pub struct SyncService {}

impl SyncService {
    /// Initializes the [`SyncService`] with the given configuration.
    pub async fn new(mut config: Config) -> Arc<Self> {
        (config.tasks_executor)(Box::pin(start_sync(
            config.chain_information,
            config.finalized_storage,
            config.network_service.0,
            config.network_service.1,
            config.network_events_receiver,
        )));

        Arc::new(SyncService {})
    }
}

/// Returns the background task of the sync service.
fn start_sync(
    initial_chain_information: chain_information::ChainInformation,
    initial_finalized_storage: BTreeMap<Vec<u8>, Vec<u8>>,
    network_service: Arc<network_service::NetworkService>,
    network_chain_index: usize,
    mut from_network_service: mpsc::Receiver<network_service::Event>,
) -> impl Future<Output = ()> {
    // Holds, in parallel of the database, the storage of the latest finalized block.
    // At the time of writing, this state is stable around ~3MiB for Polkadot, meaning that it is
    // completely acceptable to hold it entirely in memory.
    let mut finalized_block_storage = initial_finalized_storage;

    let vm = smoldot::executor::host::HostVmPrototype::new(
        &finalized_block_storage.get(&b":code"[..]).unwrap(),
        smoldot::executor::storage_heap_pages_to_value(
            finalized_block_storage
                .get(&b":heappages"[..])
                .as_ref()
                .map(|v| v.as_ref()),
        )
        .unwrap(),
        smoldot::executor::vm::ExecHint::Oneshot,
    )
    .unwrap();
    let (runtime_spec, vm) = smoldot::executor::core_version(vm).unwrap();
    let mut finalized_runtime_version = runtime_spec.decode().spec_version;
    let mut finalized_metadata = {
        let query = smoldot::metadata::query_metadata(vm);
        loop {
            match query {
                smoldot::metadata::Query::StorageGet(_) => todo!(),
                smoldot::metadata::Query::Finished(Ok((metadata, _))) => break metadata,
                smoldot::metadata::Query::Finished(Err(err)) => panic!("{}", err),
            }
        }
    };

    let mut sync = optimistic::OptimisticSync::<_, libp2p::PeerId, ()>::new(optimistic::Config {
        chain_information: initial_chain_information,
        sources_capacity: 32,
        blocks_capacity: {
            // This is the maximum number of blocks between two consecutive justifications.
            1024
        },
        source_selection_randomness_seed: rand::random(),
        blocks_request_granularity: NonZeroU32::new(128).unwrap(),
        download_ahead_blocks: {
            // Assuming a verification speed of 1k blocks/sec and a 95% latency of one second,
            // the number of blocks to download ahead of time in order to not block is 1000.
            1024
        },
        full: Some(optimistic::ConfigFull {
            finalized_runtime: {
                // Builds the runtime of the finalized block.
                // Assumed to always be valid, otherwise the block wouldn't have been saved in the
                // database, hence the large number of unwraps here.
                let module = finalized_block_storage.get(&b":code"[..]).unwrap();
                let heap_pages = executor::storage_heap_pages_to_value(
                    finalized_block_storage
                        .get(&b":heappages"[..])
                        .map(|v| &v[..]),
                )
                .unwrap();
                executor::host::HostVmPrototype::new(
                    module,
                    heap_pages,
                    executor::vm::ExecHint::CompileAheadOfTime, // TODO: probably should be decided by the optimisticsync
                )
                .unwrap()
            },
        }),
    });

    async move {
        let mut peers_source_id_map = hashbrown::HashMap::<_, _, fnv::FnvBuildHasher>::default();
        let mut block_requests_finished = stream::FuturesUnordered::new();

        loop {
            let unix_time = ffi::unix_time();

            // Verify blocks that have been fetched from queries.
            let mut process = sync.process_one(unix_time);
            loop {
                match process {
                    optimistic::ProcessOne::Idle { sync: s } => {
                        sync = s;
                        break;
                    }
                    optimistic::ProcessOne::Reset {
                        sync: s,
                        previous_best_height,
                        reason,
                    } => {
                        log::warn!(
                            "Consensus issue above block #{}: {}",
                            previous_best_height,
                            reason
                        );

                        crate::yield_once().await;
                        process = s.process_one(unix_time);
                    }
                    optimistic::ProcessOne::Finalized {
                        sync: s,
                        finalized_blocks,
                    } => {
                        log::info!(
                            "Finalized block #{}",
                            finalized_blocks.last().unwrap().header.number
                        );
                        crate::ffi::best_block_update(
                            finalized_blocks.last().unwrap().header.number,
                        );

                        crate::yield_once().await;

                        let serialized_finalized_chain = finalized_serialize::encode_chain_storage(
                            s.as_chain_information(),
                            Some(finalized_block_storage.iter()),
                        );

                        process = s.process_one(unix_time);

                        let mut new_metadata = HashMap::new();
                        let mut blocks_save = Vec::new();

                        for block in finalized_blocks {
                            for (key, value) in &block.storage_top_trie_changes {
                                if let Some(value) = value {
                                    finalized_block_storage.insert(key.clone(), value.clone());
                                } else {
                                    let _was_there = finalized_block_storage.remove(key);
                                    // TODO: if a block inserts a new value, then removes it in the next block, the key will remain in `finalized_block_storage`; either solve this or document this
                                    // assert!(_was_there.is_some());
                                }
                            }

                            if let Some(code) = block.storage_top_trie_changes.get(&b":code"[..]) {
                                let vm = smoldot::executor::host::HostVmPrototype::new(
                                    &code.as_ref().unwrap(),
                                    smoldot::executor::DEFAULT_HEAP_PAGES, // TODO:
                                    smoldot::executor::vm::ExecHint::Oneshot,
                                )
                                .unwrap();
                                let (runtime_spec, vm) =
                                    smoldot::executor::core_version(vm).unwrap();
                                finalized_runtime_version = runtime_spec.decode().spec_version;
                                finalized_metadata = {
                                    let query = smoldot::metadata::query_metadata(vm);
                                    loop {
                                        match query {
                                            smoldot::metadata::Query::StorageGet(_) => todo!(),
                                            smoldot::metadata::Query::Finished(Ok((
                                                metadata,
                                                _,
                                            ))) => break metadata,
                                            smoldot::metadata::Query::Finished(Err(err)) => {
                                                panic!("{}", err)
                                            }
                                        }
                                    }
                                };

                                new_metadata.insert(
                                    finalized_runtime_version,
                                    smoldot::json_rpc::methods::HexString(
                                        finalized_metadata.clone(),
                                    ),
                                );
                            } else if block.header.number == 1 {
                                new_metadata.insert(
                                    finalized_runtime_version,
                                    smoldot::json_rpc::methods::HexString(
                                        finalized_metadata.clone(),
                                    ),
                                );
                            }

                            let finalized_metadata =
                                smoldot::metadata::decode(&finalized_metadata).unwrap();
                            let events_storage_key =
                                smoldot::metadata::events::events_storage_key(finalized_metadata)
                                    .unwrap();
                            let events_encoded = if let Some(value) =
                                block.storage_top_trie_changes.get(&events_storage_key[..])
                            {
                                value
                            } else {
                                todo!()
                            };

                            blocks_save.push(ffi::DatabaseSaveBlock {
                                number: block.header.number,
                                runtime_spec: finalized_runtime_version,
                                events: smoldot::json_rpc::methods::HexString(
                                    events_encoded.clone().unwrap(),
                                ),
                            });
                        }

                        ffi::database_save(&ffi::DatabaseSave {
                            chain: &serialized_finalized_chain,
                            new_metadata,
                            blocks: blocks_save,
                        });
                    }

                    optimistic::ProcessOne::NewBest {
                        sync: s,
                        new_best_number,
                        ..
                    } => {
                        crate::yield_once().await;
                        crate::ffi::best_block_update(new_best_number);
                        process = s.process_one(unix_time);
                    }

                    optimistic::ProcessOne::FinalizedStorageGet(req) => {
                        let value = finalized_block_storage
                            .get(&req.key_as_vec())
                            .map(|v| &v[..]);
                        process = req.inject_value(value);
                    }
                    optimistic::ProcessOne::FinalizedStorageNextKey(req) => {
                        // TODO: to_vec() :-/
                        let req_key = req.key().as_ref().to_vec();
                        // TODO: to_vec() :-/
                        let next_key = finalized_block_storage
                            .range(req.key().as_ref().to_vec()..)
                            .find(move |(k, _)| k[..] > req_key[..])
                            .map(|(k, _)| k);
                        process = req.inject_key(next_key);
                    }
                    optimistic::ProcessOne::FinalizedStoragePrefixKeys(req) => {
                        // TODO: to_vec() :-/
                        let prefix = req.prefix().as_ref().to_vec();
                        // TODO: to_vec() :-/
                        let keys = finalized_block_storage
                            .range(req.prefix().as_ref().to_vec()..)
                            .take_while(|(k, _)| k.starts_with(&prefix))
                            .map(|(k, _)| k);
                        process = req.inject_keys(keys);
                    }
                }
            }

            // Start requests that need to be started.
            // Note that this is done after calling `process_one`, as the processing of pending
            // blocks can result in new requests but not the contrary.
            while let Some(action) = sync.next_request_action() {
                match action {
                    optimistic::RequestAction::Start {
                        start,
                        block_height,
                        source,
                        num_blocks,
                        ..
                    } => {
                        let block_request = network_service.clone().blocks_request(
                            source.clone(),
                            network_chain_index,
                            network::protocol::BlocksRequestConfig {
                                start: network::protocol::BlocksRequestConfigStart::Number(
                                    block_height,
                                ),
                                desired_count: num_blocks,
                                direction: network::protocol::BlocksRequestDirection::Ascending,
                                fields: network::protocol::BlocksRequestFields {
                                    header: true,
                                    body: true,
                                    justification: true,
                                },
                            },
                        );

                        let (rx, abort) = future::abortable(block_request);
                        let request_id = start.start(abort);
                        block_requests_finished.push(rx.map(move |r| (request_id, r)));
                    }
                    optimistic::RequestAction::Cancel { user_data, .. } => {
                        user_data.abort();
                    }
                }
            }

            futures::select! {
                network_event = from_network_service.next() => {
                    // Something happened on the network.

                    let network_event = match network_event {
                        Some(m) => m,
                        None => {
                            // The channel from the network service has been closed. Closing the
                            // sync background task as well.
                            return
                        },
                    };

                    match network_event {
                        network_service::Event::Connected { peer_id, chain_index, best_block_number, .. }
                            if chain_index == network_chain_index =>
                        {
                            let id = sync.add_source(peer_id.clone(), best_block_number);
                            peers_source_id_map.insert(peer_id.clone(), id);
                        },
                        network_service::Event::Disconnected { peer_id, chain_index }
                            if chain_index == network_chain_index =>
                        {
                            let id = peers_source_id_map.remove(&peer_id).unwrap();
                            let (_, rq_list) = sync.remove_source(id);
                            for (_, rq) in rq_list {
                                rq.abort();
                            }
                        },
                        network_service::Event::BlockAnnounce { chain_index, peer_id, announce }
                            if chain_index == network_chain_index =>
                        {
                            let id = *peers_source_id_map.get(&peer_id).unwrap();
                            sync.raise_source_best_block(id, announce.decode().header.number);
                        },
                        // Different chain index.
                        _ => {}
                    }
                },

                (request_id, result) = block_requests_finished.select_next_some() => {
                    // `result` is an error if the block request got cancelled by the sync state
                    // machine.
                    // TODO: clarify this piece of code
                    if let Ok(result) = result {
                        let result = result.map_err(|_| ());
                        let _ = sync.finish_request(request_id, result.map(|v| v.into_iter().map(|block| optimistic::RequestSuccessBlock {
                            scale_encoded_header: block.header.unwrap(), // TODO: don't unwrap
                            scale_encoded_extrinsics: block.body.unwrap(), // TODO: don't unwrap
                            scale_encoded_justification: block.justification,
                            user_data: (),
                        })).map_err(|()| optimistic::RequestFail::BlocksUnavailable));
                    }
                },
            }
        }
    }
}
