import { Component, ErrorInfo, ReactNode } from 'react';

interface Props {
  children: ReactNode;
}

interface State {
  error: Error | null;
}

export class ErrorBoundary extends Component<Props, State> {
  state: State = { error: null };

  static getDerivedStateFromError(error: Error): State {
    return { error };
  }

  componentDidCatch(error: Error, info: ErrorInfo) {
    console.error('[ErrorBoundary] React crash:', error, info.componentStack);
  }

  render() {
    const { error } = this.state;
    if (error) {
      return (
        <div style={{
          position: 'fixed', inset: 0, background: '#0B0B0B',
          color: 'white', display: 'flex', flexDirection: 'column',
          alignItems: 'center', justifyContent: 'center', padding: 32,
          fontFamily: 'monospace', gap: 16
        }}>
          <h1 style={{ color: '#DA373C', fontSize: 24, margin: 0 }}>React Crash</h1>
          <pre style={{
            background: '#161618', padding: 16, borderRadius: 8,
            color: '#f87171', fontSize: 12, maxWidth: 800,
            overflow: 'auto', whiteSpace: 'pre-wrap', wordBreak: 'break-all'
          }}>
            {error.message}
            {'\n\n'}
            {error.stack}
          </pre>
          <button
            onClick={() => window.location.reload()}
            style={{
              background: '#DA373C', color: 'white', border: 'none',
              padding: '10px 24px', borderRadius: 8, cursor: 'pointer',
              fontSize: 14, fontWeight: 'bold'
            }}
          >
            Перезапустить
          </button>
        </div>
      );
    }
    return this.props.children;
  }
}
