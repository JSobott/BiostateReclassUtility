import React, { useState, useEffect, useCallback } from 'react'
import { useApp } from '../context/AppContext'
import { fetchGroups, batchApprove, resetPending } from '../api'
import GroupRow from './GroupRow'

const STATUS_FILTERS = ['Pending', 'Approved', 'Validated', 'Failed', 'Posted']

export default function GroupTable() {
  const { statusFilter, setStatusFilter } = useApp()
  const [buckets, setBuckets] = useState([])
  const [loading, setLoading] = useState(false)

  const loadGroups = useCallback(async () => {
    setLoading(true)
    try {
      const data = await fetchGroups(statusFilter)
      setBuckets(data)
    } catch (err) {
      console.error('Failed to fetch groups:', err)
    } finally {
      setLoading(false)
    }
  }, [statusFilter])

  useEffect(() => {
    loadGroups()
  }, [loadGroups])

  const handleApprove = async (bucket, overrideClass) => {
    try {
      await batchApprove(bucket.account, bucket.entity_name, bucket.suggested_class_ref, overrideClass)
      loadGroups()
    } catch (err) {
      alert(`Approval failed: ${err.message}`)
    }
  }

  const handleReset = async (bucket) => {
    try {
      await resetPending(bucket.account, bucket.entity_name, bucket.suggested_class_ref)
      loadGroups()
    } catch (err) {
      alert(`Reset failed: ${err.message}`)
    }
  }

  const handleApproveAll = async () => {
    for (const bucket of buckets) {
      try {
        await batchApprove(bucket.account, bucket.entity_name, bucket.suggested_class_ref, null)
      } catch (err) {
        console.error('Batch approve failed for', bucket.entity_name, err)
      }
    }
    loadGroups()
  }

  const totalItems = buckets.reduce((sum, b) => sum + b.tx_count, 0)
  const totalAmount = buckets.reduce((sum, b) => sum + b.total_amount, 0)

  return (
    <div className="group-table-container">
      <div className="status-filters">
        {STATUS_FILTERS.map((s) => (
          <button
            key={s}
            className={`filter-btn ${statusFilter === s ? 'active' : ''}`}
            onClick={() => setStatusFilter(s)}
          >
            {s}
          </button>
        ))}
      </div>

      <div className="stats-bar">
        <span>{buckets.length} groups</span>
        <span>{totalItems} items</span>
        <span>${totalAmount.toLocaleString(undefined, { minimumFractionDigits: 2 })}</span>
        {statusFilter === 'Pending' && buckets.length > 0 && (
          <button className="btn btn-primary" onClick={handleApproveAll}>
            Approve All
          </button>
        )}
        <button className="btn btn-secondary" onClick={loadGroups}>
          Refresh
        </button>
      </div>

      {loading ? (
        <p className="loading-text">Loading...</p>
      ) : buckets.length === 0 ? (
        <p className="empty-text">No {statusFilter.toLowerCase()} transactions.</p>
      ) : (
        <table className="data-table">
          <thead>
            <tr>
              <th></th>
              <th>Entity</th>
              <th>Account</th>
              <th>Suggested Class</th>
              <th>Count</th>
              <th>Total $</th>
              <th>Confidence</th>
              {statusFilter === 'Pending' && <th>Override</th>}
              <th>Action</th>
            </tr>
          </thead>
          <tbody>
            {buckets.map((bucket, i) => (
              <GroupRow
                key={`${bucket.account}-${bucket.entity_name}-${bucket.suggested_class_ref}`}
                bucket={bucket}
                statusFilter={statusFilter}
                onApprove={handleApprove}
                onReset={handleReset}
              />
            ))}
          </tbody>
        </table>
      )}
    </div>
  )
}
