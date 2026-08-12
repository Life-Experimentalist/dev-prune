import React from 'react';
import { hydrateRoot, createRoot } from 'react-dom/client';
import App from './App.jsx';
import './index.css';
import './sections.css';
import './theme.css';

const root = document.getElementById('root');

// The production build ships prerendered markup, so hydrate it rather than throwing it
// away. `vite dev` serves an empty root, so fall back to a fresh render there.
if (root.hasChildNodes()) {
  hydrateRoot(root, <React.StrictMode><App /></React.StrictMode>);
} else {
  createRoot(root).render(<React.StrictMode><App /></React.StrictMode>);
}
