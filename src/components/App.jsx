import React from 'react';
import { Box, Typography } from '@material-ui/core';
import * as idb from 'idb/with-async-ittr.js';
import { TypeRegistry } from '@polkadot/types';
import { Metadata } from '@polkadot/metadata';

import * as smoldot from './../smoldot.js';
import { default as AccountInput } from './AccountInput.jsx';
import { default as NodeState } from './NodeState.jsx';

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
            let database = await idb.openDB('smoldot', 1, {
                upgrade(db) {
                    const events = db.createObjectStore('events');
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
        for (const blockIndex in to_save.blocks) {
            const block = to_save.blocks[blockIndex];

            if (block.runtime_spec != this.current_runtime_spec) {
                this.current_runtime_spec = block.runtime_spec;

                let undecoded_metadata = to_save.new_metadata.find((m) => m.runtime_spec == block.runtime_spec);
                if (!undecoded_metadata) {
                    undecoded_metadata = await this.state.database.get('metadata', block.runtime_spec);
                }

                const metadata = new Metadata(this.registry, undecoded_metadata.metadata);
                this.registry.setMetadata(metadata);
            }

            const eventRecords = this.registry.createType('Vec<EventRecord>', block.events, true);
            eventRecords.forEach((record) => {
                const data = record.event.data.toString();
                console.log(block.number, data, record.event.section, record.event.method);
            })
        }

        const tx = this.state.database.transaction(['meta', 'metadata', 'blocks'], 'readwrite');

        let promises = [];
        promises.push(tx.objectStore('meta').put(to_save.chain, 'chain'));
        promises.push(...to_save.new_metadata.map((metadata) => {
            return tx.objectStore('metadata').put(metadata);
        }));
        promises.push(...to_save.blocks.map((block) => {
            return tx.objectStore('blocks').put(block);
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
