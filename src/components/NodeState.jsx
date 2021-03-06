import React from 'react';
import { Box, CircularProgress, Typography } from '@material-ui/core';

export default React.memo(({ syncing, blockHeight }) => {
    const syncingText = syncing ? 'Syncing' : 'Idle';

    return (
        <Box>
            {syncing && <CircularProgress size="1em" />} <Typography>{`${syncingText}`}</Typography>
            <Typography>Current block: {`${blockHeight ? ('#' + blockHeight) : '<unknown>'}`}</Typography>
        </Box>
    );
});
