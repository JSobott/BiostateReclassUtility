import React, { useState } from 'react'
import { useApp } from '../context/AppContext'
import { triggerWriteback, subscribeWritebackProgress } from '../api'

export default function WritebackPanel() {
  const { statusFilter } = useApp()
  const [dryRun, setDryRun] = useState(true)
  const [running, setRunning] = useState(false)
  const [progress, setProgress] = useState(null)
  const [results, setResults] = useState([])

  if (statusFilter !== 'Approved' && statusFilter !== 'Validated') return null

  const handleSync = async () => {
    setRunning(true)
    setProgress(null)
    setResults([])

    try {
      await triggerWriteback(dryRun)
    } catch (err) {
      setResults([{ success: false, message: `Failed to start writeback: ${err.message}` }])
      setRunning(false)
      return
    }

    const es = subscribeWritebackProgress((data) => {
      if (data.status === 'started') {
        setProgress({ processed: 0, total: data.total, success: 0, failed: 0 })
      } else if (data.status === 'progress') {
        if (data.processed != null) {
          setProgress((p) => ({ ...p, processed: data.processed, success: data.success, failed: data.failed }))
        }
        if (data.entity_name) {
          setResults((r) => [...r, { success: data.success, message: `${data.entity_name}${data.error ? ': ' + data.error : ''}` }])
        }
      } else if (data.status === 'done') {
        setProgress((p) => ({ ...p, processed: data.processed, success: data.success, failed: data.failed }))
        setRunning(false)
      } else if (data.status === 'error') {
        setResults((r) => [...r, { success: false, message: data.message }])
        setRunning(false)
      }
    })

    return () => es.close()
  }

  const pct = progress && progress.total > 0 ? Math.round((progress.processed / progress.total) * 100) : 0

  return (
    <div className="writeback-panel">
      <h3>Sync to QBO</h3>
      <div className="writeback-controls">
        <label className="toggle-label">
          <input type="checkbox" checked={dryRun} onChange={(e) => setDryRun(e.target.checked)} />
          Dry Run (validate only)
        </label>
        <button className="btn btn-primary" disabled={running} onClick={handleSync}>
          {running ? 'Processing...' : dryRun ? 'Validate' : 'Write to QBO'}
        </button>
      </div>

      {progress && (
        <div className="progress-section">
          <div className="progress-bar-bg">
            <div className="progress-bar-fill" style={{ width: `${pct}%` }} />
          </div>
          <div className="progress-stats">
            <span>Processed: {progress.processed}/{progress.total}</span>
            <span className="success-count">Success: {progress.success}</span>
            <span className="failed-count">Failed: {progress.failed}</span>
          </div>
        </div>
      )}

      {results.length > 0 && (
        <div className="writeback-results">
          {results.map((r, i) => (
            <div key={i} className={`result-item ${r.success ? 'success' : 'failure'}`}>
              {r.success ? '\u2714' : '\u2718'} {r.message}
            </div>
          ))}
        </div>
      )}
    </div>
  )
}
