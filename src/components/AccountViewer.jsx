import React from 'react';
import { CircularProgress, Paper, Table, TableBody, TableCell, TableContainer, TableHead, TablePagination, TableRow } from '@material-ui/core';
import { default as AccountInput } from './AccountInput.jsx';
import { default as EventsList } from './EventsList.jsx';

export default class extends React.Component {
    constructor(props) {
        super(props);
        this.state = {
            dbEvents: null,
            loading: false,
            rowsPerPage: 10,
            page: 0,
        };
    }

    updateAddress(address) {
        this.setState({
            dbEvents: null,
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

            this.setState({
                dbEvents: records,
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

                {!this.state.loading && this.state.dbEvents &&
                    <EventsList chainSpec={this.props.chainSpec} database={this.props.database} dbEvents={this.state.dbEvents} />
                }
            </>
        );
    }
}
