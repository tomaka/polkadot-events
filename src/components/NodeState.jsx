import React from 'react';
import { Box, FormControlLabel, Switch, Tooltip, Typography } from '@material-ui/core';

export default React.memo(({ syncingPaused, setSyncingPaused, blockHeight, chainName }) => {
    return (
        <Box>
            <Tooltip title="Stops downloading new blocks. Since they are processed asynchronously, blocks in queue are still being processed." arrow>
                <FormControlLabel
                    control={
                        <Switch
                            checked={syncingPaused}
                            onChange={(event) => {
                                let paused = event.target.checked;
                                setSyncingPaused(paused);
                            }}
                        />
                    }
                    label="Pause"
                />
            </Tooltip>
            <Typography>{chainName}</Typography>
            <Tooltip title="The node is downloading blocks from the network and verifying them." arrow>
                <Typography>Current block: {`${blockHeight ? ('#' + blockHeight) : '<unknown>'}`}</Typography>
            </Tooltip>
        </Box>
    );
});
