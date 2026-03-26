import React from 'react'
import { useApp } from './context/AppContext'
import Header from './components/Header'
import TabBar from './components/TabBar'
import GroupTable from './components/GroupTable'
import WritebackPanel from './components/WritebackPanel'
import RulesPanel from './components/RulesPanel'
import AuditLog from './components/AuditLog'
import DiagnosticsPanel from './components/DiagnosticsPanel'
import SyncPanel from './components/SyncPanel'

export default function App() {
  const { activeTab } = useApp()

  return (
    <div className="app-container">
      <Header />
      <TabBar />
      <main className="main-content">
        {activeTab === 'review' && (
          <>
            <GroupTable />
            <WritebackPanel />
          </>
        )}
        {activeTab === 'sync' && <SyncPanel />}
        {activeTab === 'rules' && <RulesPanel />}
        {activeTab === 'audit' && <AuditLog />}
        {activeTab === 'diagnostics' && <DiagnosticsPanel />}
      </main>
    </div>
  )
}
