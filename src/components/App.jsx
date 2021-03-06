import * as smoldot from './../smoldot.js';
import polkadot_chains_specs from './../westend.json'; // TODO: should be polkadot

import React, { useState, useEffect } from 'react';
import { Box, Typography } from '@material-ui/core';
import { default as AccountInput } from './AccountInput.jsx';
import { default as SyncingState } from './SyncingState.jsx';

export default () => {
    const [filterAddress, setFilterAddress] = useState(null);

    smoldot.start({
        chain_spec: JSON.stringify(polkadot_chains_specs),
        database_content: null,
        database_save_callback: null,
    });

    return (
        <Box>
            <Typography variant="h1">Polkadot events scraper</Typography>
            <SyncingState blockHeight="0" syncing="true" />
            <AccountInput setAddress={setFilterAddress} />
        </Box>
    );
};
