import React from 'react';
import { createRoot } from 'react-dom/client';
import '@xyflow/react/dist/style.css';
import { Dashboard } from './Dashboard';
import { createHttpProvider } from './provider';
import './standalone.css';
const provider = createHttpProvider();
const root = document.getElementById('root');
if (root) createRoot(root).render(<React.StrictMode><Dashboard provider={provider}/></React.StrictMode>);
