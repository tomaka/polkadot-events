import React from 'react';
import { CircularProgress, Paper, Table, TableBody, TableCell, TableContainer, TableHead, TablePagination, TableRow } from '@material-ui/core';
import { default as AccountInput } from './AccountInput.jsx';
import { default as EventsList } from './EventsList.jsx';

export default class extends React.Component {
    constructor(props) {
        super(props);
        this.state = {
            blocksAndEvents: null,
            loading: false,
            rowsPerPage: 10,
            page: 0,
        };
    }

    updateAddress(address) {
        this.setState({
            blocksAndEvents: null,
            loading: true,
        });

        // TODO: reload periodically?
        (async () => {
            let records = [];
            let cursor = await this.props.database.transaction('events').store.index('account').openCursor(address);
            while (cursor) {
                records.push(cursor.value);
                cursor = await cursor.continue();
            }

            let blocksAndEvents = [];
            for (var i in records) {
                const record = records[i];
                let block = await this.props.database.get('blocks', record.block);
                block.recordIndex = record.recordIndex;
                block.argIndex = record.argIndex;
                blocksAndEvents.push(block);
            }

            this.setState({
                blocksAndEvents: blocksAndEvents,
                loading: false,
            });
        })();
    }

    render() {
        return (
            <>
                <AccountInput
                    database={this.props.database}
                    setAddress={(address) => { this.updateAddress(address) }}
                />
                {this.state.loading && <CircularProgress />}

                {!this.state.loading && this.state.blocksAndEvents &&
                    <EventsList database={this.props.database} blocksAndEvents={this.state.blocksAndEvents} />
                }
            </>
        );
    }
}
