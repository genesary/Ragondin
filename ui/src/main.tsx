import { StrictMode } from 'react';
import { createRoot } from 'react-dom/client';
import '../design/base.css';
import { createApiClient } from './api/client.ts';
import { App } from './App.tsx';
import { BUILD } from './shell/build.ts';

const root = document.getElementById('root');
if (root === null) throw new Error('index.html has no #root element');

createRoot(root).render(
  <StrictMode>
    {/* No event stream yet: the first, /jobs/events, arrives with the job queue. */}
    <App client={createApiClient()} build={BUILD} reload={() => window.location.reload()} />
  </StrictMode>,
);
