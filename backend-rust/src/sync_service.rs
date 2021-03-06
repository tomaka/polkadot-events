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
use smoldot::{chain::chain_information, executor, libp2p, network, sync::optimistic};
use std::{collections::BTreeMap, sync::Arc};

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
pub struct SyncService {}

impl SyncService {
    /// Initializes the [`SyncService`] with the given configuration.
    pub async fn new(mut config: Config) -> Arc<Self> {
        let (to_database, messages_rx) = mpsc::channel(4);

        (config.tasks_executor)(Box::pin(start_sync(
            config.chain_information,
            config.finalized_storage,
            config.network_service.0,
            config.network_service.1,
            config.network_events_receiver,
            to_database,
        )));

        (config.tasks_executor)(Box::pin(start_database_write(messages_rx)));

        Arc::new(SyncService {})
    }
}

enum ToDatabase {
    FinalizedBlocks(Vec<optimistic::Block<()>>),
}

/// Returns the background task of the sync service.
fn start_sync(
    initial_chain_information: chain_information::ChainInformation,
    initial_finalized_storage: BTreeMap<Vec<u8>, Vec<u8>>,
    network_service: Arc<network_service::NetworkService>,
    network_chain_index: usize,
    mut from_network_service: mpsc::Receiver<network_service::Event>,
    mut to_database: mpsc::Sender<ToDatabase>,
) -> impl Future<Output = ()> {
    // Holds, in parallel of the database, the storage of the latest finalized block.
    // At the time of writing, this state is stable around ~3MiB for Polkadot, meaning that it is
    // completely acceptable to hold it entirely in memory.
    let mut finalized_block_storage = initial_finalized_storage;

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
                        process = s.process_one(unix_time);

                        // TODO: maybe write in a separate task? but then we can't access the finalized storage immediately after?
                        for block in &finalized_blocks {
                            for (key, value) in &block.storage_top_trie_changes {
                                if let Some(value) = value {
                                    finalized_block_storage.insert(key.clone(), value.clone());
                                } else {
                                    let _was_there = finalized_block_storage.remove(key);
                                    // TODO: if a block inserts a new value, then removes it in the next block, the key will remain in `finalized_block_storage`; either solve this or document this
                                    // assert!(_was_there.is_some());
                                }
                            }
                        }

                        to_database
                            .send(ToDatabase::FinalizedBlocks(finalized_blocks))
                            .await
                            .unwrap();
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

/// Starts the task that writes blocks to the database.
async fn start_database_write(mut messages_rx: mpsc::Receiver<ToDatabase>) {
    // TODO: restore
    while let Some(_) = messages_rx.next().await {}

    /*let finalized_block_hash = database.finalized_block_hash().unwrap();

    let vm = smoldot::executor::host::HostVmPrototype::new(
        &database
            .finalized_block_storage_top_trie_get(&finalized_block_hash, b":code")
            .unwrap()
            .unwrap()
            .as_ref(),
        smoldot::executor::storage_heap_pages_to_value(
            database
                .finalized_block_storage_top_trie_get(&finalized_block_hash, b":heappages")
                .unwrap()
                .as_ref()
                .map(|v| v.as_ref()),
        )
        .unwrap(),
        smoldot::executor::vm::ExecHint::Oneshot,
    )
    .unwrap();
    let (runtime_spec, vm) = smoldot::executor::core_version(vm).unwrap();
    let mut finalized_runtime_version = runtime_spec.decode().spec_version;
    let mut finalized_metadata = smoldot::metadata::metadata_from_virtual_machine_prototype(vm)
        .unwrap()
        .0;

    let mut events_database = if let Some(events_db) = major_sync_export_events_sqlite.as_ref() {
        let mut insert_metadata = events_db
            .prepare(
                "
                INSERT INTO metadata(runtime_version, metadata)
                VALUES(?, ?)
                ON CONFLICT(runtime_version) DO UPDATE SET metadata=excluded.metadata
            ",
            )
            .unwrap();

        insert_metadata
            .bind(1, i64::try_from(finalized_runtime_version).unwrap())
            .unwrap();
        insert_metadata.bind(2, &finalized_metadata[..]).unwrap();
        insert_metadata.next().unwrap();

        let insert_events = events_db
            .prepare(
                "
                INSERT INTO events(block_height, runtime_version, events_storage)
                VALUES(?, ?, ?)
                ON CONFLICT(block_height) DO UPDATE
                SET runtime_version=excluded.runtime_version, events_storage=excluded.events_storage
            ",
            )
            .unwrap();

        Some((
            SqliteStmtSendHack(insert_events),
            SqliteStmtSendHack(insert_metadata),
        ))
    } else {
        None
    };

    loop {
        match messages_rx.next().await {
            None => break,
            Some(ToDatabase::FinalizedBlocks(finalized_blocks)) => {
                let new_finalized_hash = if let Some(last_finalized) = finalized_blocks.last() {
                    Some(last_finalized.header.hash())
                } else {
                    None
                };

                for block in finalized_blocks {
                    if let Some((insert_events, insert_metadata)) = events_database.as_mut() {
                        if let Some(code) = block.storage_top_trie_changes.get(&b":code"[..]) {
                            let vm = smoldot::executor::host::HostVmPrototype::new(
                                &code.as_ref().unwrap(),
                                smoldot::executor::DEFAULT_HEAP_PAGES, // TODO:
                                smoldot::executor::vm::ExecHint::Oneshot,
                            )
                            .unwrap();
                            let (runtime_spec, vm) = smoldot::executor::core_version(vm).unwrap();
                            finalized_runtime_version = runtime_spec.decode().spec_version;
                            finalized_metadata =
                                smoldot::metadata::metadata_from_virtual_machine_prototype(vm)
                                    .unwrap()
                                    .0;

                            insert_metadata.reset().unwrap();
                            insert_metadata
                                .bind(1, i64::try_from(finalized_runtime_version).unwrap())
                                .unwrap();
                            insert_metadata.bind(2, &finalized_metadata[..]).unwrap();
                            insert_metadata.next().unwrap();
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

                        insert_events.reset().unwrap();
                        insert_events
                            .bind(1, i64::try_from(block.header.number).unwrap())
                            .unwrap();
                        insert_events
                            .bind(2, i64::try_from(finalized_runtime_version).unwrap())
                            .unwrap();
                        insert_events
                            .bind(3, &events_encoded.as_ref().unwrap()[..])
                            .unwrap();
                        insert_events.next().unwrap();
                    }

                    // TODO: overhead for building the SCALE encoding of the header
                    let result = database.insert(
                        &block.header.scale_encoding().fold(Vec::new(), |mut a, b| {
                            a.extend_from_slice(b.as_ref());
                            a
                        }),
                        true, // TODO: is_new_best?
                        block.body.iter(),
                        block
                            .storage_top_trie_changes
                            .iter()
                            .map(|(k, v)| (k, v.as_ref())),
                    );

                    match result {
                        Ok(()) => {}
                        Err(full_sled::InsertError::Duplicate) => {} // TODO: this should be an error ; right now we silence them because non-finalized blocks aren't loaded from the database at startup, resulting in them being downloaded again
                        Err(err) => panic!("{}", err),
                    }
                }

                if let Some(new_finalized_hash) = new_finalized_hash {
                    database.set_finalized(&new_finalized_hash).unwrap();
                }
            }
        }
    }*/
}
