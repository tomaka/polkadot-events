import React from 'react';
import TextField from '@material-ui/core/TextField';
import { Autocomplete } from '@material-ui/lab';

export default class extends React.Component {
    constructor(props) {
        super(props);
        this.state = {
            address: '',
            autocompleteAddresses: [],
            autocompleteAddressesUpToDate: false,
        };
    }

    handleChangeButton(address) {
        this.props.setAddress(address);
        this.setState({
            address: address,
        });
    }

    componentDidMount() {
        // Every 15 seconds, set `autocompleteAddressesUpToDate` to false.
        this.timerId = setInterval(() => {
            this.setState({
                autocompleteAddressesUpToDate: false
            })
        }, 15000);
    }

    componentWillUnmount() {
        clearInterval(this.timerId);
    }

    render() {
        if (!this.state.autocompleteAddressesUpToDate && this.props.database) {
            (async () => {
                let autocompleteAddresses = new Set();

                let cursor = await this.props.database.transaction('events').store.index('account').openKeyCursor();
                while (cursor) {
                    autocompleteAddresses.add(cursor.key);
                    cursor = await cursor.continue();
                }

                this.setState({
                    autocompleteAddresses: Array.from(autocompleteAddresses),
                    autocompleteAddressesUpToDate: true,
                })
            })();
        }

        return (
            <Autocomplete
                options={this.state.autocompleteAddresses}
                freeSolo
                loading={!this.state.autocompleteAddressesUpToDate}
                size="small"
                onInputChange={(e, value) => this.handleChangeButton(value)}
                renderInput={(params) => (
                    <TextField
                        {...params}
                        required
                        label="Account address"
                        value={this.state.address != null ? this.state.address : ''}
                        variant="standard"
                        inputProps={{
                            ...params.inputProps
                        }}
                    />
                )}
            />
        );
    }
}
