import React, { useState } from 'react'
import ClassDropdown from './ClassDropdown'
import LineItemTable from './LineItemTable'

export default function GroupRow({ bucket, statusFilter, onApprove, onReset }) {
  const [expanded, setExpanded] = useState(false)
  const [overrideClass, setOverrideClass] = useState(null)
  const [busy, setBusy] = useState(false)

  const handleApprove = async () => {
    setBusy(true)
    try {
      await onApprove(bucket, overrideClass)
    } finally {
      setBusy(false)
    }
  }

  const handleReset = async () => {
    setBusy(true)
    try {
      await onReset(bucket)
    } finally {
      setBusy(false)
    }
  }

  const confidencePct = (bucket.avg_confidence * 100).toFixed(0)

  return (
    <>
      <tr className="group-row" onClick={() => setExpanded(!expanded)}>
        <td className="expand-cell">{expanded ? '\u25BC' : '\u25B6'}</td>
        <td>{bucket.entity_name}</td>
        <td>{bucket.account}</td>
        <td>{bucket.suggested_class_ref}</td>
        <td>{bucket.tx_count}</td>
        <td>${bucket.total_amount.toLocaleString(undefined, { minimumFractionDigits: 2 })}</td>
        <td>
          <span className={`confidence ${bucket.avg_confidence >= 0.8 ? 'high' : bucket.avg_confidence >= 0.5 ? 'medium' : 'low'}`}>
            {confidencePct}%
          </span>
        </td>
        {statusFilter === 'Pending' && (
          <td onClick={(e) => e.stopPropagation()}>
            <ClassDropdown value={overrideClass} onChange={setOverrideClass} />
          </td>
        )}
        <td onClick={(e) => e.stopPropagation()}>
          {statusFilter === 'Pending' && (
            <button className="btn btn-approve" disabled={busy} onClick={handleApprove}>
              {busy ? '...' : 'Approve'}
            </button>
          )}
          {statusFilter === 'Failed' && (
            <button className="btn btn-secondary" disabled={busy} onClick={handleReset}>
              {busy ? '...' : 'Reset'}
            </button>
          )}
        </td>
      </tr>
      {expanded && (
        <tr className="drilldown-row">
          <td colSpan="9">
            <LineItemTable
              status={statusFilter}
              account={bucket.account}
              entityName={bucket.entity_name}
              suggestedClass={bucket.suggested_class_ref}
            />
          </td>
        </tr>
      )}
    </>
  )
}
