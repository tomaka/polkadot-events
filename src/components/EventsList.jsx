import React from 'react';
import { Paper, Table, TableBody, TableCell, TableContainer, TableHead, TablePagination, TableRow, Typography } from '@material-ui/core';
import { Metadata } from '@polkadot/metadata';
import { TypeRegistry } from '@polkadot/types';
import { getSpecTypes, getSpecHasher, getSpecAlias, getSpecExtensions } from '@polkadot/types-known';

export default class extends React.Component {
    constructor(props) {
        super(props);
        this.state = {
            loading: true,
            rowsPerPage: 10,
            page: 0,
            finalOutcome: null
        };
    }

    componentDidMount() {
        (async () => {
            this.registries = {};

            let blockNumbers = new Set();
            this.props.dbEvents.forEach((dbEvent) => blockNumbers.add(dbEvent.block));

            let blockPromises = [];
            blockNumbers.forEach((blockNumber) => {
                blockPromises.push(this.props.database.get('blocks', blockNumber));
            });

            let blockPromisesResult = await Promise.all(blockPromises);
            let blocksFromDb = {};
            let metadataPromises = [];
            blockPromisesResult.forEach((block) => {
                blocksFromDb[block.number] = block;
                if (!this.registries[block.runtime_spec]) {
                    this.registries[block.runtime_spec] = 'Loading';
                    metadataPromises.push(this.props.database.get('metadata', block.runtime_spec));
                }
            });

            let metadataPromisesResult = await Promise.all(metadataPromises);
            metadataPromisesResult.forEach((dbMetadata) => {
                // Inspired from https://github.com/polkadot-js/api/blob/ec76b11666aea72135f899267abf784fc6309156/packages/api/src/base/Init.ts#L83
                let registry = new TypeRegistry();
                registry.register(getSpecTypes(registry, this.props.chainSpec.name, dbMetadata.spec_name, dbMetadata.runtime_spec));
                registry.setHasher(getSpecHasher(registry, this.props.chainSpec.name, dbMetadata.spec_name));
                if (registry.knownTypes.typesBundle) {
                    registry.knownTypes.typesAlias = getSpecAlias(registry, this.props.chainSpec.name, dbMetadata.spec_name);
                }
                const metadata = new Metadata(registry, dbMetadata.metadata);
                registry.setMetadata(metadata, undefined, getSpecExtensions(registry, this.props.chainSpec.name, dbMetadata.spec_name));
                this.registries[dbMetadata.runtime_spec] = registry;
            });

            for (var i in blocksFromDb) {
                let registry = this.registries[blocksFromDb[i].runtime_spec];
                const records = registry.createType('Vec<EventRecord>', blocksFromDb[i].events);
                blocksFromDb[i].decodedRecords = records;
            }

            let columns = [];
            const finalOutcome = this.props.dbEvents.map((dbEvent) => {
                const record = blocksFromDb[dbEvent.block].decodedRecords[dbEvent.recordIndex];
                const numArgs = record.event.data.length;
                while (numArgs > columns.length) {
                    columns.push({});
                }
                return {
                    blockNumber: dbEvent.block,
                    eventSection: record.event.section,
                    eventMethod: record.event.method,
                    documentation: record.event.meta.documentation,
                    argIndex: dbEvent.argIndex,
                    args: record.event.data
                };
            });

            this.setState({
                loading: false,
                finalOutcome: finalOutcome,
                columns: columns
            });
        })();
    }

    render() {
        return (
            <>
                {!this.state.loading &&
                    <Paper>
                        <TableContainer>
                            <Table stickyHeader aria-label="sticky table">
                                <TableHead>
                                    <TableRow>
                                        <TableCell>Block #</TableCell>
                                        <TableCell>Section</TableCell>
                                        <TableCell>Method</TableCell>
                                        {this.state.columns.map((dummy, colNum) => (
                                            <TableCell key={colNum}>Argument #{colNum + 1}</TableCell>
                                        ))}
                                    </TableRow>
                                </TableHead>
                                <TableBody>
                                    {this.state.finalOutcome
                                        .slice(this.state.page * this.state.rowsPerPage, this.state.page * this.state.rowsPerPage + this.state.rowsPerPage)
                                        .map((entry, i) => {
                                            return (
                                                <TableRow hover role="checkbox" tabIndex={-1} key={i}>
                                                    <TableCell>
                                                        {entry.blockNumber}
                                                    </TableCell>
                                                    <TableCell>
                                                        {entry.eventSection}
                                                    </TableCell>
                                                    <TableCell>
                                                        {entry.eventMethod}
                                                    </TableCell>
                                                    {this.state.columns.map((dummy, colNum) => (
                                                        <TableCell key={colNum}>
                                                            <Typography color={entry.argIndex == colNum ? 'primary' : 'initial'}>
                                                                {entry.args[colNum] ? entry.args[colNum].toString() : ''}
                                                            </Typography>
                                                        </TableCell>
                                                    ))}
                                                </TableRow>
                                            );
                                        })}
                                </TableBody>
                            </Table>
                        </TableContainer>
                        <TablePagination
                            rowsPerPageOptions={[10, 25, 100]}
                            component="div"
                            count={this.props.dbEvents.length}
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
