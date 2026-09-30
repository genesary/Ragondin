import { StrictMode } from 'react';
import { createRoot } from 'react-dom/client';
import '../base.css';
import { Preview } from './Preview.tsx';

const root = document.getElementById('root');
if (root === null) throw new Error('the preview page has no #root element');

createRoot(root).render(
  <StrictMode>
    <Preview />
  </StrictMode>,
);
