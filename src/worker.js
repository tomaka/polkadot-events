import * as smoldot from './smoldot.js';
import polkadot_chains_specs from './polkadot.json';

smoldot.start({
    chain_spec: JSON.stringify(polkadot_chains_specs),
    database_content: null,
    relay_chain_spec: null,
    json_rpc_callback: null,
    database_save_callback: null,
});
