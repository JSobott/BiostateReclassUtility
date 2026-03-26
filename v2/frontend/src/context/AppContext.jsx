import React, { createContext, useContext, useState, useEffect, useCallback } from 'react'
import { fetchDiagnosticsStatus, fetchClasses } from '../api'

const AppContext = createContext(null)

export function AppProvider({ children }) {
  const [connectionStatus, setConnectionStatus] = useState('loading')
  const [realmId, setRealmId] = useState(null)
  const [availableClasses, setAvailableClasses] = useState([])
  const [activeTab, setActiveTab] = useState('review')
  const [statusFilter, setStatusFilter] = useState('Pending')

  const refreshConnection = useCallback(async () => {
    try {
      const data = await fetchDiagnosticsStatus()
      setConnectionStatus(data.status)
      setRealmId(data.realm_id)
    } catch {
      setConnectionStatus('disconnected')
    }
  }, [])

  const refreshClasses = useCallback(async () => {
    try {
      const data = await fetchClasses()
      setAvailableClasses(data)
    } catch (err) {
      console.error('Failed to fetch classes:', err)
    }
  }, [])

  useEffect(() => {
    refreshConnection()
    refreshClasses()
  }, [refreshConnection, refreshClasses])

  return (
    <AppContext.Provider
      value={{
        connectionStatus, realmId,
        availableClasses, refreshClasses,
        activeTab, setActiveTab,
        statusFilter, setStatusFilter,
        refreshConnection,
      }}
    >
      {children}
    </AppContext.Provider>
  )
}

export function useApp() {
  const ctx = useContext(AppContext)
  if (!ctx) throw new Error('useApp must be used within AppProvider')
  return ctx
}
