import React, { useState } from 'react'
import { triggerSync, subscribeSyncProgress } from '../api'

export default function SyncPanel() {
  const [startDate, setStartDate] = useState('')
  const [endDate, setEndDate] = useState('')
  const [running, setRunning] = useState(false)
  const [messages, setMessages] = useState([])
  const [progress, setProgress] = useState(null)

  const handleSync = async () => {
    setRunning(true)
    setMessages([])
    setProgress(null)

    try {
      await triggerSync(startDate || null, endDate || null)
    } catch (err) {
      setMessages([{ type: 'error', text: `Failed to start sync: ${err.message}` }])
      setRunning(false)
      return
    }

    subscribeSyncProgress((data) => {
      if (data.status === 'progress') {
        setProgress(data)
        if (data.message) {
          setMessages((m) => [...m, { type: 'info', text: data.message }])
        }
      } else if (data.status === 'done') {
        setMessages((m) => [...m, { type: 'success', text: data.message || 'Sync complete!' }])
        setRunning(false)
      } else if (data.status === 'error') {
        setMessages((m) => [...m, { type: 'error', text: data.message }])
        setRunning(false)
      }
    })
  }

  return (
    <div className="sync-panel">
      <h3>Data Sync</h3>
      <p className="panel-description">
        Fetch unclassified transactions from QBO and classify them using Claude Opus 4.6.
      </p>

      <div className="sync-form">
        <label>
          Start Date
          <input type="date" value={startDate} onChange={(e) => setStartDate(e.target.value)} />
        </label>
        <label>
          End Date
          <input type="date" value={endDate} onChange={(e) => setEndDate(e.target.value)} />
        </label>
        <button className="btn btn-primary" disabled={running} onClick={handleSync}>
          {running ? 'Syncing...' : 'Start Sync'}
        </button>
      </div>

      {progress && (
        <div className="sync-stats">
          {progress.gl_entries != null && <span>GL Entries: {progress.gl_entries}</span>}
          {progress.lines_found != null && <span>Lines Found: {progress.lines_found}</span>}
          {progress.rules_applied != null && <span>Rules Applied: {progress.rules_applied}</span>}
          {progress.llm_classified != null && <span>LLM Classified: {progress.llm_classified}</span>}
        </div>
      )}

      {messages.length > 0 && (
        <div className="sync-log">
          {messages.map((m, i) => (
            <div key={i} className={`log-entry ${m.type}`}>
              {m.text}
            </div>
          ))}
        </div>
      )}
    </div>
  )
}
