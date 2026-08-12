// Server entry used only at build time by `prerender.js`. It turns the same component
// tree the browser hydrates into static HTML, so a crawler that runs no JavaScript still
// receives the full page.
import React from 'react';
import { renderToString } from 'react-dom/server';
import App from './App.jsx';

export function render() {
  return renderToString(<App />);
}
