import { useState, useEffect, useRef } from 'react'
import Ring from './Ring'
import { stateColor, stateShort } from './colors'

const C = {
  bg:       '#0f172a',
  surface:  '#1e293b',
  border:   '#1e293b',
  muted:    '#475569',
  text:     '#e2e8f0',
  textDim:  '#94a3b8',
}

const s = {
  root: { display: 'grid', gridTemplateRows: '48px 1fr', height: '100vh', overflow: 'hidden', background: C.bg, color: C.text, fontFamily: "'SF Mono', Consolas, monospace" },
  header: { display: 'flex', alignItems: 'center', gap: 10, padding: '0 20px', borderBottom: `1px solid ${C.border}`, fontSize: 13, flexShrink: 0 },
  title: { fontSize: 14, fontWeight: 700, color: '#f8fafc' },
  sessionInfo: { color: C.muted, fontSize: 12 },
  dot: { width: 7, height: 7, borderRadius: '50%', background: '#22c55e', flexShrink: 0, animation: 'pulse 2s infinite' },
  main: { display: 'grid', gridTemplateColumns: '1fr 320px', overflow: 'hidden', minHeight: 0 },
  ringWrap: { display: 'flex', alignItems: 'center', justifyContent: 'center', padding: 20, overflow: 'hidden' },
  aside: { borderLeft: `1px solid ${C.border}`, display: 'flex', flexDirection: 'column', overflow: 'hidden', minHeight: 0 },
  pane: { padding: 12, borderBottom: `1px solid ${C.border}`, overflowY: 'auto', flexShrink: 0, maxHeight: '40%' },
  paneTitle: { fontSize: 10, textTransform: 'uppercase', letterSpacing: '0.08em', color: C.muted, marginBottom: 8 },
  logWrap: { flex: 1, overflowY: 'auto', padding: 12, minHeight: 0 },
  logHeader: { display: 'flex', justifyContent: 'space-between', alignItems: 'center', marginBottom: 8 },
  btn: { fontSize: 11, padding: '3px 10px', borderRadius: 4, border: `1px solid ${C.border}`, background: 'transparent', color: C.textDim, cursor: 'pointer' },
  clearBtn: { fontSize: 10, padding: '2px 8px', borderRadius: 4, border: `1px solid ${C.border}`, background: 'transparent', color: C.muted, cursor: 'pointer' },
}

function Badge({ state }) {
  const color = stateColor(state)
  return (
    <span style={{ padding: '2px 8px', borderRadius: 4, fontSize: 11, background: color + '22', color, overflow: 'hidden', textOverflow: 'ellipsis', whiteSpace: 'nowrap', flex: 1 }}>
      {stateShort(state)}
    </span>
  )
}

function ParticipantRow({ p, d, isSelected, onClick }) {
  const state = d?.state ?? '—'
  const color = stateColor(state)
  const isBtc = p.blockchain === 'Bitcoin'
  const isLeader = d?.leader === p.id

  return (
    <div
      onClick={onClick}
      style={{ display: 'flex', alignItems: 'center', gap: 6, padding: '4px 6px', borderRadius: 6, cursor: 'pointer', background: isSelected ? '#1e293b' : 'transparent', marginBottom: 2 }}
    >
      <div style={{ width: 26, height: 26, borderRadius: '50%', display: 'flex', alignItems: 'center', justifyContent: 'center', fontWeight: 700, fontSize: 11, background: color, color: '#fff', flexShrink: 0 }}>
        {p.id}
      </div>
      <span style={{ fontSize: 10, padding: '1px 5px', borderRadius: 3, fontWeight: 600, flexShrink: 0, background: isBtc ? '#78350f' : '#1e3a8a', color: isBtc ? '#fcd34d' : '#93c5fd' }}>
        {isBtc ? 'BTC' : 'ADA'}
      </span>
      <Badge state={state} />
      {isLeader && <span style={{ color: '#fbbf24', fontSize: 12, flexShrink: 0 }} title="Leader">★</span>}
    </div>
  )
}

function DetailPanel({ p, d }) {
  if (!p || !d) return null
  const color = stateColor(d.state)
  return (
    <div style={{ padding: '10px 12px', borderBottom: `1px solid ${C.border}`, fontSize: 12 }}>
      <div style={{ fontWeight: 700, color: '#f8fafc', marginBottom: 8 }}>
        P{p.id} — {p.blockchain}
        {d.leader === p.id && <span style={{ color: '#fbbf24', marginLeft: 6 }}>★ Leader</span>}
      </div>

      <Row label="State">
        <span style={{ color, fontWeight: 600 }}>{d.state}</span>
      </Row>
      {d.leader != null && (
        <Row label="Leader"><span style={{ color: C.textDim }}>P{d.leader}</span></Row>
      )}
      <Row label="Lock broadcast">
        {d.broadcast_lock_txs.length
          ? d.broadcast_lock_txs.map(id => <Chip key={id} label={`P${id}`} color="#f59e0b" />)
          : <span style={{ color: C.muted }}>none</span>}
      </Row>
      <Row label="Lock confirmed">
        {d.confirmed_lock_txs.length
          ? d.confirmed_lock_txs.map(id => <Chip key={id} label={`P${id}`} color="#22c55e" />)
          : <span style={{ color: C.muted }}>none</span>}
      </Row>
      <Row label="Secrets">
        {d.secrets_received.length
          ? d.secrets_received.map(id => <Chip key={id} label={`P${id}`} color="#a855f7" />)
          : <span style={{ color: C.muted }}>none</span>}
      </Row>

      <div style={{ marginTop: 8 }}>
        <div style={{ fontSize: 10, textTransform: 'uppercase', letterSpacing: '0.08em', color: C.muted, marginBottom: 4 }}>State history</div>
        <div style={{ display: 'flex', flexWrap: 'wrap', gap: 3 }}>
          {d.state_history.map((st, i) => (
            <span key={i} style={{ fontSize: 10, padding: '1px 5px', borderRadius: 3, background: stateColor(st) + '33', color: stateColor(st) }}>
              {stateShort(st)}
            </span>
          ))}
        </div>
      </div>
    </div>
  )
}

function Row({ label, children }) {
  return (
    <div style={{ display: 'flex', alignItems: 'center', gap: 6, marginBottom: 4 }}>
      <span style={{ color: C.muted, fontSize: 11, width: 90, flexShrink: 0 }}>{label}</span>
      <div style={{ display: 'flex', flexWrap: 'wrap', gap: 3, alignItems: 'center' }}>{children}</div>
    </div>
  )
}

function Chip({ label, color }) {
  return (
    <span style={{ fontSize: 10, padding: '1px 5px', borderRadius: 3, background: color + '22', color, fontWeight: 600 }}>{label}</span>
  )
}

function LogEntry({ entry }) {
  const color = stateColor(entry.state)
  const time = new Date(entry.ts).toLocaleTimeString([], { hour: '2-digit', minute: '2-digit', second: '2-digit' })
  return (
    <div style={{ display: 'flex', gap: 6, padding: '2px 0', fontSize: 11 }}>
      <span style={{ color: C.muted, flexShrink: 0, width: 24 }}>P{entry.id}</span>
      <span style={{ color: '#334155', flexShrink: 0 }}>→</span>
      <span style={{ color, flex: 1 }}>{entry.state}</span>
      <span style={{ color: C.muted, fontSize: 10, flexShrink: 0 }}>{time}</span>
    </div>
  )
}

export default function App() {
  const [apiState, setApiState] = useState(null)
  const [paused, setPaused] = useState(false)
  const [selected, setSelected] = useState(null)
  const [logEntries, setLogEntries] = useState([])
  const prevHistRef = useRef({})
  const logRef = useRef([])

  useEffect(() => {
    if (paused) return

    const poll = async () => {
      try {
        const r = await fetch('http://127.0.0.1:3030/api/state')
        if (!r.ok) return
        const data = await r.json()
        setApiState(data)

        const newEntries = []
        for (const d of data.daemons) {
          const prev = prevHistRef.current[d.my_id] ?? []
          for (let i = prev.length; i < d.state_history.length; i++) {
            newEntries.push({ id: d.my_id, state: d.state_history[i], ts: Date.now() })
          }
          prevHistRef.current[d.my_id] = [...d.state_history]
        }

        if (newEntries.length > 0) {
          logRef.current = [...newEntries.reverse(), ...logRef.current].slice(0, 300)
          setLogEntries([...logRef.current])
        }
      } catch { /* daemon not yet started */ }
    }

    poll()
    const id = setInterval(poll, 600)
    return () => clearInterval(id)
  }, [paused])

  const daemonMap = apiState
    ? Object.fromEntries(apiState.daemons.map((d) => [d.my_id, d]))
    : {}

  const selectedParticipant = apiState?.participants.find((p) => p.id === selected)
  const selectedDaemon = selected != null ? daemonMap[selected] : null

  return (
    <>
      <style>{`
        @keyframes pulse { 0%,100%{opacity:1} 50%{opacity:.3} }
        ::-webkit-scrollbar { width: 4px; }
        ::-webkit-scrollbar-track { background: transparent; }
        ::-webkit-scrollbar-thumb { background: #334155; border-radius: 2px; }
      `}</style>
      <div style={s.root}>
        <header style={s.header}>
          <div style={s.dot} />
          <h1 style={s.title}>Swap Dashboard</h1>
          {apiState && (
            <span style={s.sessionInfo}>
              Session {apiState.session_id} · {apiState.participants.length} participants
            </span>
          )}
          <div style={{ flex: 1 }} />
          <button
            style={{ ...s.btn, color: paused ? '#f97316' : C.textDim }}
            onClick={() => setPaused((p) => !p)}
          >
            {paused ? '▶ Resume' : '⏸ Pause'}
          </button>
        </header>

        <main style={s.main}>
          <div style={s.ringWrap}>
            {apiState && (
              <Ring
                participants={apiState.participants}
                daemonMap={daemonMap}
                selected={selected}
                onSelect={setSelected}
              />
            )}
            {!apiState && (
              <span style={{ color: C.muted, fontSize: 13 }}>Waiting for daemon…</span>
            )}
          </div>

          <aside style={s.aside}>
            <div style={s.pane}>
              <div style={s.paneTitle}>Participants</div>
              {(apiState?.participants ?? [])
                .slice()
                .sort((a, b) => a.id - b.id)
                .map((p) => (
                  <ParticipantRow
                    key={p.id}
                    p={p}
                    d={daemonMap[p.id]}
                    isSelected={selected === p.id}
                    onClick={() => setSelected(selected === p.id ? null : p.id)}
                  />
                ))}
            </div>

            {selected != null && (
              <DetailPanel p={selectedParticipant} d={selectedDaemon} />
            )}

            <div style={s.logWrap}>
              <div style={s.logHeader}>
                <div style={s.paneTitle}>State Log</div>
                <button
                  style={s.clearBtn}
                  onClick={() => { logRef.current = []; setLogEntries([]) }}
                >
                  Clear
                </button>
              </div>
              {logEntries.map((e, i) => (
                <LogEntry key={i} entry={e} />
              ))}
              {logEntries.length === 0 && (
                <span style={{ color: C.muted, fontSize: 11 }}>No transitions yet…</span>
              )}
            </div>
          </aside>
        </main>
      </div>
    </>
  )
}
