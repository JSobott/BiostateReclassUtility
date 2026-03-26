const BASE = ''

async function fetchJSON(url, options = {}) {
  const res = await fetch(`${BASE}${url}`, {
    headers: { 'Content-Type': 'application/json', ...options.headers },
    ...options,
  })
  if (!res.ok) {
    const body = await res.text()
    throw new Error(`${res.status}: ${body}`)
  }
  return res.json()
}

async function postJSON(url, data) {
  return fetchJSON(url, { method: 'POST', body: JSON.stringify(data) })
}

// Transactions
export const fetchGroups = (status = 'Pending') =>
  fetchJSON(`/api/groups?status=${encodeURIComponent(status)}`)

export const fetchGroupItems = (status, account, entityName, suggestedClass) =>
  fetchJSON(
    `/api/group-items?status=${encodeURIComponent(status)}&account=${encodeURIComponent(account)}&entity_name=${encodeURIComponent(entityName)}&suggested_class=${encodeURIComponent(suggestedClass)}`
  )

export const fetchPending = () => fetchJSON('/api/pending')

export const batchApprove = (account, entityName, currentClass, overrideClass) =>
  postJSON('/api/batch-approve', {
    account,
    entity_name: entityName,
    current_class: currentClass,
    override_class: overrideClass || null,
  })

export const approveTransaction = (txId, lineId, suggestedClassRef) =>
  postJSON('/api/approve', { tx_id: txId, line_id: lineId, suggested_class_ref: suggestedClassRef })

export const resetPending = (account, entityName, currentClass) =>
  postJSON('/api/reset-pending', { account, entity_name: entityName, current_class: currentClass })

// Classes
export const fetchClasses = () => fetchJSON('/api/classes')

// Rules
export const fetchRules = () => fetchJSON('/api/rules')
export const createRule = (rule) => postJSON('/api/rules', rule)
export const deleteRule = (id) => postJSON('/api/rules/delete', { id })

// Writeback
export const triggerWriteback = (dryRun) => postJSON('/api/writeback', { dry_run: dryRun })

export function subscribeWritebackProgress(onMessage) {
  const es = new EventSource('/api/writeback-progress')
  es.onmessage = (e) => {
    const data = JSON.parse(e.data)
    onMessage(data)
    if (data.status === 'done' || data.status === 'error') {
      es.close()
    }
  }
  es.onerror = () => {
    es.close()
  }
  return es
}

// Sync
export const triggerSync = (startDate, endDate) =>
  postJSON('/api/sync', { start_date: startDate || null, end_date: endDate || null })

export function subscribeSyncProgress(onMessage) {
  const es = new EventSource('/api/sync-progress')
  es.onmessage = (e) => {
    const data = JSON.parse(e.data)
    onMessage(data)
    if (data.status === 'done' || data.status === 'error') {
      es.close()
    }
  }
  es.onerror = () => {
    es.close()
  }
  return es
}

// Audit
export const fetchAuditLogs = () => fetchJSON('/api/audit-logs')

// Diagnostics
export const fetchDiagnosticsStatus = () => fetchJSON('/api/diagnostics/status')
export const fetchCompanyInfo = () => fetchJSON('/api/diagnostics/company-info')
export const refreshToken = () => postJSON('/api/diagnostics/refresh', {})
export const seedTokens = (accessToken, refreshTokenVal, realmId) =>
  postJSON('/api/diagnostics/seed-tokens', {
    access_token: accessToken,
    refresh_token: refreshTokenVal,
    realm_id: realmId,
  })
