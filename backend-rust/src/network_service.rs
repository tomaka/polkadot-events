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

use crate::ffi;

use core::{num::NonZeroUsize, pin::Pin, time::Duration};
use futures::{channel::mpsc, lock::Mutex, prelude::*};
use smoldot::{
    informant::HashDisplay,
    libp2p::{connection, multiaddr::Multiaddr, peer_id::PeerId},
    network::{protocol, service},
};
use std::{collections::HashSet, sync::Arc};

/// Configuration for a [`NetworkService`].
pub struct Config {
    /// Closure that spawns background tasks.
    pub tasks_executor: Box<dyn FnMut(Pin<Box<dyn Future<Output = ()> + Send>>) + Send>,

    /// Number of event receivers returned by [`NetworkService::new`].
    pub num_events_receivers: usize,

    /// List of chains to connect to. Chains are later referred to by their index in this list.
    pub chains: Vec<ConfigChain>,
}

/// See [`Config::chains`].
pub struct ConfigChain {
    /// List of node identities and addresses that are known to belong to the chain's peer-to-pee
    /// network.
    pub bootstrap_nodes: Vec<(PeerId, Multiaddr)>,

    /// Hash of the genesis block of the chain. Sent to other nodes in order to determine whether
    /// the chains match.
    pub genesis_block_hash: [u8; 32],

    /// Number and hash of the current best block. Can later be updated with // TODO: which function?
    pub best_block: (u64, [u8; 32]),

    /// Identifier of the chain to connect to.
    ///
    /// Each blockchain has (or should have) a different "protocol id". This value identifies the
    /// chain, so as to not introduce conflicts in the networking messages.
    pub protocol_id: String,

    /// If true, the chain uses the GrandPa networking protocol.
    pub has_grandpa_protocol: bool,
}

pub struct NetworkService {
    /// Fields behind a mutex.
    guarded: Mutex<Guarded>,

    /// Data structure holding the entire state of the networking.
    network: service::ChainNetwork<ffi::Instant, (), ()>,

    /// List of nodes that are considered as important for logging purposes.
    // TODO: should also detect whenever we fail to open a block announces substream with any of these peers
    important_nodes: HashSet<PeerId, fnv::FnvBuildHasher>,
}

/// Fields of [`NetworkService`] behind a mutex.
struct Guarded {
    /// See [`Config::tasks_executor`].
    tasks_executor: Box<dyn FnMut(Pin<Box<dyn Future<Output = ()> + Send>>) + Send>,
}

impl NetworkService {
    /// Initializes the network service with the given configuration.
    ///
    /// Returns the networking service, plus a list of receivers on which events are pushed.
    /// All of these receivers must be polled regularly to prevent the networking service from
    /// slowing down.
    pub async fn new(config: Config) -> (Arc<Self>, Vec<mpsc::Receiver<Event>>) {
        let (mut senders, receivers): (Vec<_>, Vec<_>) = (0..config.num_events_receivers)
            .map(|_| mpsc::channel(16))
            .unzip();

        let important_nodes = config
            .chains
            .iter()
            .flat_map(|chain| chain.bootstrap_nodes.iter())
            .map(|(peer_id, _)| peer_id.clone())
            .collect::<HashSet<_, _>>();

        let num_chains = config.chains.len();
        let mut chains = Vec::with_capacity(num_chains);
        // TODO: this `bootstrap_nodes` field is weird ; should we de-duplicate entry in known_nodes?
        let mut known_nodes = Vec::new();

        for chain in config.chains {
            chains.push(service::ChainConfig {
                bootstrap_nodes: (known_nodes.len()
                    ..(known_nodes.len() + chain.bootstrap_nodes.len()))
                    .collect(),
                in_slots: 25,
                out_slots: 25,
                grandpa_protocol_config: if chain.has_grandpa_protocol {
                    // TODO: dummy values
                    Some(service::GrandpaState {
                        commit_finalized_height: 0,
                        round_number: 1,
                        set_id: 0,
                    })
                } else {
                    None
                },
                protocol_id: chain.protocol_id.clone(),
                best_hash: chain.best_block.1,
                best_number: chain.best_block.0,
                genesis_hash: chain.genesis_block_hash,
                role: protocol::Role::Light,
            });

            known_nodes.extend(
                chain
                    .bootstrap_nodes
                    .into_iter()
                    .map(|(peer_id, addr)| ((), peer_id, addr)),
            );
        }

        let network_service = Arc::new(NetworkService {
            guarded: Mutex::new(Guarded {
                tasks_executor: config.tasks_executor,
            }),
            network: service::ChainNetwork::new(service::Config {
                chains,
                known_nodes,
                listen_addresses: Vec::new(), // TODO:
                noise_key: connection::NoiseKey::new(&rand::random()),
                // TODO: we use an abnormally large channel in order to by pass https://github.com/paritytech/smoldot/issues/615
                // once the issue is solved, this should be restored to a smaller value, such as 16
                pending_api_events_buffer_size: NonZeroUsize::new(2048).unwrap(),
                randomness_seed: rand::random(),
            }),
            important_nodes,
        });

        // Spawn a task pulling events from the network and transmitting them to the event senders.
        (network_service.guarded.try_lock().unwrap().tasks_executor)(Box::pin({
            // TODO: keeping a Weak here doesn't really work to shut down tasks
            let network_service = Arc::downgrade(&network_service);
            async move {
                loop {
                    let event = loop {
                        let network_service = match network_service.upgrade() {
                            Some(ns) => ns,
                            None => {
                                return;
                            }
                        };

                        match network_service.network.next_event().await {
                            service::Event::Connected(peer_id) => {
                                log::info!(target: "network", "Connected to {}", peer_id);
                            }
                            service::Event::Disconnected {
                                peer_id,
                                chain_indices,
                            } => {
                                log::info!(target: "network", "Disconnected from {} (chains: {:?})", peer_id, chain_indices);
                                if !chain_indices.is_empty() {
                                    // TODO: properly implement when multiple chains
                                    if chain_indices.len() == 1 {
                                        break Event::Disconnected {
                                            peer_id,
                                            chain_index: chain_indices[0],
                                        };
                                    } else {
                                        todo!()
                                    }
                                }
                            }
                            service::Event::BlockAnnounce {
                                chain_index,
                                peer_id,
                                announce,
                            } => {
                                log::debug!(
                                    target: "network",
                                    "Connection({}) => BlockAnnounce({}, {}, is_best={})",
                                    peer_id,
                                    chain_index,
                                    HashDisplay(&announce.decode().header.hash()),
                                    announce.decode().is_best
                                );
                                break Event::BlockAnnounce {
                                    chain_index,
                                    peer_id,
                                    announce,
                                };
                            }
                            service::Event::ChainConnected {
                                peer_id,
                                chain_index,
                                best_number,
                                best_hash,
                                ..
                            } => {
                                log::debug!(
                                    target: "network",
                                    "Connection({}) => ChainConnected({}, {}, {})",
                                    peer_id,
                                    chain_index,
                                    best_number,
                                    HashDisplay(&best_hash)
                                );
                                break Event::Connected {
                                    peer_id,
                                    chain_index,
                                    best_block_number: best_number,
                                    best_block_hash: best_hash,
                                };
                            }
                            service::Event::ChainDisconnected {
                                peer_id,
                                chain_index,
                            } => {
                                log::debug!(
                                    target: "network",
                                    "Connection({}) => ChainDisconnected({})",
                                    peer_id,
                                    chain_index,
                                );
                                break Event::Disconnected {
                                    peer_id,
                                    chain_index,
                                };
                            }
                            service::Event::IdentifyRequestIn { peer_id, request } => {
                                log::debug!(
                                    target: "network",
                                    "Connection({}) => IdentifyRequest",
                                    peer_id,
                                );
                                request.respond("smoldot").await;
                            }
                        }
                    };

                    // Dispatch the event to the various senders.
                    // This little `if` avoids having to do `event.clone()` if we don't have to.
                    if senders.len() == 1 {
                        let _ = senders[0].send(event).await;
                    } else {
                        for sender in &mut senders {
                            let _ = sender.send(event.clone()).await;
                        }
                    }
                }
            }
        }));

        // Spawn tasks dedicated to opening connections.
        // TODO: spawn several, or do things asynchronously, so that we try open multiple connections simultaneously
        for chain_index in 0..num_chains {
            (network_service.guarded.try_lock().unwrap().tasks_executor)(Box::pin({
                // TODO: keeping a Weak here doesn't really work to shut down tasks
                let network_service = Arc::downgrade(&network_service);
                async move {
                    loop {
                        // TODO: very crappy way of not spamming the network service ; instead we should wake this task up when a disconnect or a discovery happens
                        ffi::Delay::new(Duration::from_secs(1)).await;

                        let network_service = match network_service.upgrade() {
                            Some(ns) => ns,
                            None => {
                                return;
                            }
                        };

                        let start_connect =
                            match network_service.network.fill_out_slots(chain_index).await {
                                Some(sc) => sc,
                                None => continue,
                            };

                        let is_important_peer = network_service
                            .important_nodes
                            .contains(&start_connect.expected_peer_id);

                        // Convert the `multiaddr` (typically of the form `/ip4/a.b.c.d/tcp/d/ws`)
                        // into a `Future<dyn Output = Result<TcpStream, ...>>`.
                        let socket = {
                            log::debug!(target: "connections", "Pending({:?}) started: {}", start_connect.id, start_connect.multiaddr);
                            ffi::Connection::connect(&start_connect.multiaddr.to_string())
                        };

                        // TODO: handle dialing timeout here

                        let network_service2 = network_service.clone();
                        (network_service.guarded.lock().await.tasks_executor)(Box::pin({
                            connection_task(
                                socket,
                                network_service2,
                                start_connect.id,
                                start_connect.expected_peer_id,
                                is_important_peer,
                            )
                        }));
                    }
                }
            }));
        }

        (network_service.guarded.try_lock().unwrap().tasks_executor)(Box::pin({
            // TODO: keeping a Weak here doesn't really work to shut down tasks
            let network_service = Arc::downgrade(&network_service);
            async move {
                loop {
                    let network_service = match network_service.upgrade() {
                        Some(ns) => ns,
                        None => {
                            return;
                        }
                    };

                    network_service
                        .network
                        .next_substream()
                        .await
                        .open(ffi::Instant::now())
                        .await;
                }
            }
        }));

        (network_service, receivers)
    }

    /// Sends a blocks request to the given peer.
    // TODO: more docs
    pub async fn blocks_request(
        self: Arc<Self>,
        target: PeerId,
        chain_index: usize,
        config: protocol::BlocksRequestConfig,
    ) -> Result<Vec<protocol::BlockData>, service::BlocksRequestError> {
        log::debug!(target: "network", "Connection({}) <= BlocksRequest({:?})", target, config);

        let result = self
            .network
            .blocks_request(ffi::Instant::now(), target.clone(), chain_index, config)
            .await;

        log::debug!(
            target: "network",
            "Connection({}) => BlocksRequest({:?})",
            target,
            result.as_ref().map(|b| b.len())
        );

        result
    }
}

/// Event that can happen on the network service.
#[derive(Debug, Clone)]
pub enum Event {
    Connected {
        peer_id: PeerId,
        chain_index: usize,
        best_block_number: u64,
        best_block_hash: [u8; 32],
    },
    Disconnected {
        peer_id: PeerId,
        chain_index: usize,
    },
    BlockAnnounce {
        peer_id: PeerId,
        chain_index: usize,
        announce: service::EncodedBlockAnnounce,
    },
}

/// Asynchronous task managing a specific connection.
///
/// `is_important_peer` controls the log level used for problems that happen on this connection.
async fn connection_task(
    websocket: impl Future<Output = Result<Pin<Box<ffi::Connection>>, ()>>,
    network_service: Arc<NetworkService>,
    pending_id: service::PendingId,
    expected_peer_id: PeerId,
    is_important_peer: bool,
) {
    // Finishing the ongoing connection process.
    let mut websocket = match websocket.await {
        Ok(s) => s,
        Err(()) => {
            log::debug!(
                target: "connections",
                "Pending({:?}, {}) => Failed to reach",
                pending_id, expected_peer_id,
            );

            network_service
                .network
                .pending_outcome_err(pending_id)
                .await;

            return;
        }
    };

    let id = network_service
        .network
        .pending_outcome_ok(pending_id, ())
        .await;

    log::debug!(
        target: "connections",
        "Pending({:?}, {}) => Connection({:?})",
        pending_id,
        expected_peer_id,
        id
    );

    let mut write_buffer = vec![0; 4096];

    loop {
        let read_buffer = websocket.read_buffer().now_or_never().unwrap_or(Some(&[]));

        let now = ffi::Instant::now();

        let read_write = match network_service
            .network
            .read_write(id, now, read_buffer, (&mut write_buffer, &mut []))
            .await
        {
            Ok(rw) => rw,
            Err(_err) => {
                if is_important_peer {
                    log::warn!(target: "connections", "Error in connection with {}: {}", expected_peer_id, _err);
                } else {
                    log::debug!(target: "connections", "Connection({:?}, {}) => Closed: {}", id, expected_peer_id, _err);
                }

                return;
            }
        };

        let read_buffer_has_data = read_buffer.map_or(false, |b| !b.is_empty());
        let read_buffer_closed = read_buffer.is_none();

        if read_write.write_close && read_buffer_closed {
            log::debug!(target: "connections", "Connection({:?}, {}) => Closed gracefully", id, expected_peer_id);
            return;
        }

        if read_write.written_bytes != 0 {
            websocket.send(&write_buffer[..read_write.written_bytes]);
        }

        websocket.advance_read_cursor(read_write.read_bytes);

        // Starting from here, we block (or not) the current task until more processing needs
        // to happen.

        // Future ready when the connection state machine requests more processing.
        let poll_after = if let Some(wake_up) = read_write.wake_up_after {
            if wake_up > now {
                let dur = wake_up - now;
                future::Either::Left(ffi::Delay::new(dur))
            } else {
                continue;
            }
        } else {
            future::Either::Right(future::pending())
        }
        .fuse();

        // Future that is woken up when new data is ready on the socket.
        let read_buffer_ready =
            if !(read_buffer_has_data && read_write.read_bytes == 0) && !read_buffer_closed {
                future::Either::Left(websocket.read_buffer())
            } else {
                future::Either::Right(future::pending())
            };

        // Wait until either some data is ready on the socket, or the connection state machine
        // has been requested to be polled again.
        futures::pin_mut!(read_buffer_ready);
        future::select(
            future::select(read_buffer_ready, read_write.wake_up_future),
            poll_after,
        )
        .await;
    }
}
