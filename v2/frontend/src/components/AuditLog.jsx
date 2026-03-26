import React, { useState, useEffect } from 'react'
import { fetchAuditLogs } from '../api'

export default function AuditLog() {
  const [logs, setLogs] = useState([])
  const [loading, setLoading] = useState(true)

  useEffect(() => {
    setLoading(true)
    fetchAuditLogs()
      .then(setLogs)
      .catch((err) => console.error('Failed to fetch audit logs:', err))
      .finally(() => setLoading(false))
  }, [])

  const statusColor = (status) => {
    if (status === 'SUCCESS') return '#7ee787'
    if (status === 'FAILED') return '#f85149'
    if (status === 'DRY_RUN') return '#58a6ff'
    return '#8b949e'
  }

  if (loading) return <p className="loading-text">Loading audit logs...</p>

  return (
    <div className="audit-panel">
      <h3>Writeback Audit History</h3>
      {logs.length === 0 ? (
        <p className="empty-text">No audit entries yet.</p>
      ) : (
        <table className="data-table">
          <thead>
            <tr>
              <th>Timestamp</th>
              <th>Status</th>
              <th>Batch</th>
              <th>Details</th>
            </tr>
          </thead>
          <tbody>
            {logs.map((log) => (
              <tr key={log.id}>
                <td className="mono">{new Date(log.timestamp).toLocaleString()}</td>
                <td>
                  <span className="badge" style={{ backgroundColor: statusColor(log.status) }}>
                    {log.status}
                  </span>
                </td>
                <td>{log.batch_id}</td>
                <td>
                  <details>
                    <summary>View JSON</summary>
                    <pre className="json-pre">{log.response_json}</pre>
                  </details>
                </td>
              </tr>
            ))}
          </tbody>
        </table>
      )}
    </div>
  )
}
