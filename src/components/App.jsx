import React from 'react';
import { Box, Typography } from '@material-ui/core';
import * as idb from 'idb';

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
        idb.openDB('smoldot', 1, {
            upgrade(db) {
                db.createObjectStore('hashes_by_number', { keyPath: 'number' });
                const events = db.createObjectStore('events');
                events.createIndex('account', 'account', { unique: false });
                db.createObjectStore('timestamps', { keyPath: 'number' });
                db.createObjectStore('finalized_storage', { keyPath: 'key' });
            },
        }).then((database) => {
            this.smoldot = smoldot.start({
                chain_spec: JSON.stringify(this.state.chainSpec),
                database_content: null,
                database_save_callback: null,
                best_block_update_callback: (num) => {
                    this.setState({
                        currentBlockHeight: num,
                    });
                }
            });
        });

    }

    componentWillUnmount() {
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
