import React from 'react';
import TextField from '@material-ui/core/TextField';
import { Box, Typography } from '@material-ui/core';
import { default as AccountInput } from './AccountInput.jsx';
import { default as SyncingState } from './SyncingState.jsx';

export default React.memo((props) => {
    return (
        <Box>
            <Typography variant="h1">Polkadot events scraper</Typography>
            <SyncingState blockHeight="0" syncing="true" />
            <AccountInput />
        </Box>
    );
});
