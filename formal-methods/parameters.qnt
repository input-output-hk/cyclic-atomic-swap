// -*- mode: Bluespec; -*-
//
// Static configuration of the cyclic atomic swap: the participating parties,
// the leader, the trust profile (HONEST), the transfer cycle, and the
// staggered refund-window heights.  Plus the basic cycle helpers that depend
// only on these constants.

module parameters {

  import types.* from "types"

  // ─── Protocol parameters ────────────────────────────────────────────────────

  // The set of all participating parties.
  val PARTIES: Set[Party] = Set("A", "B", "C")

  // The leader initiates the Claim phase and must not share its adaptor secret
  // off-chain (see §7.1 of the protocol spec).
  val LEADER: Party = "A"

  // The set of parties assumed to follow the spec's prescribed behaviour —
  // in particular, the liveness obligation to claim promptly during their
  // race window.  Parties NOT in HONEST may deviate arbitrarily (delay,
  // withhold messages, refund opportunistically, collude).
  //
  // Default: every party honest.  Override for adversarial verification:
  //   HONEST = Set("B")          // coalition against B (A and C dishonest)
  //   HONEST = PARTIES.exclude(Set(LEADER))  // honest-leader-attack
  //   HONEST = Set("A","B")      // partial-claim: C is offline
  //
  // Invariants over HONEST (see invHonestNoLoss) capture the spec's actual
  // safety guarantee: "honest parties don't lose".  The step relation gives
  // honest parties priority to claim during the race window, so that the
  // model checker doesn't construct spurious losses by simply not scheduling
  // an honest party's claim.
  val HONEST: Set[Party] = PARTIES

  // TRANSFERS[p] = the party that p sends its asset to.
  // The cycle is: A → B → C → A.
  val TRANSFERS: Party -> Party = Map(
    "A" -> "B",
    "B" -> "C",
    "C" -> "A",
  )

  // INVTRANSFERS[p] = the party from whom p receives (i.e., the depositor for
  // the transfer that p claims).
  val INVTRANSFERS: Party -> Party = Map(
    "A" -> "C",
    "B" -> "A",
    "C" -> "B",
  )

  // Refund window heights W_p: the minimum block height at which the refund
  // transaction for the transfer deposited by party p becomes valid.
  // Must satisfy: WINDOWS[A] > WINDOWS[B] > WINDOWS[C]
  // and each gap must be ≥ DELTA  (see §2.4 of the protocol spec).
  val WINDOWS: Party -> int = Map(
    "A" -> 300,
    "B" -> 200,
    "C" -> 100,
  )

  // Minimum claim propagation time Δ: the maximum number of blocks a party
  // needs to observe a trigger, build a withdraw tx, and get it confirmed.
  val DELTA: int = 50

  // Maximum number of reorgs allowed per party's published claim.  The spec
  // models finality depth (§2.4): a transaction at finality depth is final;
  // before finality depth, a confirmed transaction may be reverted by a
  // chain reorganisation.  With Δ chosen conservatively, multi-block reorgs
  // are extraordinarily rare; bounding by a small constant captures this
  // without making the model unfaithful.
  //
  //   MAX_REORGS = 0  — BFT-final chains; no reorgs ever.
  //   MAX_REORGS = 1  — default; one reorg per claim, faithful to §2.4.
  //   MAX_REORGS = k  — paranoid model; allow k reorgs per claim.
  val MAX_REORGS: int = 1

  // Finite set of secrets for nondeterministic initialisation.
  val SECRETS: Set[int] = 1.to(2)

  // ─── Basic helper functions ──────────────────────────────────────────────────

  // The party from whom p receives (= the depositor for p's claim transfer).
  pure def senderOf(p: Party): Party = INVTRANSFERS.get(p)

  // The party to whom p sends (= the claimant for p's deposit transfer).
  pure def recipientOf(p: Party): Party = TRANSFERS.get(p)

  // True when all deposit transactions have been confirmed on-chain.
  pure def allLocked(assets: Party -> AssetStatus): bool =
    PARTIES.forall(p => assets.get(p) == Locked)

  // Linear ordering on phases.  After the terminal-flag split, "done" and
  // "refunded" are tracked as separate Set[Party] facts in the model rather
  // than as phases, since a party may publish both a refund of its own
  // deposit and a confirmClaim of its incoming (independent on-chain
  // transactions).  Phases now go up to PhaseClaim only.
  //   Config → Setup(AdaptorBroadcast → Key1 → Key2 → Key3 → SignInTx)
  //          → Lock → Claim
  pure def phaseRank(ph: Phase): int = match ph {
    | PhaseConfig                => 0
    | PhaseSetupAdaptorBroadcast => 1
    | PhaseSetupKey1             => 2
    | PhaseSetupKey2             => 3
    | PhaseSetupKey3             => 4
    | PhaseSetupSignInTx         => 5
    | PhaseLock                  => 6
    | PhaseClaim                 => 7
  }

  pure def phaseAtLeast(ph: Phase, threshold: Phase): bool =
    phaseRank(ph) >= phaseRank(threshold)

  // Staggered-window background assumption.
  // For each transfer from depositor d to recipient r = recipientOf(d):
  //   • r ≠ LEADER: W_d > W_r  and  W_d − W_r ≥ Δ  (ordering + gap)
  //   • r = LEADER: W_d < W_LEADER                   (last link; no gap needed)
  assume windowsOrdered =
    PARTIES.forall(d =>
      val recip = recipientOf(d)
      (recip == LEADER and WINDOWS.get(d) < WINDOWS.get(LEADER))
      or (recip != LEADER
            and WINDOWS.get(d) > WINDOWS.get(recip)
            and WINDOWS.get(d) - WINDOWS.get(recip) >= DELTA)
    )

}
