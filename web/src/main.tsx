import React from 'react';
import ReactDOM from 'react-dom/client';
import App from './App';
import AdminApp from './AdminApp';
import './styles.css';

const isAdmin = window.location.pathname.startsWith('/admin');
const Root = isAdmin ? AdminApp : App;
document.title = isAdmin ? '照看室' : '搭档状态';

ReactDOM.createRoot(document.getElementById('root')!).render(
  <React.StrictMode>
    <Root />
  </React.StrictMode>,
);
