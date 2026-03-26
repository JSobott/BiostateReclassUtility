import React from 'react'
import { useApp } from '../context/AppContext'

export default function ClassDropdown({ value, onChange, includeEmpty = true }) {
  const { availableClasses } = useApp()

  return (
    <select className="class-dropdown" value={value || ''} onChange={(e) => onChange(e.target.value || null)}>
      {includeEmpty && <option value="">-- No Override --</option>}
      {availableClasses.map(([id, name]) => (
        <option key={id} value={name}>
          {name}
        </option>
      ))}
    </select>
  )
}
