This repository contains a JavaScript package that connects to the Polkadot blockchain, synchronizes the blocks (verifying them), and indexes the events that happened on chain by account.

This allows you to get the list of all the events that historically happened on a chain, filteredby account.

Hosted here: https://tomaka.github.io/polkadot-events/

# How to build

```
yarn install
yarn run dev
```

Doing `yarn run build` produces a single `.html` file that inlines everything. However, the JS code uses IndexedDB, and browsers don't save the database if you open a standalone HTML file. This is a design issue that needs to be solved, but it's unclera how.

# How it works

The script runs a full node that connects to the Polkadot chain, reads the list of events at each block, then uses the PolkadotJS library to decode these events and index them.

# How to maintain this code

Everything in the `src` directory was written for this project, except for the `smoldot.js` file. Feel free to modify.

The code in `backend-rust` and the `smoldot.js` file have mostly been copy-pasted from [the smoldot repository](https://github.com/paritytech/smoldot) and trimmed down. The major part that has been written specifically for this project is everything related to sending finalized blocks from Rust to the frontend. The Rust code is difficult to read and thus to maintain, but it isn't really expected to change. I would encourage improving the code in the smoldot repository in order to make code more reusable (see [this issue](https://github.com/paritytech/smoldot/issues/595)), which should reduce the amount of code in `backend-rust`.
