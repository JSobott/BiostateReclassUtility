import React from 'react'
import { useApp } from '../context/AppContext'

export default function Header() {
  const { connectionStatus, realmId } = useApp()

  const statusColor = connectionStatus === 'connected' ? '#7ee787' : '#f85149'
  const statusText = connectionStatus === 'connected' ? `Connected (${realmId})` : 'Disconnected'

  return (
    <header className="app-header">
      <div>
        <h1>BiostateReclassUtility v2</h1>
        <p className="subtitle">QBO Transaction Classification with Claude Opus 4.6</p>
      </div>
      <div className="connection-badge" style={{ color: statusColor }}>
        <span className="status-dot" style={{ backgroundColor: statusColor }} />
        {statusText}
      </div>
    </header>
  )
}
