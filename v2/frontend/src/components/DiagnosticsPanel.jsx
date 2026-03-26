import React, { useState } from 'react'
import { useApp } from '../context/AppContext'
import { fetchCompanyInfo, refreshToken, seedTokens } from '../api'

export default function DiagnosticsPanel() {
  const { connectionStatus, realmId, refreshConnection } = useApp()
  const [companyInfo, setCompanyInfo] = useState(null)
  const [message, setMessage] = useState('')
  const [seedForm, setSeedForm] = useState({ access_token: '', refresh_token: '', realm_id: '' })

  const handleRefresh = async () => {
    try {
      const data = await refreshToken()
      setMessage(data.message || 'Token refreshed')
      refreshConnection()
    } catch (err) {
      setMessage(`Refresh failed: ${err.message}`)
    }
  }

  const handleFetchCompany = async () => {
    try {
      const data = await fetchCompanyInfo()
      setCompanyInfo(data)
    } catch (err) {
      setMessage(`Failed: ${err.message}`)
    }
  }

  const handleSeed = async () => {
    try {
      const data = await seedTokens(seedForm.access_token, seedForm.refresh_token, seedForm.realm_id)
      setMessage(data.message || 'Tokens saved')
      refreshConnection()
    } catch (err) {
      setMessage(`Seed failed: ${err.message}`)
    }
  }

  const handleLogin = () => {
    window.location.href = '/auth/login'
  }

  const handleDisconnect = () => {
    window.location.href = '/auth/disconnect'
  }

  return (
    <div className="diagnostics-panel">
      <h3>OAuth Diagnostics</h3>

      <div className="diag-section">
        <h4>Connection Status</h4>
        <p>
          Status: <strong>{connectionStatus}</strong>
          {realmId && <> | Realm: <code>{realmId}</code></>}
        </p>
        <div className="btn-group">
          <button className="btn btn-primary" onClick={handleLogin}>Authenticate with Intuit</button>
          <button className="btn btn-secondary" onClick={handleRefresh}>Refresh Token</button>
          <button className="btn btn-danger" onClick={handleDisconnect}>Disconnect</button>
        </div>
      </div>

      <div className="diag-section">
        <h4>Company Info</h4>
        <button className="btn btn-secondary" onClick={handleFetchCompany}>Fetch Company Info</button>
        {companyInfo && <pre className="json-pre">{JSON.stringify(companyInfo, null, 2)}</pre>}
      </div>

      <div className="diag-section">
        <h4>Seed Tokens (Manual)</h4>
        <div className="seed-form">
          <textarea
            placeholder="Access Token"
            value={seedForm.access_token}
            onChange={(e) => setSeedForm({ ...seedForm, access_token: e.target.value })}
          />
          <textarea
            placeholder="Refresh Token"
            value={seedForm.refresh_token}
            onChange={(e) => setSeedForm({ ...seedForm, refresh_token: e.target.value })}
          />
          <input
            type="text"
            placeholder="Realm ID"
            value={seedForm.realm_id}
            onChange={(e) => setSeedForm({ ...seedForm, realm_id: e.target.value })}
          />
          <button className="btn btn-primary" onClick={handleSeed}>Save to Keychain</button>
        </div>
      </div>

      {message && <div className="diag-message">{message}</div>}
    </div>
  )
}
