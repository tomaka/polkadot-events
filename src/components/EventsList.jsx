import React from 'react';
import { Paper, Table, TableBody, TableCell, TableContainer, TableHead, TablePagination, TableRow } from '@material-ui/core';

export default class extends React.Component {
    constructor(props) {
        super(props);
        this.state = {
            rowsPerPage: 10,
            page: 0,
        };
    }

    render() {
        return (
            <>
                {this.props.blocksAndEvents &&
                    <Paper>
                        <TableContainer>
                            <Table stickyHeader aria-label="sticky table">
                                <TableHead>
                                    <TableRow>
                                        <TableCell>Block #</TableCell>
                                    </TableRow>
                                </TableHead>
                                <TableBody>
                                    {this.props.blocksAndEvents
                                        .slice(this.state.page * this.state.rowsPerPage, this.state.page * this.state.rowsPerPage + this.state.rowsPerPage)
                                        .map((blockEvent, i) => {
                                            return (
                                                <TableRow hover role="checkbox" tabIndex={-1} key={i}>
                                                    <TableCell>
                                                        {blockEvent.number}
                                                    </TableCell>
                                                </TableRow>
                                            );
                                        })}
                                </TableBody>
                            </Table>
                        </TableContainer>
                        <TablePagination
                            rowsPerPageOptions={[10, 25, 100]}
                            component="div"
                            count={this.props.blocksAndEvents.length}
                            rowsPerPage={this.state.rowsPerPage}
                            page={this.state.page}
                            onChangePage={(event, newPage) => { this.setState({ page: newPage }) }}
                            onChangeRowsPerPage={(event) => { this.setState({ page: 0, rowsPerPage: +event.target.value }) }}
                        />
                    </Paper>
                }
            </>
        );
    }
}
