import { stateColor } from './colors'

const R = 200

function nodeRadius(n) {
  return Math.min(26, Math.floor((Math.PI * R) / n * 0.68))
}

function ringPositions(participants) {
  const n = participants.length
  return Object.fromEntries(
    participants.map((p, i) => {
      const a = (2 * Math.PI * i) / n - Math.PI / 2
      return [p.id, { x: R * Math.cos(a), y: R * Math.sin(a) }]
    })
  )
}

export default function Ring({ participants, daemonMap, selected, onSelect }) {
  if (!participants.length) return null

  const nr = nodeRadius(participants.length)
  const pos = ringPositions(participants)

  return (
    <svg
      viewBox="-300 -300 600 600"
      style={{ width: '100%', maxWidth: 580, height: 'auto' }}
    >
      <defs>
        <marker id="arr" markerWidth="7" markerHeight="7" refX="5" refY="3" orient="auto">
          <path d="M0,0 L0,6 L7,3 z" fill="#1e3a5f" />
        </marker>
      </defs>

      {/* Arrows — drawn behind nodes */}
      {participants.map((p) => {
        const f = pos[p.id]
        const t = pos[p.target]
        if (!f || !t) return null
        const dx = t.x - f.x
        const dy = t.y - f.y
        const dist = Math.sqrt(dx * dx + dy * dy)
        const ux = dx / dist
        const uy = dy / dist
        return (
          <line
            key={`arrow-${p.id}`}
            x1={f.x + ux * nr}
            y1={f.y + uy * nr}
            x2={t.x - ux * (nr + 8)}
            y2={t.y - uy * (nr + 8)}
            stroke="#1e3a5f"
            strokeWidth="1.5"
            markerEnd="url(#arr)"
          />
        )
      })}

      {/* Nodes */}
      {participants.map((p) => {
        const { x, y } = pos[p.id]
        const d = daemonMap[p.id]
        const state = d?.state ?? 'Initialized'
        const color = stateColor(state)
        const isLeader = d?.leader === p.id
        const isBtc = p.blockchain === 'Bitcoin'
        const confirmed = d?.confirmed_lock_txs.includes(p.id)
        const broadcast = d?.broadcast_lock_txs.includes(p.id)
        const isSelected = selected === p.id
        const label = `P${p.id}`
        const fs = Math.max(7, Math.floor(nr * (label.length > 2 ? 0.46 : 0.58)))

        return (
          <g
            key={p.id}
            transform={`translate(${x},${y})`}
            onClick={() => onSelect(isSelected ? null : p.id)}
            style={{ cursor: 'pointer' }}
          >
            {isSelected && (
              <circle
                r={nr + 9}
                fill="none"
                stroke="#f8fafc"
                strokeWidth="1.5"
                strokeDasharray="4 3"
                opacity="0.4"
              />
            )}
            <circle
              r={nr + 3}
              fill="none"
              stroke={isBtc ? '#f59e0b' : '#3b82f6'}
              strokeWidth={isSelected ? 2.5 : 1.5}
            />
            <circle r={nr} fill={color} opacity={0.92} />
            <text
              textAnchor="middle"
              dominantBaseline="central"
              fill="#fff"
              fontSize={fs}
              fontWeight="700"
              fontFamily="SF Mono, Consolas, monospace"
            >
              {label}
            </text>
            {isLeader && (
              <text
                textAnchor="middle"
                dominantBaseline="auto"
                y={-(nr + 5)}
                fill="#fbbf24"
                fontSize="11"
              >
                ★
              </text>
            )}
            {(confirmed || broadcast) && (
              <circle
                cx={nr * 0.65}
                cy={-nr * 0.65}
                r="4"
                fill={confirmed ? '#22c55e' : '#f59e0b'}
              />
            )}
          </g>
        )
      })}
    </svg>
  )
}
