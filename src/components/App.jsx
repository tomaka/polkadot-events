import React from 'react';
import { Grid, Typography } from '@material-ui/core';
import * as idb from 'idb/with-async-ittr.js';
import { Metadata } from '@polkadot/metadata';
import { TypeRegistry } from '@polkadot/types';
import { getSpecTypes, getSpecHasher, getSpecAlias, getSpecExtensions } from '@polkadot/types-known';

import * as smoldot from './../smoldot.js';
import { default as AccountViewer } from './AccountViewer.jsx';
import { default as NodeState } from './NodeState.jsx';

export default class extends React.Component {
    constructor(props) {
        super(props);
        this.state = {
            currentBlockHeight: null,
            syncingPaused: false,
        };
    }

    componentDidMount() {
        (async () => {
            let database = await idb.openDB('events-scraper-' + this.props.chainSpec.id, 1, {
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

            this.smoldot = await smoldot.start({
                chain_spec: JSON.stringify(this.props.chainSpec),
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

            this.smoldot.set_syncing_paused(this.state.paused);
        })();
    }

    componentWillUnmount() {
        // TODO: somehow stop smoldot?
    }

    /// To call when smoldot sends back blocks to decode and save in database.
    async blocksFromSmoldot(to_save) {
        let blocksToStore = [];
        let eventsToStore = [];

        for (const blockIndex in to_save.blocks) {
            const block = to_save.blocks[blockIndex];
            let includeBlock = false;

            if (block.runtime_spec != this.current_runtime_spec) {
                this.current_runtime_spec = block.runtime_spec;

                let undecoded_metadata = to_save.new_metadata.find((m) => m.runtime_spec == block.runtime_spec);
                if (!undecoded_metadata) {
                    undecoded_metadata = await this.state.database.get('metadata', block.runtime_spec);
                }

                // Inspired from https://github.com/polkadot-js/api/blob/ec76b11666aea72135f899267abf784fc6309156/packages/api/src/base/Init.ts#L83
                this.registry.setChainProperties(this.props.chainSpec.properties);
                this.registry.register(getSpecTypes(this.registry, this.props.chainSpec.name, undecoded_metadata.spec_name, block.runtime_spec));
                this.registry.setHasher(getSpecHasher(this.registry, this.props.chainSpec.name, undecoded_metadata.spec_name));
                if (this.registry.knownTypes.typesBundle) {
                    this.registry.knownTypes.typesAlias = getSpecAlias(this.registry, this.props.chainSpec.name, undecoded_metadata.spec_name);
                }
                const metadata = new Metadata(this.registry, undecoded_metadata.metadata);
                this.registry.setMetadata(metadata, undefined, getSpecExtensions(this.registry, this.props.chainSpec.name, undecoded_metadata.spec_name));
            }

            // TODO: properly figure out and/or handle decode failures
            const eventRecords = this.registry.createType('Vec<EventRecord>', block.events);
            eventRecords.forEach((record, recordIndex) => {
                record.event.meta.args.forEach((arg, argIndex) => {
                    if (arg == 'AccountId') {
                        includeBlock = true;
                        eventsToStore.push({
                            account: record.event.data[argIndex].toString(),
                            block: block.number,
                            recordIndex: recordIndex,
                            argIndex: argIndex,
                        });
                    }
                });
            });

            if (includeBlock) {
                blocksToStore.push(block);
            }
        }

        // Store everything in the database.
        // This is done in a single transaction, in order to make sure that events aren't missed.
        const tx = this.state.database.transaction(['meta', 'metadata', 'blocks', 'events'], 'readwrite');
        let promises = [];
        promises.push(tx.objectStore('meta').put(to_save.chain, 'chain'));
        promises.push(...to_save.new_metadata.map((metadata) => {
            return tx.objectStore('metadata').put(metadata);
        }));
        promises.push(...blocksToStore.map((block) => {
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
            <Grid
                container
                direction="column"
                justify="center"
                alignItems="stretch"
            >
                <Typography variant="h1">Polkadot events scraper</Typography>
                <NodeState syncingPaused={this.state.syncingPaused} setSyncingPaused={(paused) => { this.smoldot.set_syncing_paused(paused); this.setState({ syncingPaused: paused }); }} blockHeight={this.state.currentBlockHeight} chainName={this.props.chainSpec.name} />
                <AccountViewer database={this.state.database} />
            </Grid>
        );
    }
}
