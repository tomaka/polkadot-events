import React from 'react';
import { Box, FormControlLabel, Switch, Tooltip, Typography } from '@material-ui/core';

export default React.memo(({ syncingPaused, setSyncingPaused, verifiedBlockHeight, savedBlockHeight, chainName }) => {
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
            <Tooltip title="Latest block that has been successfully verified by the node." arrow>
                <Typography>Current block: {`${verifiedBlockHeight ? ('#' + verifiedBlockHeight) : '<unknown>'}`}</Typography>
            </Tooltip>
            <Tooltip title="Block that has been saved in the database and whose events are visible." arrow>
                <Typography>Saved block: {`${savedBlockHeight ? ('#' + savedBlockHeight) : '<unknown>'}`}</Typography>
            </Tooltip>
        </Box>
    );
});
