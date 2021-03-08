import React from 'react';
import { Grid, Typography } from '@material-ui/core';
import * as idb from 'idb/with-async-ittr.js';
import { Metadata } from '@polkadot/metadata';
import { TypeRegistry } from '@polkadot/types';
import { getSpecTypes, getSpecHasher, getSpecAlias, getSpecExtensions } from '@polkadot/types-known';

import * as smoldot from './../smoldot.js';
import { default as AccountViewer } from './AccountViewer.jsx';
import { default as Header } from './Header.jsx';

export default class extends React.Component {
    constructor(props) {
        super(props);
        this.state = {
            verifiedBlockHeight: null,
            savedBlockHeight: null,  // TODO: fill on startup?
            syncingPaused: true,  // TODO: false?
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

            this.smoldot = await smoldot.start({
                chain_spec: JSON.stringify(this.props.chainSpec),
                database_content: database_content,
                database_save_callback: (to_save) => {
                    // In order to avoid race conditions, each database save must wait for the
                    // previous one to have finished.
                    let prev = this.previousDatabaseSave || Promise.resolve(null);
                    this.previousDatabaseSave = (async () => {
                        await prev;
                        await this.blocksFromSmoldot(to_save);
                    })();
                },
                best_block_update_callback: (num) => {
                    this.setState({
                        verifiedBlockHeight: num,
                    });
                }
            });

            this.smoldot.set_syncing_paused(this.state.syncingPaused);
        })();
    }

    componentWillUnmount() {
        // TODO: somehow stop smoldot?
    }

    /// To call when smoldot sends back blocks to decode and save in database.
    async blocksFromSmoldot(to_save) {
        let blocksToStore = [];
        let eventsToStore = [];
        let savedBlockHeight = 0;

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
                this.registry = new TypeRegistry();
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

            if (block.number > savedBlockHeight) {
                savedBlockHeight = block.number;
            }
        }

        this.setState({
            savedBlockHeight: savedBlockHeight
        });

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

        console.log("Saved up to block #" + to_save.blocks[to_save.blocks.length - 1].number);
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
                <Header syncingPaused={this.state.syncingPaused} setSyncingPaused={(paused) => { this.smoldot.set_syncing_paused(paused); this.setState({ syncingPaused: paused }); }} verifiedBlockHeight={this.state.verifiedBlockHeight} savedBlockHeight={this.state.savedBlockHeight} chainName={this.props.chainSpec.name} />
                <AccountViewer chainSpec={this.props.chainSpec} database={this.state.database} />
            </Grid>
        );
    }
}
