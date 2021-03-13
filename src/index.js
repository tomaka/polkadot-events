import React from 'react';
import ReactDom from 'react-dom';
import { default as App } from './components/App.jsx';

import polkadot_chains_specs from './polkadot.json';

ReactDom.render(
    <App chainSpec={polkadot_chains_specs} />,
    document.getElementById("app")
)
