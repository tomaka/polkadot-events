import React, { useState, useEffect } from 'react';
import { default as AccountInput } from './AccountInput.jsx';

export default class extends React.Component {
    constructor(props) {
        super(props);
    }

    render() {
        return (
            <span>{this.props.event}</span>
        );
    }
}
