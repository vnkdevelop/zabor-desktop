import React from 'react'
import ReactDOM from 'react-dom/client'
import App from './App'
import './index.css'
import './i18n'
import { ErrorBoundary } from './ErrorBoundary'
import { UpdaterWindow } from './components/UpdaterWindow'

function Root(): JSX.Element {
  const isUpdaterRoute = (): boolean => {
    return (
      window.location.hash === '#updater' ||
      window.location.hash.startsWith('#updater') ||
      window.location.search.includes('updater')
    )
  }

  const [isUpdater, setIsUpdater] = React.useState(isUpdaterRoute)

  React.useEffect(() => {
    const handleHashChange = (): void => {
      setIsUpdater(isUpdaterRoute())
    }
    window.addEventListener('hashchange', handleHashChange)
    return () => window.removeEventListener('hashchange', handleHashChange)
  }, [])

  return isUpdater ? <UpdaterWindow /> : <App />
}

ReactDOM.createRoot(document.getElementById('root') as HTMLElement).render(
  <ErrorBoundary>
    <Root />
  </ErrorBoundary>
)