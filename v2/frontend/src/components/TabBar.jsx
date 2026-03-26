import React from 'react'
import { useApp } from '../context/AppContext'

const TABS = [
  { id: 'review', label: 'Review' },
  { id: 'sync', label: 'Sync' },
  { id: 'rules', label: 'Rules' },
  { id: 'audit', label: 'Audit' },
  { id: 'diagnostics', label: 'Diagnostics' },
]

export default function TabBar() {
  const { activeTab, setActiveTab } = useApp()

  return (
    <nav className="tab-bar">
      {TABS.map((tab) => (
        <button
          key={tab.id}
          className={`tab-btn ${activeTab === tab.id ? 'active' : ''}`}
          onClick={() => setActiveTab(tab.id)}
        >
          {tab.label}
        </button>
      ))}
    </nav>
  )
}
