import React, { useState, useEffect } from 'react'
import { fetchGroupItems } from '../api'

export default function LineItemTable({ status, account, entityName, suggestedClass }) {
  const [items, setItems] = useState([])
  const [loading, setLoading] = useState(true)

  useEffect(() => {
    setLoading(true)
    fetchGroupItems(status, account, entityName, suggestedClass)
      .then(setItems)
      .catch((err) => console.error('Failed to fetch group items:', err))
      .finally(() => setLoading(false))
  }, [status, account, entityName, suggestedClass])

  if (loading) return <p className="loading-text">Loading details...</p>
  if (items.length === 0) return <p className="empty-text">No items.</p>

  return (
    <table className="detail-table">
      <thead>
        <tr>
          <th>Date</th>
          <th>Type</th>
          <th>Tx:Line</th>
          <th>Entity</th>
          <th>Account</th>
          <th>Description</th>
          <th>Amount</th>
          <th>LLM Reasoning</th>
        </tr>
      </thead>
      <tbody>
        {items.map((item) => (
          <tr key={`${item.tx_id}:${item.line_id}`}>
            <td>{item.tx_date || '-'}</td>
            <td>{item.tx_type}</td>
            <td className="mono">{item.tx_id}:{item.line_id}</td>
            <td>{item.entity_name || '-'}</td>
            <td>{item.account || '-'}</td>
            <td title={item.line_description || ''}>{(item.line_description || '-').slice(0, 60)}</td>
            <td>{item.amount != null ? `$${item.amount.toFixed(2)}` : '-'}</td>
            <td title={item.llm_reasoning || ''}>{(item.llm_reasoning || '-').slice(0, 80)}</td>
          </tr>
        ))}
      </tbody>
    </table>
  )
}
