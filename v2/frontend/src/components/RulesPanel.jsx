import React, { useState, useEffect, useCallback } from 'react'
import { fetchRules, createRule, deleteRule } from '../api'
import ClassDropdown from './ClassDropdown'

const CONDITION_FIELDS = ['account_type', 'account', 'tx_type', 'entity_name']

export default function RulesPanel() {
  const [rules, setRules] = useState([])
  const [field, setField] = useState('account_type')
  const [value, setValue] = useState('')
  const [targetClass, setTargetClass] = useState(null)

  const loadRules = useCallback(async () => {
    try {
      setRules(await fetchRules())
    } catch (err) {
      console.error('Failed to fetch rules:', err)
    }
  }, [])

  useEffect(() => {
    loadRules()
  }, [loadRules])

  const handleAdd = async () => {
    if (!value.trim() || !targetClass) return
    try {
      await createRule({ condition_field: field, condition_value: value.trim(), target_class: targetClass })
      setValue('')
      setTargetClass(null)
      loadRules()
    } catch (err) {
      alert(`Failed to create rule: ${err.message}`)
    }
  }

  const handleDelete = async (id) => {
    try {
      await deleteRule(id)
      loadRules()
    } catch (err) {
      alert(`Failed to delete rule: ${err.message}`)
    }
  }

  return (
    <div className="rules-panel">
      <h3>Classification Rules</h3>
      <p className="panel-description">
        Heuristic rules are applied before LLM inference. Matching transactions skip the AI and are classified directly.
      </p>

      <div className="rule-form">
        <label>If</label>
        <select value={field} onChange={(e) => setField(e.target.value)}>
          {CONDITION_FIELDS.map((f) => (
            <option key={f} value={f}>{f}</option>
          ))}
        </select>
        <label>equals</label>
        <input type="text" value={value} onChange={(e) => setValue(e.target.value)} placeholder="Value..." />
        <label>then set class to</label>
        <ClassDropdown value={targetClass} onChange={setTargetClass} includeEmpty={false} />
        <button className="btn btn-primary" onClick={handleAdd}>
          + Add Rule
        </button>
      </div>

      {rules.length === 0 ? (
        <p className="empty-text">No rules defined.</p>
      ) : (
        <table className="data-table">
          <thead>
            <tr>
              <th>Condition</th>
              <th>Value</th>
              <th>Target Class</th>
              <th>Action</th>
            </tr>
          </thead>
          <tbody>
            {rules.map((r) => (
              <tr key={r.id}>
                <td>{r.condition_field}</td>
                <td>{r.condition_value}</td>
                <td>{r.target_class}</td>
                <td>
                  <button className="btn btn-danger btn-sm" onClick={() => handleDelete(r.id)}>
                    Delete
                  </button>
                </td>
              </tr>
            ))}
          </tbody>
        </table>
      )}
    </div>
  )
}
