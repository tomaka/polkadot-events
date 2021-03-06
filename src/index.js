import React from 'react';
import ReactDom from 'react-dom';
import { default as App } from './components/App.jsx';

ReactDom.render(<App />, document.getElementById("app"))

//const worker = new Worker('./worker.js');
