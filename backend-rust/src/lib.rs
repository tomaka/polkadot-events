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

#![recursion_limit = "512"]
#![deny(broken_intra_doc_links)]
#![deny(unused_crate_dependencies)]

use futures::{channel::mpsc, prelude::*};
use smoldot::{
    chain, chain_spec,
    libp2p::{multiaddr, peer_id::PeerId},
};
use std::{collections::BTreeMap, iter, sync::Arc};

pub mod ffi;

mod network_service;
mod sync_service;

// Use the default "system" allocator. In the context of Wasm, this uses the `dlmalloc` library.
// See <https://github.com/rust-lang/rust/tree/1.47.0/library/std/src/sys/wasm>.
//
// While the `wee_alloc` crate is usually the recommended choice in WebAssembly, testing has shown
// that using it makes memory usage explode from ~100MiB to ~2GiB and more (the environment then
// refuses to allocate 4GiB).
#[global_allocator]
static ALLOC: std::alloc::System = std::alloc::System;

pub struct ChainConfig {
    pub specification: String,
    pub database_content: Option<String>,
}

/// Starts a client running the given chain specifications.
pub async fn start_client(chain: ChainConfig, max_log_level: log::LevelFilter) {
    // Try initialize the logging and the panic hook.
    // Note that `start_client` can theoretically be called multiple times, meaning that these
    // calls shouldn't panic if reached multiple times.
    let _ =
        log::set_boxed_logger(Box::new(ffi::Logger)).map(|()| log::set_max_level(max_log_level));
    std::panic::set_hook(Box::new(|info| {
        ffi::throw(info.to_string());
    }));

    // Fool-proof check to make sure that randomness is properly implemented.
    assert_ne!(rand::random::<u64>(), 0);
    assert_ne!(rand::random::<u64>(), rand::random::<u64>());

    // Decode the chain specifications.
    let chain_spec = match chain_spec::ChainSpec::from_json_bytes(&chain.specification) {
        Ok(cs) => {
            log::info!("Loaded chain specs for {}", cs.name());
            cs
        }
        Err(err) => ffi::throw(format!("Error while opening chain specs: {}", err)),
    };

    // Load the information about the chains from the chain specs. If a light sync state is
    // present in the chain specs, it is possible to start sync at the finalized block it
    // describes.
    let genesis_chain_information =
        chain::chain_information::ChainInformation::from_genesis_storage(
            chain_spec.genesis_storage(),
        )
        .unwrap();

    // Any error while decoding is treated as if there was no database.
    let (chain_information, finalized_storage) =
        if let Some(database_content) = &chain.database_content {
            match smoldot::database::finalized_serialize::decode_chain(database_content) {
                Ok((parsed, Some(finalized_storage))) => ((parsed, Some(finalized_storage))),
                Ok((_, None)) => (genesis_chain_information.clone(), None),
                Err(error) => {
                    log::warn!("Failed to decode chain information: {}", error);
                    (genesis_chain_information.clone(), None)
                }
            }
        } else {
            (genesis_chain_information.clone(), None)
        };
    let finalized_storage = if let Some(s) = finalized_storage {
        // Note: the database decoding code returns a `HashMap` while we need a `BTreeMap`.
        // This is a small, mostly harmless, inefficiency.
        s.into_iter().collect()
    } else {
        let mut finalized_block_storage = BTreeMap::<Vec<u8>, Vec<u8>>::new();
        for (key, value) in chain_spec.genesis_storage() {
            finalized_block_storage.insert(key.to_owned(), value.to_owned());
        }
        finalized_block_storage
    };

    ffi::best_block_update(chain_information.finalized_block_header.number);

    // Starting here, the code below initializes the various "services" that make up the node.
    // Services need to be able to spawn asynchronous tasks on their own. Since "spawning a task"
    // isn't really something that a browser or Node environment can do efficiently, we instead
    // combine all the asynchronous tasks into one `FuturesUnordered` below.
    //
    // The `new_task_tx` and `new_task_rx` variables are used when spawning a new task is
    // required. Send a task on `new_task_tx` to start running it.
    let (new_task_tx, mut new_task_rx) = mpsc::unbounded();

    // The code below consists in spawning various services one by one. Services must be created
    // in a specific order, because some services must be passed an `Arc` to others.
    // One thing to be aware of, is that in order to start, a service might perform a request on
    // the other service(s) passed as parameter. These requests might in turn depend on background
    // tasks being spawned. For this reason, the tasks sent to `new_task_tx` must be start running
    // as soon as possible, and not be delayed.
    // This is why we spawn the entire initialization through `new_task_tx` instead of directly
    // using `await`.
    new_task_tx
        .clone()
        .unbounded_send(
            async move {
                // The network service is responsible for connecting to the peer-to-peer network
                // of the chain.
                let (network_service, mut network_event_receivers) =
                    network_service::NetworkService::new(network_service::Config {
                        tasks_executor: Box::new({
                            let new_task_tx = new_task_tx.clone();
                            move |fut| new_task_tx.unbounded_send(fut).unwrap()
                        }),
                        num_events_receivers: 1, // Configures the length of `network_event_receivers`
                        chains: iter::once(network_service::ConfigChain {
                            bootstrap_nodes: {
                                let mut list = Vec::with_capacity(chain_spec.boot_nodes().len());
                                for node in chain_spec.boot_nodes() {
                                    let mut address: multiaddr::Multiaddr = node.parse().unwrap(); // TODO: don't unwrap?
                                    if let Some(multiaddr::Protocol::P2p(peer_id)) = address.pop() {
                                        let peer_id = PeerId::from_multihash(peer_id).unwrap(); // TODO: don't unwrap
                                        list.push((peer_id, address));
                                    } else {
                                        panic!() // TODO:
                                    }
                                }
                                list
                            },
                            has_grandpa_protocol: matches!(
                                chain_information.finality,
                                chain::chain_information::ChainInformationFinality::Grandpa { .. }
                            ),
                            genesis_block_hash: genesis_chain_information
                                .finalized_block_header
                                .hash(),
                            best_block: (
                                chain_information.finalized_block_header.number,
                                chain_information.finalized_block_header.hash(),
                            ),
                            protocol_id: chain_spec.protocol_id().to_string(),
                        })
                        .collect(),
                    })
                    .await;

                // The sync service is leveraging the network service, downloads block headers,
                // and verifies them, to determine what are the best and finalized blocks of the
                // chain.
                let sync_service = Arc::new(
                    sync_service::SyncService::new(sync_service::Config {
                        chain_information,
                        finalized_storage,
                        tasks_executor: Box::new({
                            let new_task_tx = new_task_tx.clone();
                            move |fut| new_task_tx.unbounded_send(fut).unwrap()
                        }),
                        network_service: (network_service.clone(), 0),
                        network_events_receiver: network_event_receivers.pop().unwrap(),
                    })
                    .await,
                );

                log::info!("Initialization complete");
            }
            .boxed(),
        )
        .unwrap();

    // This is the main future that executes the entire client.
    let mut all_tasks = stream::FuturesUnordered::new();
    async move {
        // Since `all_tasks` is initially empty, polling it would produce `None` and immediately
        // interrupt the processing.
        // As such, we start by filling it with the initial content of the `new_task` channel.
        while let Some(Some(task)) = new_task_rx.next().now_or_never() {
            all_tasks.push(task);
        }

        loop {
            match future::select(new_task_rx.select_next_some(), all_tasks.next()).await {
                future::Either::Left((new_task, _)) => {
                    all_tasks.push(new_task);
                }
                future::Either::Right((Some(()), _)) => {}
                future::Either::Right((None, _)) => {
                    log::info!("All tasks complete. Stopping client.");
                    break;
                }
            }
        }
    }
    .await
}

/// Use in an asynchronous context to interrupt the current task execution and schedule it back.
///
/// This function is useful in order to guarantee a fine granularity of tasks execution time in
/// situations where a CPU-heavy task is being performed.
async fn yield_once() {
    let mut pending = true;
    futures::future::poll_fn(move |cx| {
        if pending {
            pending = false;
            cx.waker().wake_by_ref();
            core::task::Poll::Pending
        } else {
            core::task::Poll::Ready(())
        }
    })
    .await
}
