// -*- mode: Bluespec; -*-
//
// Full protocol model — flat-action / Apalache-friendly variant.  The
// network is abstracted to per-recipient inboxes that the protocol
// guards actually consume; the passage of time is modelled by a single
// stutter-style action `openNextWindow` that fires only when no other
// action is enabled.
//
// Network abstraction (post Layer-2 — selective per-recipient sends + equivocation)
// ──────────────────────────────────────────────────────────────────────────────
// Off-chain messages are tracked per recipient via three state vars
// per kind:
//
//   adaptorReceivedAt  : Party -> Set[Party]  — r ↦ senders whose adaptor r received
//   adaptorDecided     : Set[Party]           — senders who have committed
//   adaptorEquivocated : Party -> Set[Party]  — r ↦ senders whose adaptor r got a divergent version
//   keyReceivedAt      : Party -> Set[Party]  — round-1 keys
//   keyDecided         : Set[Party]
//   keyEquivocated     : Party -> Set[Party]
//   nonceReceivedAt    : Party -> Set[Party]  — round-2 nonces
//   nonceDecided       : Set[Party]
//   nonceEquivocated   : Party -> Set[Party]
//   tAggReceivedAt     : Party -> Set[Party]  — r ↦ depositors whose T_agg r received
//   tAggDecided        : Set[Party]           — depositors d for which recipientOf(d) decided
//   tAggEquivocated    : Party -> Set[Party]  — r ↦ depositors d for which r got a divergent T_agg
//   secretReceivedAt   : Party -> Set[Party]  — r ↦ senders whose secret r received
//   secretDecided      : Set[Party]           — non-leaders who have committed
//   secretEquivocated  : Party -> Set[Party]
//   preSigs            : Party -> Set[Party]  — preSigs[signer] = depositors signed for
//   coSigs             : Party -> Set[Party]  — coSigs[signer]  = depositors signed for
//   triggerBroadcast   : bool                 — leader has published MsgTrigger
//
// Each off-chain send is one-shot: a sender fires `sendXTo(p, recipients, divergent)`
// exactly once, committing to a recipient subset and to a divergence subset.
// Honest senders are constrained to `recipients == PARTIES` and `divergent == Set()`;
// dishonest senders pick any `recipients ⊆ PARTIES` and any `divergent ⊆ recipients`.
// The `XDecided` flag records commitment; after committing, the send action is
// permanently disabled, so `weakFair(stepBy(p), …)` is satisfied by the single
// fire without forcing dishonest senders to eventually deliver to everyone.
//
// Equivocation semantics: `divergent` is the set of recipients to whom the
// attacker delivered an alternate payload.  An honest recipient r refuses to
// advance past the phase consuming kind X if `XEquivocated[r] != ∅` — modelling
// downstream detection (commit-and-reveal / hash-commitment checks).  The model
// over-approximates the attacker (it doesn't require some other recipient to
// have received the canonical version), which is sound: any real adversary is
// at least as restricted.
//
// State-space reduction: `divergent ⊆ recipients ∩ HONEST`.  Equivocating to
// a dishonest recipient is operationally inert — only honest parties consult
// `*Equivocated` (advance gates and the (E) invariants).  Restricting the
// attacker to equivocate only against honest recipients is behavior-preserving
// (any real adversary can choose to equivocate against dishonest recipients
// too, but doing so changes no observable behavior) and prunes branching
// from 2^|recipients| to 2^|recipients ∩ HONEST| per send.
//
// The crypto verification checks (verifyTAgg, adaptor-point matching) are
// vacuous in this abstraction; we model selective sends + equivocation
// faithfully and assume cryptographic correctness otherwise.
//
// Time and refund windows
// ───────────────────────
// Block-height progression is modelled as a single parameterless action
// `openNextWindow` that opens the unopened refund window with the smallest
// `W` and is enabled iff no other action is enabled.  Refund windows
// therefore advance precisely when the protocol has run out of progress
// steps — capturing the spec's intent that refund is a fallback path,
// and structurally encoding the §2.4 Δ-gap: while any honest party can
// still claim or advance setup, windows cannot open further.
//
// Adversarial behaviour
// ─────────────────────
// Off-chain selective withholding is modelled by `sendXTo(p, recipients)`
// with `recipients ⊊ PARTIES` — including the empty set, which is the
// uniform "withhold from everyone" decision.  On-chain decisions still
// need explicit withhold actions (no recipients dimension):
//
//   withholdLock    — depositor declines to lock their deposit
//   withholdClaim   — non-leader claimant declines to broadcast its withdraw
//   withholdTrigger — dishonest leader declines to broadcast the trigger
//
// (The previous `withholdSecret` is subsumed by `sendSecretTo(p, Set())`.)
//
// Run:
//   quint run     --invariant=invAtomicity        protocolFlat.qnt
//   quint verify  --inductive-invariant=invAtomicityInductive --invariant=invAtomicity protocolFlat.qnt

module protocolFlat {

  import basicSpells.*  from "spells/basicSpells"
  import customSpells.* from "spells/customSpells"
  import types.*        from "types"
  import parameters.*   from "parameters"

  // ─── State ────────────────────────────────────────────────────────────────

  var phases:            Party -> Phase
  var assets:            Party -> AssetStatus
  var adaptorReceivedAt:  Party -> Set[Party]
  var adaptorDecided:     Set[Party]
  var adaptorEquivocated: Party -> Set[Party]   // r ↦ senders whose adaptor r received a divergent version
  var keyReceivedAt:      Party -> Set[Party]
  var keyDecided:         Set[Party]
  var keyEquivocated:     Party -> Set[Party]
  var nonceReceivedAt:    Party -> Set[Party]
  var nonceDecided:       Set[Party]
  var nonceEquivocated:   Party -> Set[Party]
  var tAggReceivedAt:     Party -> Set[Party]
  var tAggDecided:        Set[Party]
  var tAggEquivocated:    Party -> Set[Party]   // inner set is depositors d
  var preSigs:            Party -> Set[Party]
  var coSigs:             Party -> Set[Party]
  var secretReceivedAt:   Party -> Set[Party]
  var secretDecided:      Set[Party]
  var secretEquivocated:  Party -> Set[Party]
  var triggerBroadcast:  bool
  var openWindows:       Set[Party]
  var claimsFinalized:   bool
  // Terminal facts: a party may end up in *both* (its own deposit refunded
  // and its incoming claim confirmed) since they are independent on-chain
  // transactions on different UTXOs.  Modelled as separate sets rather than
  // mutually exclusive PhaseDone / PhaseRefund.
  var refunded:          Set[Party]
  var claimConfirmed:    Set[Party]
  // Per-party count of reorgs of that party's published claim of its
  // incoming.  Capped at MAX_REORGS (parameters.qnt) — captures finality
  // depth (§2.4): once Δ blocks elapse a transaction is final and cannot
  // be reorged again.
  var reorgCount:        Party -> int
  // Withhold flags for on-chain actions (no recipients dimension): monotonic
  // sets of dishonest commitments not to act.  Off-chain withholding is
  // expressed via `sendXTo(p, Set())` and no longer needs dedicated flags.
  var withheldLocks:     Set[Party]
  var withheldClaims:    Set[Party]
  var withheldTrigger:   bool

  // ─── Init ─────────────────────────────────────────────────────────────────

  action init: bool = all {
    phases'            = PARTIES.mapBy(_ => PhaseConfig),
    assets'            = PARTIES.mapBy(p => OwnedBy(p)),
    adaptorReceivedAt'  = PARTIES.mapBy(_ => Set()),
    adaptorDecided'     = Set(),
    adaptorEquivocated' = PARTIES.mapBy(_ => Set()),
    keyReceivedAt'      = PARTIES.mapBy(_ => Set()),
    keyDecided'         = Set(),
    keyEquivocated'     = PARTIES.mapBy(_ => Set()),
    nonceReceivedAt'    = PARTIES.mapBy(_ => Set()),
    nonceDecided'       = Set(),
    nonceEquivocated'   = PARTIES.mapBy(_ => Set()),
    tAggReceivedAt'     = PARTIES.mapBy(_ => Set()),
    tAggDecided'        = Set(),
    tAggEquivocated'    = PARTIES.mapBy(_ => Set()),
    preSigs'            = PARTIES.mapBy(_ => Set()),
    coSigs'             = PARTIES.mapBy(_ => Set()),
    secretReceivedAt'   = PARTIES.mapBy(_ => Set()),
    secretDecided'      = Set(),
    secretEquivocated'  = PARTIES.mapBy(_ => Set()),
    triggerBroadcast'  = false,
    openWindows'       = Set(),
    claimsFinalized'   = false,
    refunded'          = Set(),
    claimConfirmed'    = Set(),
    reorgCount'        = PARTIES.mapBy(_ => 0),
    withheldLocks'     = Set(),
    withheldClaims'    = Set(),
    withheldTrigger'   = false,
  }

  // ─── Helpers ──────────────────────────────────────────────────────────────

  // Per-recipient round-completion predicates.
  pure def allAdaptorPointsPresentFor(r: Party, m: Party -> Set[Party]): bool =
    m.get(r) == PARTIES
  pure def allKeysPresentFor(r: Party, m: Party -> Set[Party]): bool =
    m.get(r) == PARTIES
  pure def allNoncesPresentFor(r: Party, m: Party -> Set[Party]): bool =
    m.get(r) == PARTIES

  // Honest equivocation detection: r's inbox for this kind contains no
  // divergent-payload markers.  Honest parties refuse to advance past a
  // phase consuming kind X unless this holds for X at r.
  pure def noEquivocationAt(r: Party, m: Party -> Set[Party]): bool =
    m.get(r) == Set()

  // roundDoneFor(p, m): every depositor signed for by p, AND every signer
  // signed for depositor p (p's row + p's column in the signature matrix m).
  // Used for both the Round 3 pre-signature matrix and the Round 4 co-signature
  // matrix — same shape, different signatures.
  pure def roundDoneFor(p: Party, m: Party -> Set[Party]): bool =
    PARTIES.forall(d => m.get(p).contains(d))
    and PARTIES.forall(q => m.get(q).contains(p))

  // Honest-liveness: every honest party's incoming has been moved past the
  // race window.
  val honestClaimsDone: bool =
    HONEST.forall(p =>
      match assets.get(senderOf(p)) {
        | PendingClaim(_) => true
        | Claimed(_)      => true
        | _               => false
      })

  val anyClaimed:  bool = PARTIES.exists(p => match assets.get(p) { | Claimed(_)  => true | _ => false })
  val anyRefunded: bool = PARTIES.exists(p => match assets.get(p) { | Refunded(_) => true | _ => false })

  // claimedByNext(p): p's deposit has been claimed (or is awaiting finality)
  // by the legitimate next party in the cycle.
  pure def claimedByNext(p: Party, assets: Party -> AssetStatus): bool =
    match assets.get(p) {
      | Claimed(q)      => q == recipientOf(p)
      | PendingClaim(q) => q == recipientOf(p)
      | _               => false
    }

  // iClaimedIncoming(p): p's incoming transfer (the deposit at senderOf(p))
  // has been (or is being) claimed by p specifically.
  pure def iClaimedIncoming(p: Party, assets: Party -> AssetStatus): bool =
    match assets.get(senderOf(p)) {
      | Claimed(q)      => q == p
      | PendingClaim(q) => q == p
      | _               => false
    }

  // ─── Enabling predicates ──────────────────────────────────────────────────
  //
  // One predicate per action; reused in (i) the action's own guard and
  // (ii) `noOtherActionEnabled`, the gate on `openNextWindow`.  Keeping the
  // guards in a single place means the gate cannot drift from the actions.

  def config_enabled(p: Party): bool =
    phases.get(p) == PhaseConfig

  def sendAdaptorPointTo_enabled(p: Party): bool = and {
    phases.get(p) == PhaseSetupAdaptorBroadcast,
    not(adaptorDecided.contains(p)),
  }

  def adaptorBroadcastAdvance_enabled(p: Party): bool = and {
    phases.get(p) == PhaseSetupAdaptorBroadcast,
    allAdaptorPointsPresentFor(p, adaptorReceivedAt),
    HONEST.contains(p) implies noEquivocationAt(p, adaptorEquivocated),
  }

  def sendKeyTo_enabled(p: Party): bool = and {
    phases.get(p) == PhaseSetupKey1,
    not(keyDecided.contains(p)),
  }

  def key1Advance_enabled(p: Party): bool = and {
    phases.get(p) == PhaseSetupKey1,
    allKeysPresentFor(p, keyReceivedAt),
    HONEST.contains(p) implies noEquivocationAt(p, keyEquivocated),
  }

  def sendNonceTo_enabled(p: Party): bool = and {
    phases.get(p) == PhaseSetupKey2,
    not(nonceDecided.contains(p)),
  }

  def key2Advance_enabled(p: Party): bool = and {
    phases.get(p) == PhaseSetupKey2,
    allNoncesPresentFor(p, nonceReceivedAt),
    HONEST.contains(p) implies noEquivocationAt(p, nonceEquivocated),
  }

  def sendTAggTo_enabled(d: Party): bool = and {
    phases.get(recipientOf(d)) == PhaseSetupKey3,
    allAdaptorPointsPresentFor(recipientOf(d), adaptorReceivedAt),
    not(tAggDecided.contains(d)),
  }

  def sendPreSig_enabled(p: Party, d: Party): bool = and {
    phases.get(p) == PhaseSetupKey3,
    allAdaptorPointsPresentFor(p, adaptorReceivedAt),
    tAggReceivedAt.get(p).contains(d),
    not(preSigs.get(p).contains(d)),
    // Honest signers refuse to pre-sign when they detected equivocation on
    // either adaptor points (which feed every signature) or this depositor's
    // T_agg specifically.
    HONEST.contains(p) implies and {
      noEquivocationAt(p, adaptorEquivocated),
      not(tAggEquivocated.get(p).contains(d)),
    },
  }

  def key3Advance_enabled(p: Party): bool = and {
    phases.get(p) == PhaseSetupKey3,
    roundDoneFor(p, preSigs),
    HONEST.contains(p) implies noEquivocationAt(p, tAggEquivocated),
  }

  def sendCoSig_enabled(q: Party, d: Party): bool = and {
    phases.get(q) == PhaseSetupSignInTx,
    not(coSigs.get(q).contains(d)),
    (recipientOf(d) == LEADER and WINDOWS.get(d) < WINDOWS.get(recipientOf(d)))
      or (recipientOf(d) != LEADER and WINDOWS.get(d) > WINDOWS.get(recipientOf(d))),
    recipientOf(d) == LEADER or WINDOWS.get(d) - WINDOWS.get(recipientOf(d)) >= DELTA,
    // Honest co-signers refuse to co-sign if any of the inputs feeding
    // d's signature were equivocated against q.
    HONEST.contains(q) implies and {
      noEquivocationAt(q, adaptorEquivocated),
      noEquivocationAt(q, keyEquivocated),
      noEquivocationAt(q, nonceEquivocated),
      not(tAggEquivocated.get(q).contains(d)),
    },
  }

  def signInTxAdvance_enabled(p: Party): bool = and {
    phases.get(p) == PhaseSetupSignInTx,
    roundDoneFor(p, coSigs),
  }

  // Lock guard: honest parties refuse to lock once any window is open
  // (they would be putting principal at risk with no path to claim).
  // Dishonest parties are free to lock at any time before withholding.
  def lock_enabled(p: Party): bool = and {
    phases.get(p) == PhaseLock,
    assets.get(p) == OwnedBy(p),
    HONEST.contains(p) implies openWindows == Set(),
    not(withheldLocks.contains(p)),
  }

  def advanceToClaim_enabled(p: Party): bool = and {
    phases.get(p) == PhaseLock,
    allLocked(assets),
  }

  def sendSecretTo_enabled(p: Party): bool = and {
    phases.get(p) == PhaseClaim,
    p != LEADER,
    not(secretDecided.contains(p)),
  }

  val trigger_enabled: bool = and {
    phases.get(LEADER) == PhaseClaim,
    not(triggerBroadcast),
    PARTIES.exclude(Set(LEADER)).forall(p =>
      secretReceivedAt.get(LEADER).contains(p)),
    assets.get(senderOf(LEADER)) == Locked,
    not(withheldTrigger),
    // Honest leader refuses to trigger on equivocated secret reveals.
    HONEST.contains(LEADER) implies noEquivocationAt(LEADER, secretEquivocated),
  }

  def claim_enabled(p: Party): bool = and {
    phases.get(p) == PhaseClaim,
    triggerBroadcast,
    assets.get(senderOf(p)) == Locked,
    not(withheldClaims.contains(p)),
  }

  def confirmClaim_enabled(p: Party): bool = and {
    assets.get(senderOf(p)) == PendingClaim(p),
    claimsFinalized,
    not(claimConfirmed.contains(p)),
  }

  def reorgClaim_enabled(p: Party): bool = and {
    assets.get(senderOf(p)) == PendingClaim(p),
    not(claimsFinalized),
    reorgCount.get(p) < MAX_REORGS,
  }

  val finalizeClaims_enabled: bool = and {
    not(claimsFinalized),
    triggerBroadcast,
    honestClaimsDone,
  }

  // Refund guard.  Refund is enabled once p's window is open and p's
  // deposit is still Locked (not yet claimed or refunded).  No explicit
  // Δ-gap conjunct is needed here: `openNextWindow` is gated by
  // `noOtherActionEnabled`, so an honest recipient that can still claim
  // from p suppresses further window openings, and `claim` /
  // `confirmClaim` move the asset out of `Locked` before refund can
  // become enabled.
  def refund_enabled(p: Party): bool = and {
    phases.get(p) == PhaseLock or phases.get(p) == PhaseClaim,
    assets.get(p) == Locked,
    openWindows.contains(p),
    not(refunded.contains(p)),
  }

  // Withholds — only available to dishonest parties.
  // (withholdSecret is subsumed by sendSecretTo(p, Set()).)

  def withholdLock_enabled(p: Party): bool = and {
    not(HONEST.contains(p)),
    phases.get(p) == PhaseLock,
    assets.get(p) == OwnedBy(p),
    not(withheldLocks.contains(p)),
  }

  def withholdClaim_enabled(p: Party): bool = and {
    not(HONEST.contains(p)),
    phases.get(p) == PhaseClaim,
    triggerBroadcast,
    assets.get(senderOf(p)) == Locked,
    not(withheldClaims.contains(p)),
  }

  val withholdTrigger_enabled: bool = and {
    not(HONEST.contains(LEADER)),
    phases.get(LEADER) == PhaseClaim,
    not(triggerBroadcast),
    not(withheldTrigger),
  }

  // The gate on `openNextWindow`: every protocol action and every
  // withhold action is disabled.  Time advances iff the model is otherwise
  // stuck.
  val noOtherActionEnabled: bool = and {
    not(PARTIES.exists(p => config_enabled(p))),
    not(PARTIES.exists(p => sendAdaptorPointTo_enabled(p))),
    not(PARTIES.exists(p => adaptorBroadcastAdvance_enabled(p))),
    not(PARTIES.exists(p => sendKeyTo_enabled(p))),
    not(PARTIES.exists(p => key1Advance_enabled(p))),
    not(PARTIES.exists(p => sendNonceTo_enabled(p))),
    not(PARTIES.exists(p => key2Advance_enabled(p))),
    not(PARTIES.exists(d => sendTAggTo_enabled(d))),
    not(PARTIES.exists(p => PARTIES.exists(d => sendPreSig_enabled(p, d)))),
    not(PARTIES.exists(p => key3Advance_enabled(p))),
    not(PARTIES.exists(q => PARTIES.exists(d => sendCoSig_enabled(q, d)))),
    not(PARTIES.exists(p => signInTxAdvance_enabled(p))),
    not(PARTIES.exists(p => lock_enabled(p))),
    not(PARTIES.exists(p => advanceToClaim_enabled(p))),
    not(PARTIES.exists(p => sendSecretTo_enabled(p))),
    not(trigger_enabled),
    not(PARTIES.exists(p => claim_enabled(p))),
    not(PARTIES.exists(p => confirmClaim_enabled(p))),
    not(PARTIES.exists(p => reorgClaim_enabled(p))),
    not(finalizeClaims_enabled),
    not(PARTIES.exists(p => refund_enabled(p))),
    not(PARTIES.exists(p => withholdLock_enabled(p))),
    not(PARTIES.exists(p => withholdClaim_enabled(p))),
    not(withholdTrigger_enabled),
  }

  // ─── Frames ───────────────────────────────────────────────────────────────
  //
  // Named "unchanged" actions composed into each transition via `all { ... }`.
  // The compiler inlines them, so Apalache and TLC both see plain `x' = x`
  // conjuncts.  Each atomic frame asserts exactly one primed equation; the
  // aggregates bundle the variables that move as a cluster.  `phases` and
  // `assets` are kept inline in each action.

  action keepAdaptor:         bool = all {
    adaptorReceivedAt'  = adaptorReceivedAt,
    adaptorDecided'     = adaptorDecided,
    adaptorEquivocated' = adaptorEquivocated,
  }
  action keepKeys:            bool = all {
    keyReceivedAt'      = keyReceivedAt,
    keyDecided'         = keyDecided,
    keyEquivocated'     = keyEquivocated,
  }
  action keepNonces:          bool = all {
    nonceReceivedAt'    = nonceReceivedAt,
    nonceDecided'       = nonceDecided,
    nonceEquivocated'   = nonceEquivocated,
  }
  action keepTAgg:            bool = all {
    tAggReceivedAt'     = tAggReceivedAt,
    tAggDecided'        = tAggDecided,
    tAggEquivocated'    = tAggEquivocated,
  }
  action keepPreSigs:         bool = preSigs'          = preSigs
  action keepCoSigs:          bool = coSigs'           = coSigs
  action keepSecrets:         bool = all {
    secretReceivedAt'   = secretReceivedAt,
    secretDecided'      = secretDecided,
    secretEquivocated'  = secretEquivocated,
  }
  action keepTrigger:         bool = triggerBroadcast' = triggerBroadcast
  action keepFinalized:       bool = claimsFinalized'  = claimsFinalized
  action keepConfirmed:       bool = claimConfirmed'   = claimConfirmed
  action keepOpen:            bool = openWindows'      = openWindows
  action keepRefunded:        bool = refunded'         = refunded
  action keepReorg:           bool = reorgCount'       = reorgCount
  action keepWLocks:          bool = withheldLocks'    = withheldLocks
  action keepWClaims:         bool = withheldClaims'   = withheldClaims
  action keepWTrigger:        bool = withheldTrigger'  = withheldTrigger

  action keepSetupBroadcasts: bool = all {
    keepAdaptor, keepKeys, keepNonces, keepTAgg, keepPreSigs, keepCoSigs,
  }
  action keepClaimFlags: bool = all {
    keepSecrets, keepTrigger, keepFinalized, keepConfirmed,
  }
  action keepRefundState: bool = all {
    keepOpen, keepRefunded, keepReorg,
  }
  action keepWithholds: bool = all {
    keepWLocks, keepWClaims, keepWTrigger,
  }

  // ─── Transitions ──────────────────────────────────────────────────────────

  action config(p: Party): bool = all {
    config_enabled(p),
    phases' = phases.set(p, PhaseSetupAdaptorBroadcast),
    // UNCHANGED
    assets' = assets,
    keepSetupBroadcasts,
    keepClaimFlags,
    keepRefundState,
    keepWithholds,
  }

  action sendAdaptorPointTo(p: Party, recipients: Set[Party], divergent: Set[Party]): bool = all {
    sendAdaptorPointTo_enabled(p),
    HONEST.contains(p) implies recipients == PARTIES,
    HONEST.contains(p) implies divergent == Set(),
    divergent.subseteq(recipients.intersect(HONEST)),
    adaptorReceivedAt' = PARTIES.mapBy(r =>
      if (recipients.contains(r))
        adaptorReceivedAt.get(r).union(Set(p))
      else
        adaptorReceivedAt.get(r)),
    adaptorEquivocated' = PARTIES.mapBy(r =>
      if (divergent.contains(r))
        adaptorEquivocated.get(r).union(Set(p))
      else
        adaptorEquivocated.get(r)),
    adaptorDecided' = adaptorDecided.union(Set(p)),
    // UNCHANGED
    phases' = phases,
    assets' = assets,
    keepKeys, keepNonces, keepTAgg, keepPreSigs, keepCoSigs,
    keepClaimFlags,
    keepRefundState,
    keepWithholds,
  }

  action adaptorBroadcastAdvance(p: Party): bool = all {
    adaptorBroadcastAdvance_enabled(p),
    phases' = phases.set(p, PhaseSetupKey1),
    // UNCHANGED
    assets' = assets,
    keepSetupBroadcasts,
    keepClaimFlags,
    keepRefundState,
    keepWithholds,
  }

  action sendKeyTo(p: Party, recipients: Set[Party], divergent: Set[Party]): bool = all {
    sendKeyTo_enabled(p),
    HONEST.contains(p) implies recipients == PARTIES,
    HONEST.contains(p) implies divergent == Set(),
    divergent.subseteq(recipients.intersect(HONEST)),
    keyReceivedAt' = PARTIES.mapBy(r =>
      if (recipients.contains(r))
        keyReceivedAt.get(r).union(Set(p))
      else
        keyReceivedAt.get(r)),
    keyEquivocated' = PARTIES.mapBy(r =>
      if (divergent.contains(r))
        keyEquivocated.get(r).union(Set(p))
      else
        keyEquivocated.get(r)),
    keyDecided' = keyDecided.union(Set(p)),
    // UNCHANGED
    phases' = phases,
    assets' = assets,
    keepAdaptor, keepNonces, keepTAgg, keepPreSigs, keepCoSigs,
    keepClaimFlags,
    keepRefundState,
    keepWithholds,
  }

  action key1Advance(p: Party): bool = all {
    key1Advance_enabled(p),
    phases' = phases.set(p, PhaseSetupKey2),
    // UNCHANGED
    assets' = assets,
    keepSetupBroadcasts,
    keepClaimFlags,
    keepRefundState,
    keepWithholds,
  }

  action sendNonceTo(p: Party, recipients: Set[Party], divergent: Set[Party]): bool = all {
    sendNonceTo_enabled(p),
    HONEST.contains(p) implies recipients == PARTIES,
    HONEST.contains(p) implies divergent == Set(),
    divergent.subseteq(recipients.intersect(HONEST)),
    nonceReceivedAt' = PARTIES.mapBy(r =>
      if (recipients.contains(r))
        nonceReceivedAt.get(r).union(Set(p))
      else
        nonceReceivedAt.get(r)),
    nonceEquivocated' = PARTIES.mapBy(r =>
      if (divergent.contains(r))
        nonceEquivocated.get(r).union(Set(p))
      else
        nonceEquivocated.get(r)),
    nonceDecided' = nonceDecided.union(Set(p)),
    // UNCHANGED
    phases' = phases,
    assets' = assets,
    keepAdaptor, keepKeys, keepTAgg, keepPreSigs, keepCoSigs,
    keepClaimFlags,
    keepRefundState,
    keepWithholds,
  }

  action key2Advance(p: Party): bool = all {
    key2Advance_enabled(p),
    phases' = phases.set(p, PhaseSetupKey3),
    // UNCHANGED
    assets' = assets,
    keepSetupBroadcasts,
    keepClaimFlags,
    keepRefundState,
    keepWithholds,
  }

  // T_agg(d) is broadcast by the claimant recipientOf(d).  Honesty of the
  // claimant constrains the recipients subset; one-shot per depositor d.
  action sendTAggTo(d: Party, recipients: Set[Party], divergent: Set[Party]): bool = all {
    sendTAggTo_enabled(d),
    HONEST.contains(recipientOf(d)) implies recipients == PARTIES,
    HONEST.contains(recipientOf(d)) implies divergent == Set(),
    divergent.subseteq(recipients.intersect(HONEST)),
    tAggReceivedAt' = PARTIES.mapBy(r =>
      if (recipients.contains(r))
        tAggReceivedAt.get(r).union(Set(d))
      else
        tAggReceivedAt.get(r)),
    tAggEquivocated' = PARTIES.mapBy(r =>
      if (divergent.contains(r))
        tAggEquivocated.get(r).union(Set(d))
      else
        tAggEquivocated.get(r)),
    tAggDecided' = tAggDecided.union(Set(d)),
    // UNCHANGED
    phases' = phases,
    assets' = assets,
    keepAdaptor, keepKeys, keepNonces, keepPreSigs, keepCoSigs,
    keepClaimFlags,
    keepRefundState,
    keepWithholds,
  }

  action sendPreSig(p: Party, d: Party): bool = all {
    sendPreSig_enabled(p, d),
    preSigs' = preSigs.set(p, preSigs.get(p).union(Set(d))),
    // UNCHANGED
    phases' = phases,
    assets' = assets,
    keepAdaptor, keepKeys, keepNonces, keepTAgg, keepCoSigs,
    keepClaimFlags,
    keepRefundState,
    keepWithholds,
  }

  action key3Advance(p: Party): bool = all {
    key3Advance_enabled(p),
    phases' = phases.set(p, PhaseSetupSignInTx),
    // UNCHANGED
    assets' = assets,
    keepSetupBroadcasts,
    keepClaimFlags,
    keepRefundState,
    keepWithholds,
  }

  // Co-signer q co-signs for depositor d.  Window/gap conditions baked into
  // the parameter constraints (windowsOrdered) so the spec-level ordering
  // checks are vacuous here, but we keep the per-d explicit step.
  action sendCoSig(q: Party, d: Party): bool = all {
    sendCoSig_enabled(q, d),
    coSigs' = coSigs.set(q, coSigs.get(q).union(Set(d))),
    // UNCHANGED
    phases' = phases,
    assets' = assets,
    keepAdaptor, keepKeys, keepNonces, keepTAgg, keepPreSigs,
    keepClaimFlags,
    keepRefundState,
    keepWithholds,
  }

  action signInTxAdvance(p: Party): bool = all {
    signInTxAdvance_enabled(p),
    phases' = phases.set(p, PhaseLock),
    // UNCHANGED
    assets' = assets,
    keepSetupBroadcasts,
    keepClaimFlags,
    keepRefundState,
    keepWithholds,
  }

  action lock(p: Party): bool = all {
    lock_enabled(p),
    assets' = assets.set(p, Locked),
    // UNCHANGED
    phases' = phases,
    keepSetupBroadcasts,
    keepClaimFlags,
    keepRefundState,
    keepWithholds,
  }

  action advanceToClaim(p: Party): bool = all {
    advanceToClaim_enabled(p),
    phases' = phases.set(p, PhaseClaim),
    // UNCHANGED
    assets' = assets,
    keepSetupBroadcasts,
    keepClaimFlags,
    keepRefundState,
    keepWithholds,
  }

  action sendSecretTo(p: Party, recipients: Set[Party], divergent: Set[Party]): bool = all {
    sendSecretTo_enabled(p),
    HONEST.contains(p) implies recipients == PARTIES,
    HONEST.contains(p) implies divergent == Set(),
    divergent.subseteq(recipients.intersect(HONEST)),
    secretReceivedAt' = PARTIES.mapBy(r =>
      if (recipients.contains(r))
        secretReceivedAt.get(r).union(Set(p))
      else
        secretReceivedAt.get(r)),
    secretEquivocated' = PARTIES.mapBy(r =>
      if (divergent.contains(r))
        secretEquivocated.get(r).union(Set(p))
      else
        secretEquivocated.get(r)),
    secretDecided' = secretDecided.union(Set(p)),
    // UNCHANGED
    phases' = phases,
    assets' = assets,
    keepSetupBroadcasts,
    keepTrigger, keepFinalized, keepConfirmed,
    keepRefundState,
    keepWithholds,
  }

  action trigger: bool = all {
    trigger_enabled,
    assets' = assets.set(senderOf(LEADER), PendingClaim(LEADER)),
    triggerBroadcast' = true,
    // UNCHANGED
    phases' = phases,
    keepSetupBroadcasts,
    keepSecrets, keepFinalized, keepConfirmed,
    keepRefundState,
    keepWithholds,
  }

  action claim(p: Party): bool = all {
    claim_enabled(p),
    assets' = assets.set(senderOf(p), PendingClaim(p)),
    // UNCHANGED
    phases' = phases,
    keepSetupBroadcasts,
    keepClaimFlags,
    keepRefundState,
    keepWithholds,
  }

  // confirmClaim records the claim of p's incoming.  Independent of whether
  // p has refunded its own deposit — the two are separate on-chain actions
  // on different UTXOs.
  action confirmClaim(p: Party): bool = all {
    confirmClaim_enabled(p),
    assets' = assets.set(senderOf(p), Claimed(p)),
    claimConfirmed' = claimConfirmed.union(Set(p)),
    // UNCHANGED
    phases' = phases,
    keepSetupBroadcasts,
    keepSecrets, keepTrigger, keepFinalized,
    keepRefundState,
    keepWithholds,
  }

  // reorgClaim is bounded by MAX_REORGS per claimant.  The bound captures
  // the §2.4 finality assumption: once Δ blocks elapse a claim transaction
  // is final.
  action reorgClaim(p: Party): bool = all {
    reorgClaim_enabled(p),
    assets' = assets.set(senderOf(p), Locked),
    reorgCount' = reorgCount.set(p, reorgCount.get(p) + 1),
    // UNCHANGED
    phases' = phases,
    keepSetupBroadcasts,
    keepClaimFlags,
    keepOpen, keepRefunded,
    keepWithholds,
  }

  action finalizeClaims: bool = all {
    finalizeClaims_enabled,
    claimsFinalized' = true,
    // UNCHANGED
    phases' = phases,
    assets' = assets,
    keepSetupBroadcasts,
    keepSecrets, keepTrigger, keepConfirmed,
    keepRefundState,
    keepWithholds,
  }

  // Refund.  Moves p's deposit from Locked to Refunded once the guard
  // in `refund_enabled` is satisfied (p's window is open, deposit still
  // Locked, not already refunded).  The chain-level Δ-gap between an
  // honest claim and the depositor's refund is captured implicitly by
  // the `noOtherActionEnabled` gate on `openNextWindow`: while an
  // honest recipient can still claim, time does not advance, and once
  // the claim fires `assets.get(p)` is no longer `Locked` so refund is
  // disabled.
  action refund(p: Party): bool = all {
    refund_enabled(p),
    assets' = assets.set(p, Refunded(p)),
    refunded' = refunded.union(Set(p)),
    // UNCHANGED
    phases' = phases,
    keepSetupBroadcasts,
    keepClaimFlags,
    keepOpen, keepReorg,
    keepWithholds,
  }

  // ─── Withhold actions (dishonest parties only) ────────────────────────────
  //
  // Off-chain `withholdSecret` is subsumed by `sendSecretTo(p, Set())`.
  // The remaining withholds are on-chain decisions with no recipients
  // dimension.

  action withholdLock(p: Party): bool = all {
    withholdLock_enabled(p),
    withheldLocks' = withheldLocks.union(Set(p)),
    // UNCHANGED
    phases' = phases,
    assets' = assets,
    keepSetupBroadcasts,
    keepClaimFlags,
    keepRefundState,
    keepWClaims, keepWTrigger,
  }

  action withholdClaim(p: Party): bool = all {
    withholdClaim_enabled(p),
    withheldClaims' = withheldClaims.union(Set(p)),
    // UNCHANGED
    phases' = phases,
    assets' = assets,
    keepSetupBroadcasts,
    keepClaimFlags,
    keepRefundState,
    keepWLocks, keepWTrigger,
  }

  action withholdTrigger: bool = all {
    withholdTrigger_enabled,
    withheldTrigger' = true,
    // UNCHANGED
    phases' = phases,
    assets' = assets,
    keepSetupBroadcasts,
    keepClaimFlags,
    keepRefundState,
    keepWLocks, keepWClaims,
  }

  // ─── Time: openNextWindow ─────────────────────────────────────────────────
  //
  // Picks the unopened party with the smallest `W` and is enabled iff no
  // other action is enabled — i.e. the protocol has run out of useful
  // steps and time must advance.
  //
  // The smallest-W cascade is encoded by the inner `forall`: any non-minimal
  // unopened d fails it (because some smaller-W q would still be unopened).
  // If `openWindows == PARTIES`, no `d` satisfies `not(openWindows.contains(d))`
  // and the action is disabled, which is correct — there is nothing left to
  // open.
  action openNextWindow: bool =
    nondet d = oneOf(PARTIES)
    all {
      noOtherActionEnabled,
      not(openWindows.contains(d)),
      PARTIES.forall(q =>
        WINDOWS.get(q) < WINDOWS.get(d) implies openWindows.contains(q)),
      openWindows' = openWindows.union(Set(d)),
      // UNCHANGED
      phases' = phases,
      assets' = assets,
      keepSetupBroadcasts,
      keepClaimFlags,
      keepRefunded, keepReorg,
      keepWithholds,
    }

  // ─── Per-party step (used as the unit of fairness) ───────────────────────
  //
  // Fires one of party p's locally-driven transitions.  Includes the
  // remaining on-chain withhold actions so that fairness on stepBy(p) for
  // every p forces dishonest parties to make their decision eventually.
  // Off-chain withholding is encoded by `sendXTo(p, recipients)` with a
  // partial (possibly empty) recipients set — one-shot, so fairness is
  // satisfied without dragging the dishonest sender into delivering to
  // every party.

  action stepBy(p: Party): bool = any {
    config(p),
    nondet rs = oneOf(PARTIES.powerset())
      (nondet ds = oneOf(rs.powerset()) sendAdaptorPointTo(p, rs, ds)),
    adaptorBroadcastAdvance(p),
    nondet rs = oneOf(PARTIES.powerset())
      (nondet ds = oneOf(rs.powerset()) sendKeyTo(p, rs, ds)),
    key1Advance(p),
    nondet rs = oneOf(PARTIES.powerset())
      (nondet ds = oneOf(rs.powerset()) sendNonceTo(p, rs, ds)),
    key2Advance(p),
    nondet d = oneOf(PARTIES) all {
      recipientOf(d) == p,
      nondet rs = oneOf(PARTIES.powerset())
        (nondet ds = oneOf(rs.powerset()) sendTAggTo(d, rs, ds)),
    },
    nondet d = oneOf(PARTIES) sendPreSig(p, d),
    key3Advance(p),
    nondet d = oneOf(PARTIES) sendCoSig(p, d),
    signInTxAdvance(p),
    lock(p),
    advanceToClaim(p),
    nondet rs = oneOf(PARTIES.powerset())
      (nondet ds = oneOf(rs.powerset()) sendSecretTo(p, rs, ds)),
    all { p == LEADER, trigger },
    claim(p),
    confirmClaim(p),
    reorgClaim(p),
    refund(p),
    finalizeClaims,
    // Dishonest decisions (guards self-restrict to not(HONEST.contains(p))):
    withholdLock(p),
    withholdClaim(p),
    all { p == LEADER, withholdTrigger },
  }

  // ─── Temporal liveness ────────────────────────────────────────────────────

  // Fairness assumptions:
  //   • weakFair on stepBy(p) for every party — honest and dishonest.
  //     Honest parties make protocol-conforming progress under fairness.
  //     Dishonest parties' fairness only forces them to eventually decide;
  //     the content of the decision (which recipients, or which withhold)
  //     is free.  The atomic-commit shape of `sendXTo` means a single fire
  //     satisfies weak fairness without forcing eventual delivery to
  //     every recipient.
  //   • weakFair on openNextWindow — time eventually advances when the
  //     protocol is stuck.
  //   • strongFair on finalizeClaims — models the §2.4 finality assumption
  //     that Δ blocks elapse.  Reorgs can break finalizeClaims's enabling
  //     intermittently, so weakFair would be too weak to defeat reorg loops.
  val allVars = (phases, assets,
                 adaptorReceivedAt, adaptorDecided, adaptorEquivocated,
                 keyReceivedAt, keyDecided, keyEquivocated,
                 nonceReceivedAt, nonceDecided, nonceEquivocated,
                 tAggReceivedAt, tAggDecided, tAggEquivocated,
                 preSigs, coSigs,
                 secretReceivedAt, secretDecided, secretEquivocated,
                 triggerBroadcast,
                 openWindows, claimsFinalized, refunded, claimConfirmed,
                 reorgCount, withheldLocks, withheldClaims, withheldTrigger)

  temporal honestFairness: bool = and {
    PARTIES.forall(p => weakFair(stepBy(p), allVars)),
    weakFair(openNextWindow, allVars),
    strongFair(finalizeClaims, allVars),
  }

  // (L*) Settled liveness — every honest party's deposit eventually leaves
  // the Locked state for good.  The "eventually-always" shape rules out
  // transient PendingClaim satisfying the property only to be reverted by a
  // later reorg; the MAX_REORGS cap makes the PendingClaim stable after a
  // bounded number of reorgs.
  temporal honestSettled: bool =
    honestFairness implies
      HONEST.forall(p => eventually(always(
        assets.get(p) != Locked
      )))

  // (L**) Atomic outcome — every honest party eventually stabilises in one
  // of three atomic-correct shapes:
  //   (a) the swap completed for that party — their deposit is claimed by
  //       the legitimate next party AND their incoming is claimed by them;
  //   (b) the party recovered via refund;
  //   (c) the party never put principal at risk (OwnedBy).
  temporal atomicOutcome: bool =
    honestFairness implies
      HONEST.forall(p => eventually(always(
        (claimedByNext(p, assets) and iClaimedIncoming(p, assets))
        or assets.get(p) == Refunded(p)
        or assets.get(p) == OwnedBy(p)
      )))

  // ─── Step ─────────────────────────────────────────────────────────────────

  action step: bool = any {
    nondet p = oneOf(PARTIES) config(p),
    nondet p = oneOf(PARTIES)
      (nondet rs = oneOf(PARTIES.powerset())
        (nondet ds = oneOf(rs.powerset()) sendAdaptorPointTo(p, rs, ds))),
    nondet p = oneOf(PARTIES) adaptorBroadcastAdvance(p),
    nondet p = oneOf(PARTIES)
      (nondet rs = oneOf(PARTIES.powerset())
        (nondet ds = oneOf(rs.powerset()) sendKeyTo(p, rs, ds))),
    nondet p = oneOf(PARTIES) key1Advance(p),
    nondet p = oneOf(PARTIES)
      (nondet rs = oneOf(PARTIES.powerset())
        (nondet ds = oneOf(rs.powerset()) sendNonceTo(p, rs, ds))),
    nondet p = oneOf(PARTIES) key2Advance(p),
    nondet d = oneOf(PARTIES)
      (nondet rs = oneOf(PARTIES.powerset())
        (nondet ds = oneOf(rs.powerset()) sendTAggTo(d, rs, ds))),
    nondet p = oneOf(PARTIES) (nondet d = oneOf(PARTIES) sendPreSig(p, d)),
    nondet p = oneOf(PARTIES) key3Advance(p),
    nondet q = oneOf(PARTIES) (nondet d = oneOf(PARTIES) sendCoSig(q, d)),
    nondet p = oneOf(PARTIES) signInTxAdvance(p),
    nondet p = oneOf(PARTIES) lock(p),
    nondet p = oneOf(PARTIES) advanceToClaim(p),
    nondet p = oneOf(PARTIES)
      (nondet rs = oneOf(PARTIES.powerset())
        (nondet ds = oneOf(rs.powerset()) sendSecretTo(p, rs, ds))),
    trigger,
    nondet p = oneOf(PARTIES) claim(p),
    nondet p = oneOf(PARTIES) confirmClaim(p),
    nondet p = oneOf(PARTIES) reorgClaim(p),
    finalizeClaims,
    nondet p = oneOf(PARTIES) refund(p),
    nondet p = oneOf(PARTIES) withholdLock(p),
    nondet p = oneOf(PARTIES) withholdClaim(p),
    withholdTrigger,
    openNextWindow,
  }

  // ─── Type-domain anchor ───────────────────────────────────────────────────

  val allAssetStatuses: Set[AssetStatus] =
    PARTIES.map(p => OwnedBy(p))
      .union(Set(Locked))
      .union(PARTIES.map(p => PendingClaim(p)))
      .union(PARTIES.map(p => Claimed(p)))
      .union(PARTIES.map(p => Refunded(p)))

  val allPhases: Set[Phase] = Set(
    PhaseConfig,
    PhaseSetupAdaptorBroadcast,
    PhaseSetupKey1,
    PhaseSetupKey2,
    PhaseSetupKey3,
    PhaseSetupSignInTx,
    PhaseLock,
    PhaseClaim,
  )

  val typeInv: bool = and {
    phases.in(PARTIES.setOfMaps(allPhases)),
    assets.in(PARTIES.setOfMaps(allAssetStatuses)),
    adaptorReceivedAt.in(PARTIES.setOfMaps(PARTIES.powerset())),
    adaptorDecided.in(PARTIES.powerset()),
    adaptorEquivocated.in(PARTIES.setOfMaps(PARTIES.powerset())),
    keyReceivedAt.in(PARTIES.setOfMaps(PARTIES.powerset())),
    keyDecided.in(PARTIES.powerset()),
    keyEquivocated.in(PARTIES.setOfMaps(PARTIES.powerset())),
    nonceReceivedAt.in(PARTIES.setOfMaps(PARTIES.powerset())),
    nonceDecided.in(PARTIES.powerset()),
    nonceEquivocated.in(PARTIES.setOfMaps(PARTIES.powerset())),
    tAggReceivedAt.in(PARTIES.setOfMaps(PARTIES.powerset())),
    tAggDecided.in(PARTIES.powerset()),
    tAggEquivocated.in(PARTIES.setOfMaps(PARTIES.powerset())),
    preSigs.in(PARTIES.setOfMaps(PARTIES.powerset())),
    coSigs.in(PARTIES.setOfMaps(PARTIES.powerset())),
    secretReceivedAt.in(PARTIES.setOfMaps(PARTIES.powerset())),
    secretDecided.in(PARTIES.powerset()),
    secretEquivocated.in(PARTIES.setOfMaps(PARTIES.powerset())),
    triggerBroadcast.in(Set(true, false)),
    openWindows.in(PARTIES.powerset()),
    claimsFinalized.in(Set(true, false)),
    refunded.in(PARTIES.powerset()),
    claimConfirmed.in(PARTIES.powerset()),
    reorgCount.in(PARTIES.setOfMaps(0.to(MAX_REORGS))),
    withheldLocks.in(PARTIES.powerset()),
    withheldClaims.in(PARTIES.powerset()),
    withheldTrigger.in(Set(true, false)),
  }

  // ─── Invariants ───────────────────────────────────────────────────────────

  val invAtomicity: bool = not(anyClaimed and anyRefunded)

  // Setup monotonicity: a party's phase reflects the round-completion facts
  // it must have observed before advancing.  Per-recipient form: p's phase
  // is bounded by what p has personally received.
  val invSetupMonotone: bool =
    PARTIES.forall(p =>
      val ph = phases.get(p)
      and {
        phaseAtLeast(ph, PhaseSetupKey1)     implies allAdaptorPointsPresentFor(p, adaptorReceivedAt),
        phaseAtLeast(ph, PhaseSetupKey2)     implies allKeysPresentFor(p, keyReceivedAt),
        phaseAtLeast(ph, PhaseSetupKey3)     implies allNoncesPresentFor(p, nonceReceivedAt),
        phaseAtLeast(ph, PhaseSetupSignInTx) implies roundDoneFor(p, preSigs),
        phaseAtLeast(ph, PhaseLock)          implies roundDoneFor(p, coSigs),
      }
    )

  val invHonestNoLoss: bool =
    HONEST.forall(p => {
      val depositStatus = assets.get(p)
      val claimStatus   = assets.get(senderOf(p))
      or {
        depositStatus == Locked,
        depositStatus == OwnedBy(p),
        depositStatus == Refunded(p),
        depositStatus == PendingClaim(recipientOf(p)),
        claimStatus   == Claimed(p),
        claimStatus   == PendingClaim(p),
        claimStatus   == Locked and not(openWindows.contains(senderOf(p))),
      }
    })

  // (M) A sender's phase must have reached the phase at which it fires.
  // The existing `invSetupMonotone` is "row-side" (recipient phase implies
  // inbox is full); this is the symmetric "column-side" — every act of
  // sending implies the sender reached the relevant send phase, which is
  // monotone so the sender is still at or past it.  Without (M), Apalache
  // can construct spurious predecessors where e.g. a signer's coSigs row
  // is populated but the signer hasn't reached PhaseSetupSignInTx — letting
  // a tAgg send fire late and grow `tAggEquivocated` against an honest
  // party who has already advanced past key3.
  val invSendImpliesPhase: bool = and {
    PARTIES.forall(q => and {
      keyDecided.contains(q)    implies phaseAtLeast(phases.get(q), PhaseSetupKey1),
      nonceDecided.contains(q)  implies phaseAtLeast(phases.get(q), PhaseSetupKey2),
      preSigs.get(q) != Set()   implies phaseAtLeast(phases.get(q), PhaseSetupKey3),
      coSigs.get(q) != Set()    implies phaseAtLeast(phases.get(q), PhaseSetupSignInTx),
      secretDecided.contains(q) implies phaseAtLeast(phases.get(q), PhaseClaim),
    }),
    PARTIES.forall(d =>
      tAggDecided.contains(d) implies phaseAtLeast(phases.get(recipientOf(d)), PhaseSetupKey3)),
  }

  // (P) A pre-signature for d implies the signer received T_agg for d.
  // Structurally true: `sendPreSig_enabled` requires
  // `tAggReceivedAt.get(p).contains(d)`.  Lifts the row-side
  // `roundDoneFor(p, preSigs)` from invSetupMonotone into a corresponding
  // fact about `tAggReceivedAt`, which is what (E) needs to conclude all
  // tAgg senders have decided.
  val invPreSigImpliesTAgg: bool =
    PARTIES.forall(p => preSigs.get(p).subseteq(tAggReceivedAt.get(p)))

  // (D) Inbox entries imply decided senders.  Structurally true: each
  // `sendXTo` adds the sender to `*Decided` concurrently with adding it to
  // `*ReceivedAt[r]`.  Needed to chain `*ReceivedAt[p] == PARTIES` (from
  // invSetupMonotone) up to `*Decided == PARTIES`, which then disables
  // further sends — required for (E) to be inductive.
  val invInboxDecided: bool =
    PARTIES.forall(r => and {
      adaptorReceivedAt.get(r).subseteq(adaptorDecided),
      keyReceivedAt.get(r).subseteq(keyDecided),
      nonceReceivedAt.get(r).subseteq(nonceDecided),
      tAggReceivedAt.get(r).subseteq(tAggDecided),
      secretReceivedAt.get(r).subseteq(secretDecided),
    })

  // (D') Equivocation markers only come from senders who actually delivered.
  // Structurally true: `divergent ⊆ recipients` in every send action.
  val invEquivocatedInInbox: bool =
    PARTIES.forall(r => and {
      adaptorEquivocated.get(r).subseteq(adaptorReceivedAt.get(r)),
      keyEquivocated.get(r).subseteq(keyReceivedAt.get(r)),
      nonceEquivocated.get(r).subseteq(nonceReceivedAt.get(r)),
      tAggEquivocated.get(r).subseteq(tAggReceivedAt.get(r)),
      secretEquivocated.get(r).subseteq(secretReceivedAt.get(r)),
    })

  // (E) Honest parties never advance past a phase whose equivocation
  // inbox was non-empty.  Since the relevant senders are one-shot per kind
  // and advance requires all of them to have fired (`allXPresentFor`),
  // `*Equivocated[p]` is frozen at what it was at advance time — namely
  // Set() under the honest-aborts-on-inconsistency guards.
  val invNoEquivocationAtAdvance: bool =
    PARTIES.forall(p =>
      HONEST.contains(p) implies and {
        phaseAtLeast(phases.get(p), PhaseSetupKey1)     implies adaptorEquivocated.get(p) == Set(),
        phaseAtLeast(phases.get(p), PhaseSetupKey2)     implies keyEquivocated.get(p) == Set(),
        phaseAtLeast(phases.get(p), PhaseSetupKey3)     implies nonceEquivocated.get(p) == Set(),
        phaseAtLeast(phases.get(p), PhaseSetupSignInTx) implies tAggEquivocated.get(p) == Set(),
      })

  // (E') Honest leader's trigger requires no secret-level equivocation
  // against it; once triggered, no more secret-sends can fire so
  // secretEquivocated[LEADER] is frozen.
  val invNoSecretEquivAtTrigger: bool =
    triggerBroadcast and HONEST.contains(LEADER) implies
      secretEquivocated.get(LEADER) == Set()

  // Candidate inductive invariant for invHonestNoLoss.  Mirrors
  // invAtomicityInductive's structural facts and adds invHonestNoLoss as
  // the goal.
  val invHonestNoLossInductive: bool = and {
    typeInv,
    invHonestNoLoss,
    invSetupMonotone,
    invSendImpliesPhase,
    invPreSigImpliesTAgg,
    invInboxDecided,
    invEquivocatedInInbox,
    invNoEquivocationAtAdvance,
    invNoSecretEquivAtTrigger,

    // (C1) Asset → phase/flag coherence.
    PARTIES.forall(p =>
      match assets.get(p) {
        | OwnedBy(q)      => q == p
        | Locked          => phaseAtLeast(phases.get(p), PhaseLock)
        | PendingClaim(q) => q == recipientOf(p) and triggerBroadcast
                             and phaseAtLeast(phases.get(p), PhaseLock)
                             and phaseAtLeast(phases.get(q), PhaseClaim)
        | Claimed(q)      => q == recipientOf(p) and triggerBroadcast
                             and claimsFinalized
                             and claimConfirmed.contains(q)
                             and phaseAtLeast(phases.get(p), PhaseLock)
        | Refunded(q)     => q == p
                             and refunded.contains(p)
                             and openWindows.contains(p)
      }),

    // (C2) Terminal-flag → asset coherence.
    PARTIES.forall(p =>
      refunded.contains(p) implies assets.get(p) == Refunded(p)),
    PARTIES.forall(p =>
      claimConfirmed.contains(p) implies (match assets.get(senderOf(p)) {
        | Claimed(q) => q == p
        | _          => false
      })),

    // (T1) advanceToClaim requires allLocked.
    (PARTIES.exists(p =>
        phases.get(p) == PhaseClaim or claimConfirmed.contains(p)))
      implies PARTIES.forall(q => assets.get(q) != OwnedBy(q)),

    // (T1') triggerBroadcast implies leader was at PhaseClaim, hence allLocked.
    triggerBroadcast implies PARTIES.forall(q => assets.get(q) != OwnedBy(q)),

    // (X+) Finalization implies all honest claims are past the race window.
    claimsFinalized implies honestClaimsDone,

    // (S) Trigger fires only after leader has every non-leader's secret.
    // Previously implicit in trigger_enabled + monotonicity of secretsRevealed;
    // with the per-recipient split this needs stating.
    triggerBroadcast implies
      PARTIES.exclude(Set(LEADER)).forall(p =>
        secretReceivedAt.get(LEADER).contains(p)),
  }

  val invAtomicityInductive: bool = and {
    typeInv,
    not(anyClaimed and anyRefunded),
    invSetupMonotone,
    invSendImpliesPhase,
    invPreSigImpliesTAgg,
    invInboxDecided,
    invEquivocatedInInbox,
    invNoEquivocationAtAdvance,
    invNoSecretEquivAtTrigger,

    // (C1) Asset → phase/flag coherence.
    PARTIES.forall(p =>
      match assets.get(p) {
        | OwnedBy(q)      => q == p
        | Locked          => phaseAtLeast(phases.get(p), PhaseLock)
        | PendingClaim(q) => q == recipientOf(p) and triggerBroadcast
                             and phaseAtLeast(phases.get(p), PhaseLock)
                             and phaseAtLeast(phases.get(q), PhaseClaim)
        | Claimed(q)      => q == recipientOf(p) and triggerBroadcast
                             and claimsFinalized
                             and claimConfirmed.contains(q)
                             and phaseAtLeast(phases.get(p), PhaseLock)
        | Refunded(q)     => q == p
                             and refunded.contains(p)
                             and openWindows.contains(p)
      }),

    // (C2) Terminal-flag → asset coherence.
    PARTIES.forall(p =>
      refunded.contains(p) implies assets.get(p) == Refunded(p)),
    PARTIES.forall(p =>
      claimConfirmed.contains(p) implies (match assets.get(senderOf(p)) {
        | Claimed(q) => q == p
        | _          => false
      })),

    // (T1) advanceToClaim requires allLocked.
    (PARTIES.exists(p =>
        phases.get(p) == PhaseClaim or claimConfirmed.contains(p)))
      implies PARTIES.forall(q => assets.get(q) != OwnedBy(q)),

    // (T1') triggerBroadcast implies leader was at PhaseClaim, hence allLocked.
    triggerBroadcast implies PARTIES.forall(q => assets.get(q) != OwnedBy(q)),

    // (X)  Refunds and finalization mutually exclusive.
    anyRefunded     implies not(claimsFinalized),
    claimsFinalized implies not(anyRefunded),

    // (X+) Finalization implies all honest claims are past the race window.
    claimsFinalized implies honestClaimsDone,

    // (S) Trigger fires only after leader has every non-leader's secret.
    triggerBroadcast implies
      PARTIES.exclude(Set(LEADER)).forall(p =>
        secretReceivedAt.get(LEADER).contains(p)),
  }

}
