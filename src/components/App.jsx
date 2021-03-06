import React from 'react';
import { Box, Typography } from '@material-ui/core';
import * as idb from 'idb/with-async-ittr.js';

import * as smoldot from './../smoldot.js';
import { default as AccountInput } from './AccountInput.jsx';
import { default as NodeState } from './NodeState.jsx';

export default class extends React.Component {
    constructor(props) {
        super(props);
        this.state = {
            chainSpec: props.chainSpec,
            currentBlockHeight: 0,
        };
    }

    componentDidMount() {
        (async () => {
            let database = await idb.openDB('smoldot', 1, {
                upgrade(db) {
                    const events = db.createObjectStore('events');
                    events.createIndex('account', 'account', { unique: false });
                    db.createObjectStore('blocks', { keyPath: 'number' });
                    db.createObjectStore('metadata', { keyPath: 'runtime_version' });
                    db.createObjectStore('meta');
                },
            });
            
            const database_content = await database.get('meta', 'chain');

            this.smoldot = smoldot.start({
                chain_spec: JSON.stringify(this.state.chainSpec),
                database_content: database_content,
                database_save_callback: null,
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
