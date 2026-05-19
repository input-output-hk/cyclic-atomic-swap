export const STATE_COLORS = {
  Initialized:                '#6b7280',
  AwaitingAdaptorPoints:      '#2563eb',
  AwaitingLeaderCommitments:  '#1d4ed8',
  AwaitingLeaderNonces:       '#1e40af',
  RefundTxsSigning:           '#d97706',
  Funding:                    '#ea580c',
  AwaitingLockConfirmations:  '#f97316',
  AwaitingSecrets:            '#9333ea',
  AwaitingLeaderSpend:        '#a855f7',
  Claiming:                   '#4ade80',
  Completed:                  '#16a34a',
  Refunded:                   '#7c3aed',
  Failed:                     '#dc2626',
}

export const STATE_SHORT = {
  Initialized:                'Init',
  AwaitingAdaptorPoints:      'AwaitAdaptor',
  AwaitingLeaderCommitments:  'AwaitCommit',
  AwaitingLeaderNonces:       'AwaitNonce',
  RefundTxsSigning:           'SignRefund',
  Funding:                    'Funding',
  AwaitingLockConfirmations:  'AwaitLock',
  AwaitingSecrets:            'AwaitSecret',
  AwaitingLeaderSpend:        'AwaitSpend',
  Claiming:                   'Claiming',
  Completed:                  'Completed',
  Refunded:                   'Refunded',
  Failed:                     'Failed',
}

export const stateColor = (s) => STATE_COLORS[s] ?? '#6b7280'
export const stateShort = (s) => STATE_SHORT[s] ?? s
