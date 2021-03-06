import React, { useState, useEffect } from 'react';
import TextField from '@material-ui/core/TextField';

export default React.memo(({ setAddress }) => {
    const [address, setValue] = useState(null);
    
    useEffect(() => {
        setAddress(address)
    }, [address, setAddress]);

    const handleChangeButton = (e) => {
        const val = e.currentTarget.value;
        setValue(val);
    };

    return (
        <TextField
            required
            onChange={handleChangeButton}
            onFocus={handleChangeButton}
            onBlur={handleChangeButton}
            label="Account address"
            value={address != null ? address : ''}
            helperText="Name of the accounts whose events to scrap"
            variant="standard"
        />
    );
});
