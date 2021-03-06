import React from 'react';
import { Box, Typography } from '@material-ui/core';
import * as idb from 'idb/with-async-ittr.js';
import { Metadata } from '@polkadot/metadata';
import { TypeRegistry } from '@polkadot/types';
import { getSpecTypes } from '@polkadot/types-known';

import * as smoldot from './../smoldot.js';
import { default as AccountInput } from './AccountInput.jsx';
import { default as NodeState } from './NodeState.jsx';

// TODO: good account for testing => 5GEkeVgLtxezLCYDKQ11LcUwotRQwxyDKJsWBx52CsZbLDQB

export default class extends React.Component {
    constructor(props) {
        super(props);
        this.state = {
            chainSpec: props.chainSpec,
            currentBlockHeight: null,
        };
    }

    componentDidMount() {
        (async () => {
            let database = await idb.openDB('polkadot-events-scraper', 1, {
                upgrade(db) {
                    const events = db.createObjectStore('events', { keyPath: ['block', 'recordIndex', 'argIndex'] });
                    events.createIndex('account', 'account', { unique: false });
                    db.createObjectStore('blocks', { keyPath: 'number' });
                    db.createObjectStore('metadata', { keyPath: 'runtime_spec' });
                    db.createObjectStore('meta');
                },
            });

            this.setState({
                database: database,
            });

            const database_content = await database.get('meta', 'chain');

            this.current_runtime_spec = null;
            this.registry = new TypeRegistry();

            this.smoldot = smoldot.start({
                chain_spec: JSON.stringify(this.state.chainSpec),
                database_content: database_content,
                database_save_callback: (to_save) => {
                    // TODO: can this be racy? should we wait for previous save to finish?
                    (async () => {
                        await this.blocksFromSmoldot(to_save);
                    })();
                },
                best_block_update_callback: (num) => {
                    this.setState({
                        currentBlockHeight: num,
                    });
                }
            });
        })();
    }

    componentWillUnmount() {
        // TODO: somehow stop smoldot?
    }

    /// To call when smoldot sends back blocks to decode and save in database.
    async blocksFromSmoldot(to_save) {
        let eventsToStore = [];

        for (const blockIndex in to_save.blocks) {
            const block = to_save.blocks[blockIndex];

            if (block.runtime_spec != this.current_runtime_spec) {
                this.current_runtime_spec = block.runtime_spec;

                let undecoded_metadata = to_save.new_metadata.find((m) => m.runtime_spec == block.runtime_spec);
                if (!undecoded_metadata) {
                    undecoded_metadata = await this.state.database.get('metadata', block.runtime_spec);
                }

                // TODO: finish here?
                //this.registry.setChainProperties(chainProps || this.registry.getChainProperties());
                //this.registry.setKnownTypes(this._options);
                // TODO: chain name
                this.registry.register(getSpecTypes(this.registry, 'Westend', undecoded_metadata.spec_name, block.runtime_spec));
                //this.registry.setHasher(getSpecHasher(this.registry, chain, version.specName));

                /*if (this.registry.knownTypes.typesBundle) {
                    this.registry.knownTypes.typesAlias = getSpecAlias(registry, chain, version.specName);
                }*/

                const metadata = new Metadata(this.registry, undecoded_metadata.metadata);
                this.registry.setMetadata(metadata, undefined, {
                    /*...getSpecExtensions(registry, chain, version.specName),
                    ...(this._options.signedExtensions || {})*/
                });
            }

            const eventRecords = this.registry.createType('Vec<EventRecord>', block.events);
            eventRecords.forEach((record, recordIndex) => {
                record.event.meta.args.forEach((arg, argIndex) => {
                    if (arg == 'AccountId') {
                        eventsToStore.push({
                            account: record.event.data[argIndex].toString(),
                            block: block.number,
                            recordIndex: recordIndex,
                            argIndex: argIndex,
                        });
                    }
                });
            })
        }

        // Store everything in the database.
        // This is done in a single transaction, in order to make sure that events aren't missed.
        const tx = this.state.database.transaction(['meta', 'metadata', 'blocks', 'events'], 'readwrite');
        let promises = [];
        promises.push(tx.objectStore('meta').put(to_save.chain, 'chain'));
        promises.push(...to_save.new_metadata.map((metadata) => {
            return tx.objectStore('metadata').put(metadata);
        }));
        promises.push(...to_save.blocks.map((block) => {
            return tx.objectStore('blocks').put(block);
        }));
        promises.push(...eventsToStore.map((event) => {
            return tx.objectStore('events').put(event);
        }));
        promises.push(tx.done);
        await Promise.all(promises);
    }

    render() {
        return (
            <Box>
                <Typography variant="h1">Polkadot events scraper</Typography>
                <NodeState blockHeight={this.state.currentBlockHeight} syncing="true" />
                <AccountInput setAddress={() => { }} />
            </Box>
        );
    }
}
