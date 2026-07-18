import type { ReactNode } from 'react';
import { BackgroundDecoration } from './BackgroundDecoration';

export function Shell({ children }: { children: ReactNode }) {
  return (
    <main className="app-shell">
      <BackgroundDecoration />
      <div className="app-content">{children}</div>
    </main>
  );
}
