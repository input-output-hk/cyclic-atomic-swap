// -*- mode: Bluespec; -*-
//
// Type definitions for the cyclic atomic swap protocol.
// See doc/protocol-spec.pdf for the full protocol description.
//
// Trimmed to just the types the abstract model (`protocolFlat.qnt`) actually
// consumes.  The full type set (concrete messages, network, ProtocolSystem
// snapshot record, per-party crypto state) lives in old-spec/types.qnt.

module types {

  import basicSpells.* from "spells/basicSpells"

  // ─── Identity ────────────────────────────────────────────────────────────

  // Parties are identified by name.
  type Party = str

  // ─── Protocol phases ─────────────────────────────────────────────────────
  //
  // Phases are ordered:
  //   Config → Setup(AdaptorBroadcast → Key1 → Key2 → Key3 → SignInTx)
  //          → Lock → Claim
  //
  // The terminal facts (claim confirmed, deposit refunded) are NOT phases.
  // A party may publish both a refund of its own deposit and a confirmClaim
  // of its incoming as independent on-chain transactions, so they are
  // tracked as separate Set[Party] state vars in the model rather than
  // forced into a mutually-exclusive PhaseDone / PhaseRefund split.
  //
  // The Setup* sub-phases correspond to the Setup phase in Figure 3 of the
  // protocol spec (§5).  They are split here for granularity but collectively
  // represent a single phase from the spec's point of view.

  type Phase =
      PhaseConfig
    | PhaseSetupAdaptorBroadcast
    | PhaseSetupKey1
    | PhaseSetupKey2
    | PhaseSetupKey3
    | PhaseSetupSignInTx
    | PhaseLock
    | PhaseClaim

  // ─── On-chain asset state ────────────────────────────────────────────────

  type AssetStatus =
      OwnedBy(Party)        // not yet deposited, or returned after refund
    | Locked                // deposit tx confirmed; pending withdraw or refund
    | PendingClaim(Party)   // claim tx published (secret revealed) but not yet confirmed
    | Claimed(Party)        // withdraw tx confirmed (final)
    | Refunded(Party)       // refund tx confirmed

}
