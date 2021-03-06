import * as smoldot from './../smoldot.js';
import polkadot_chains_specs from './../westend.json'; // TODO: should be polkadot

import React, { useState, useEffect } from 'react';
import { Box, Typography } from '@material-ui/core';
import { default as AccountInput } from './AccountInput.jsx';
import { default as SyncingState } from './SyncingState.jsx';

export default class extends React.Component {
    constructor(props) {
        super(props);
        this.state = {
            currentBlockHeight: 0,
        };
    }

    componentDidMount() {
        this.smoldot = smoldot.start({
            chain_spec: JSON.stringify(polkadot_chains_specs),
            database_content: null,
            database_save_callback: null,
            best_block_update_callback: (num) => {
                this.setState({
                    currentBlockHeight: num,
                });
            }
        });
    }

    componentWillUnmount() {
    }

    render() {
        return (
            <Box>
                <Typography variant="h1">Polkadot events scraper</Typography>
                <SyncingState blockHeight={this.state.currentBlockHeight} syncing="true" />
                <AccountInput setAddress={() => {}} />
            </Box>
        );
    }
}
