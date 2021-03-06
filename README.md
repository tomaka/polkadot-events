This repository contains a JavaScript package that connects to the Polkadot blockchain, synchronizes the blocks (verifying them), and indexes the events that happened on chain by account.

This allows you to get the list of all the events that historically happened on a chain, filteredby account.

# How to build

```
yarn install
yarn run dev
```

Doing `yarn run build` produces a single `.html` file that inlines everything. However, the JS code uses IndexedDB, and browsers don't save the database if you open a standalone HTML file. This is a design issue that needs to be solved, but it's unclera how.

# How it works

The script runs a full node that connects to the Polkadot chain, reads the list of events at each block, then uses the PolkadotJS library to decode these events and index them.
