import '@fontsource-variable/inter/wght.css';
import '@fontsource-variable/jetbrains-mono/wght.css';
import './styles/index.css';

import { StrictMode } from 'react';
import { createRoot } from 'react-dom/client';

import { App } from './app/App.tsx';

const root = document.getElementById('root');
if (!root) throw new Error('SOLPRINT: #root element missing');

createRoot(root).render(
  <StrictMode>
    <App />
  </StrictMode>,
);
