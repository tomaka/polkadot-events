import React from 'react';
import { Box, Typography } from '@material-ui/core';

export default React.memo(({ syncing }) => {
    const syncingText = syncing ? 'Syncing' : 'Idle';

    return (
        <Box>
            <Typography>
                {`${syncingText}`}
            </Typography>
        </Box>
    );
});
