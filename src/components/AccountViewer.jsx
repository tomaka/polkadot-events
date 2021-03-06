import React from 'react';
import { default as AccountInput } from './AccountInput.jsx';

export default class extends React.Component {
    constructor(props) {
        super(props);
        this.state = {
            blocksAndEvents: []
        };
    }

    updateAddress(address) {
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
                blocksAndEvents: blocksAndEvents
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
                {this.state.blocksAndEvents.map((blockEvent, i) => {
                    return <p key={i}>{blockEvent.number}</p>
                })}
            </>
        );
    }
}
