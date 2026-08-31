# Notes on the cyclic-atomic-swap Quint formalization

The active verification model is **`protocolFlat.qnt`**: the full
protocol (Config → Setup → Lock → Claim → Done/Refund), with safety
properties discharged inductively (unbounded) by Apalache and liveness
properties discharged via bounded explicit-state search by TLC.

This document records, for each material way the formalization
departs from the prose spec
([`docs/protocol-spec.tex`](../docs/protocol-spec.tex)), what the
departure is and why it does (or does not) bound what the proofs
establish about the protocol.

## Two backends, two roles

Quint's `quint verify` command dispatches to one of two model checkers
based on `--backend` (default `apalache`):

- **Apalache** is a *symbolic* model checker built on SMT.  It is the
  right tool for **safety properties** — invariants over reachable states.
  Crucially, with `--inductive-invariant=Ind`, Apalache discharges
  `Init ⇒ Ind`, `Ind ∧ step ⇒ Ind'`, and `Ind ⇒ Inv` as three length-1
  SMT problems, giving an **unbounded** safety proof in seconds.
  Apalache *does not* support fairness, and its temporal-property mode
  is experimental.
- **TLC** is an *explicit-state* model checker.  It is the right tool
  for **liveness / temporal properties** — formulas with `eventually`
  and `always` that depend on fairness.  TLC enumerates the reachable
  state space (so the model must be effectively finite) and checks
  Büchi-style conditions including weak and strong fairness.  TLC does
  *not* do inductive-invariant analysis — for safety it does bounded
  explicit search up to whatever fits in memory.

Apalache's inductive-invariant mode imposes structural requirements
on the model (state split across top-level variables, transitions as
named top-level actions, every variable type-anchored).  These are
documented in [Model structure for Apalache](#model-structure-for-apalache)
below; they are mechanical encodings that do not depart from the
protocol semantics and are not listed under "Differences from the
prose spec".

---

## Differences from the prose spec

The departures below are grouped by what they affect: (A) bounds on
the proof's quantification, (B) sound abstractions over the protocol's
data, and (C) abstractions over time and adversary modelling.

### A1.  Fixed 3-party instantiation

The prose spec is parameterised by `N` parties
$P_0,\ldots,P_{N-1}$ arranged in a directed cycle (§1.1).  The
formalization fixes:

```quint
val PARTIES: Set[Party] = Set("A", "B", "C")     // N = 3
val LEADER:  Party      = "A"
val TRANSFERS:    Party -> Party = Map("A" -> "B", "B" -> "C", "C" -> "A")
val INVTRANSFERS: Party -> Party = Map("A" -> "C", "B" -> "A", "C" -> "B")
```

**Significance — bounds the proof's quantification.**  Every safety
and liveness theorem discharged below holds for `N = 3` only; the
proof says nothing directly about `N = 4` or `N = 100`.

The prose spec's §11 worked example ("Example Trace: $N = 3$, Leader
= $A$") is the same instantiation, so the formalization corresponds
exactly to the spec's worked case.  Spec-level arguments about how
the staggered-window invariant and the trigger cascade generalise to
arbitrary `N` are not mechanised.

Apalache's inductive-invariant mode is in principle parametric, but
the SMT problem grows steeply with `N` (the state shape contains
`Party -> Set[Party]` per off-chain message kind, plus per-party
sub-phases).  Going to `N = 4` is mechanical; going to `N = N` would
require either inductive reasoning over the cycle structure (which
Apalache does not provide directly) or a separate paper proof.

### A2.  Bounded reorg model (`MAX_REORGS = 1`)

The prose spec §2.4 commits to a *finality depth* Δ: a transaction
at finality depth is final; before then it can be reverted by a
chain reorganisation.  The formalization represents reorgs with a
`reorgClaim(p)` action and bounds them per claim:

```quint
val MAX_REORGS: int = 1
```

`MAX_REORGS = 0` corresponds to BFT-final chains, `MAX_REORGS = 1`
to the default, and `MAX_REORGS > 1` to paranoid chain models.

**Significance — faithful to the adversarially interesting case.**
A claim publication reveals $t_{agg}$ on-chain.  A reorg of the
claim does not un-reveal the secret: by the time the reorg happens,
every observer (including the attacker) has $t_{agg}$ and can use it
to complete the withdraw for any locked account before that
account's refund window opens.  The spec's choice of Δ is precisely
the conservative bound that gives the re-broadcast claim enough
blocks to finalise after one reorg.  A second reorg adds nothing
adversarially: the secret is already out, so the only effect of
further reorgs in the abstract model is to permit an unfair lasso
(`claim → reorg → claim → reorg → …`) in which the asset oscillates
between `PendingClaim` and `Locked` forever, never stabilising.

`MAX_REORGS = 1` therefore captures the spec's §2.4 assumption
faithfully — it is the smallest bound that exercises the
secret-reveal-survives-reorg behaviour without admitting a lasso the
spec rules out by its choice of Δ.  Setting `MAX_REORGS` back to a
large value reproduces the lasso counter-example under TLC, useful
when stress-testing the conservativity of Δ.

### A3.  Finite secret space (`SECRETS = 1.to(2)`)

Spec adaptor secrets $t_i$ are drawn uniformly at random from the
scalar field (§4).  The formalization:

```quint
val SECRETS: Set[int] = 1.to(2)
```

**Significance — none for the safety/liveness properties.**  Secret
values play no role in any guard or post-condition the formalization
checks: the model abstracts cryptographic content (see B1 and B3
below), so secret *identity* and not secret *value* is what matters.
A 2-element set suffices to nondeterministically initialise the
parties' adaptor secrets distinctly; making it larger has no effect
on the proofs.  Listed here because TLC requires effective finiteness
and the bound is visible in the model.

### B1.  No off-chain message content; per-recipient inboxes record sender identity only

The prose spec (§5, §7.1) defines structured off-chain messages — Round-1
`Key1Resp(X_j)`, Round-2 `Key2Resp(R_j)`, Round-3
`Key3Req(Tagg)`/`Key3Resp(s'_j)`, Round-4
`SignInTxReq(dep_i, ref_i, W_i)`/`SignInTxResp(sig)`, secret reveal —
each carrying real cryptographic content.

The formalization replaces each off-chain message kind with three
state variables tracking *sender identity* per recipient:

```quint
var adaptorReceivedAt:  Party -> Set[Party]   // r ↦ senders r received from
var adaptorDecided:     Set[Party]            // senders who committed
var adaptorEquivocated: Party -> Set[Party]   // r ↦ senders r got a divergent X from
// …same triple for key, nonce, tAgg, secret
```

A send is one-shot per sender (`*Decided` records commitment;
subsequent sends from the same sender are disabled).  Honest senders
are forced to broadcast to all recipients; dishonest senders may
choose any subset of recipients (selective send) and any subset of
those recipients to receive a divergent payload (equivocation, B3
below).

**Significance — sound over-approximation.**  Guards never branch on
message *content*; they only check whether the relevant sender's
contribution is present in the recipient's inbox (and not flagged as
equivocated).  Round-completion predicates reduce to set equality:

```quint
pure def allAdaptorPointsPresentFor(r: Party, m: Party -> Set[Party]): bool =
  m.get(r) == PARTIES
```

The abstraction loses the distinction between "received the correct
$X_j$" and "received any $X_j$"; it recovers this distinction
through the equivocation flag (a recipient who received a divergent
payload is in `*Equivocated`).  Cryptographic verification of the
content (B2) is treated separately, as a non-modelled assumption.

The state shape `Party -> Set[Party]` is small enough that Apalache
can type-anchor it (versus a `Party -> Set[Msg]` with 30+ message
variants per inbox, which it cannot).

### B2.  Cryptographic verification is vacuous

The prose spec relies on Schnorr adaptor signatures, MuSig2
aggregation, and verifyTAgg checks at Round 3 (§5.4).  In the
formalization, all such checks succeed by construction:

- `adaptorPoint(t) = t` (the conceptual identity map).
- `verifyTAgg` is not modelled — the round-3 advance gate checks only
  inbox membership and (post B3) absence of equivocation.
- Signatures are not values; `preSigs` and `coSigs` are
  `Party -> Set[Party]` blackboards recording *which depositors a
  signer has signed for*.

This is consistent with the spec's own scope statement (§13,
"Cryptographic soundness: the adaptor signature scheme, the MuSig2
multi-signature scheme, and their combination are assumed correct
and secure").

**Significance — sound modulo the same assumption the spec makes.**
The proofs hold under the assumption that an honest signer's
cryptographic output verifies for every honest recipient that
received it.  Any attack that relies on forging or breaking the
signature primitive is out of scope here, as in the spec.

### B3.  Equivocation as a per-recipient divergence flag

Equivocation — a dishonest sender delivering *different* content to
different recipients in the same round — is modelled with one bit per
(sender, recipient, message kind):

```quint
var keyEquivocated: Party -> Set[Party]   // r ↦ senders r got a divergent key from
```

Honest senders are forced to `divergent == Set()`; dishonest senders
may pick any `divergent ⊆ recipients ∩ HONEST`.  Each honest
phase-advance gate refuses to fire while the corresponding
`*Equivocated[p]` is non-empty (modelling downstream detection
through commit-and-reveal / hash-commitment checks).

**Significance — sound over-approximation of a faithful per-payload
model.**  The faithful representation would re-introduce message
identity (`keySent: Party -> Party -> Option[Key]` per sender per
recipient).  The divergence flag is strictly weaker:
it does not require some other recipient to have received a
*canonical* payload before the attacker can be said to have
equivocated.  Any real adversary is at least as restricted, so
safety/liveness conclusions established here apply to the faithful
model as well.  A future property whose truth depends on the *value*
of the divergent payload (e.g. "the equivocated payload still lets
the receiver recover something") would need the faithful encoding.

The further state-space reduction `divergent ⊆ recipients ∩ HONEST`
is behavior-preserving: only honest parties consult `*Equivocated`,
so equivocating to a dishonest recipient is operationally inert.

### B4.  PreSig / coSig as global blackboards (not per-recipient inboxes)

Spec Rounds 3 and 4 produce pre-signatures and co-signatures that
travel to specific recipients:

- Round 3: claimant $P_{(i+1)\bmod N}$ collects per-party
  pre-signatures $s'_j$ for transfer $i$'s withdraw tx.
- Round 4: depositor $P_i$ collects per-party co-signatures for
  transfer $i$'s refund tx.

The formalization collapses both to global blackboards:

```quint
var preSigs: Party -> Set[Party]   // preSigs[signer] = depositors signer has signed for
var coSigs:  Party -> Set[Party]
```

Once `q` has signed for depositor `d`, the fact is visible to every
observer; no `*Equivocated` flag is tracked.

**Significance — sound under B2 and B3.**  Equivocation on a
signature is downstream of equivocation on its inputs (adaptor
points, keys, nonces, T_agg): a signer who equivocated on those
already produces signatures that fail verification from at least one
recipient's view.  Because cryptographic verification is abstracted
as vacuous (B2), an explicit `*Equivocated` flag on signatures would
be redundant — the honest-input gates on `sendPreSig` and
`sendCoSig` carry the necessary work (honest `q` will not co-sign
for `d` if `tAggEquivocated.get(q).contains(d)`, etc.).

### B5.  Per-transfer Setup rounds collapsed to per-party events

The prose spec §5 runs Rounds 1–4 *per transfer*: for $N = 3$ there
are three independent instances of each round, indexed by which
transfer they belong to.  The formalization fires each off-chain
send action *once per sender*, with no per-transfer index:

```quint
action sendKeyTo(p: Party, recipients: Set[Party], …)   // one fire per p
action sendNonceTo(p: Party, recipients: Set[Party], …) // one fire per p
```

Only `sendPreSig(p, d)` and `sendCoSig(q, d)` retain a per-transfer
parameter (the depositor `d`), because the inductive invariants
need to talk about which transfers a given signer has signed for.

**Significance — sound for the public-key and adaptor-point
material, an abstraction for the per-transfer nonces.**  Public keys
in MuSig2 are long-term (one per signer across all transfers); the
adaptor point $T_j$ is also one per party per swap.  Collapsing
these to a single send per party is faithful.  Per-transfer *nonces*
($R_j$ in Round 2) are normally fresh per signature; the
formalization elides this distinction.  None of the safety or
liveness properties checked here depend on nonce uniqueness (which
is a cryptographic soundness property of MuSig2, deferred to B2), so
the collapse is sound for what is being verified.

### B6.  Setup phase split into five sub-phases

The spec presents Setup as a single phase (Figure 2) containing the
preliminary adaptor-point broadcast plus four numbered rounds.  The
formalization splits Setup into five sub-phases per party:

```quint
type Phase =
    PhaseConfig
  | PhaseSetupAdaptorBroadcast
  | PhaseSetupKey1
  | PhaseSetupKey2
  | PhaseSetupKey3
  | PhaseSetupSignInTx
  | PhaseLock
  | PhaseClaim
```

**Significance — none; ergonomic, not semantic.**  The five
sub-phases correspond exactly to the four spec rounds plus the
preliminary adaptor-point broadcast.  Splitting them gives each its
own advance action and its own inductive-invariant conjuncts,
which simplifies the inductive proof but does not change the set of
reachable states the spec admits.

### B7.  `t_agg` value abstracted to a boolean

The spec's trigger (§7.1) is the leader's withdraw transaction
revealing $t_{agg}$ on-chain.  The formalization tracks only whether
the trigger has fired:

```quint
var triggerBroadcast: bool
```

The leader's `trigger` action sets it to `true`; downstream actions
(`claim` by non-leaders) read it as the enabling condition.

**Significance — sound.**  Because cryptographic verification is
vacuous (B2), no guard branches on the literal value of $t_{agg}$; a
boolean trigger flag carries every observable effect of the trigger
in the abstracted model.

### C1.  Block heights replaced by stutter-style time progression

The spec parameterises the refund windows by absolute block heights
$W_0 > W_1 > \cdots > W_{N-1}$ (§2.4) and refers to "the current
block height" throughout.  The formalization carries no block-height
variable.  Refund windows open through a single action:

```quint
action openNextWindow: bool =
  nondet d = oneOf(PARTIES)
  all {
    noOtherActionEnabled,
    not(openWindows.contains(d)),
    PARTIES.forall(q =>
      WINDOWS.get(q) < WINDOWS.get(d) implies openWindows.contains(q)),
    openWindows' = openWindows.union(Set(d)),
    …
  }
```

The `WINDOWS` constants are present only to encode the staggered
ordering ("the unopened party with the smallest $W$ is opened
next"); the `noOtherActionEnabled` gate means time advances precisely
when the protocol has run out of progress steps.

**Significance — abstracts both chain timing and the Δ-gap into
"advance only when stuck".**  The spec's Δ-gap (§2.4) is a
quantitative statement — honest recipients have at least Δ blocks to
claim before the next refund window opens.  The formalization
replaces "block-height counting" with "stutter-style time
progression": time *cannot* advance while any other action is
enabled, so any honest party that *can* still claim or advance Setup
*will* (under fairness on `stepBy(p)`) before windows open further.
The `refund` action carries no separate Δ-gap guard — once a claim
fires, the asset is no longer `Locked` and refund is disabled by its
own enabling predicate.

This is sound for the liveness properties checked here, which only
depend on whether honest parties eventually settle, not on how many
abstract steps elapse before they do.  Properties that depend on a
specific scheduling — for instance, exact bounds on settlement
latency — would need a richer time model.

### C2.  Adversarial behaviour modelled by `HONEST` and explicit withholds

The prose spec defines correctness against parties that may deviate
arbitrarily — withhold messages, refund opportunistically, collude.
The formalization captures this through:

- A configurable `HONEST: Set[Party]` constant.  Parties not in
  `HONEST` are unconstrained except by the chain rules.
- Honest senders are forced to broadcast (`recipients == PARTIES`)
  and to not equivocate (`divergent == Set()`); dishonest senders
  pick any subset.
- On-chain withholding actions (`withholdLock`, `withholdClaim`,
  `withholdTrigger`) that are enabled only when the actor is not in
  `HONEST`.

**Significance — refines what the spec leaves implicit.**  The spec
talks about "the honest path" and "what happens under adversarial
timing" without naming a formal honest/dishonest split.  The
formalization makes it explicit and verifies each non-empty subset
of `PARTIES` (seven profiles for $N = 3$) separately.  `invAtomicity`
is meaningful only under `HONEST = PARTIES`; `invHonestNoLoss` is
the property that the spec actually claims for every profile.

---

## Model structure for Apalache

These are not departures from the protocol; they are encoding
requirements of Apalache's inductive-invariant mode.  Briefly:

- **State is split across many top-level `var`s** rather than held in
  a single record.  Apalache's assignment analysis needs each `var'`
  to have a self-contained assignment expression with no read of any
  unprimed variable that hasn't already been anchored.
- **Each transition is a top-level `action` named directly**
  (`lock(p)`, `trigger`, `refund(p)`, …) with explicit `var' = …`
  for every state variable.  Apalache encodes `step` as
  `∃choice. action₁ ∨ action₂ ∨ …`, so the disjunctive structure
  must be syntactically visible.
- **`step` is a flat disjunction** with priority encoded in transition
  guards (e.g. `finalizeClaims` requires `honestClaimsDone`) rather
  than in a multi-tier scheduler.
- **`typeInv` anchors every variable** in a `var.in(set)` clause
  (the Quint analogue of TLA+'s `TypeOK`).  This unblocks Apalache's
  analyser, which otherwise reports `<var> is used before it is
  assigned` because the inductive-step encoding leaves unprimed
  variables otherwise unconstrained.
- **The network is abstracted** to `Party -> Set[Party]` per off-chain
  message kind (see B1) so the SMT problem stays tractable.

`Ind` is strengthened iteratively against Apalache's
counter-examples — each counter-example names a fact the actions
maintain in concert that `Ind` does not yet jointly assert.  The
current `invAtomicityInductive` and `invHonestNoLossInductive` are
the fixed points of that loop.

---

## Verification recipes

### Safety — Apalache, unbounded

```bash
# atomicity (only meaningful under HONEST = PARTIES)
quint verify --inductive-invariant=invAtomicityInductive \
             --invariant=invAtomicity protocolFlat.qnt

# honest-no-loss (works under every HONEST profile)
quint verify --inductive-invariant=invHonestNoLossInductive \
             --invariant=invHonestNoLoss protocolFlat.qnt
```

These complete in seconds on the 3-party instantiation.  Without
`--inductive-invariant`, Apalache falls back to bounded model
checking up to `--max-steps`, which for this model is too slow to
be useful.

Edit `parameters.qnt`'s `HONEST` line to switch profile.  All seven
non-empty subsets of `PARTIES = {A, B, C}` (with `LEADER = A`) have
been verified for `invHonestNoLoss`:

```quint
val HONEST: Set[Party] = PARTIES                          // all honest — also covers invAtomicity
val HONEST: Set[Party] = Set("A", "B")                    // C dishonest
val HONEST: Set[Party] = Set("A", "C")                    // B dishonest
val HONEST: Set[Party] = PARTIES.exclude(Set(LEADER))     // honest-leader-attack
val HONEST: Set[Party] = Set("A")                         // only leader honest
val HONEST: Set[Party] = Set("B")                         // only B honest
val HONEST: Set[Party] = Set("C")                         // only C honest
```

`invAtomicity` is meaningful only under `HONEST = PARTIES`: when any
party is dishonest, that party may unilaterally refund its own
deposit while others claim, so atomicity
(`not(anyClaimed and anyRefunded)`) is not the right property.
`invHonestNoLoss` is the property the spec actually claims for every
profile.

### Liveness — TLC

Apalache does not support fairness (it errors out with
`Handling fairness is not supported yet!`), so liveness uses
`--backend=tlc`.  TLC enumerates the reachable state space and
checks Büchi-style conditions including the `weakFair` and
`strongFair` assumptions in `honestFairness`:

```quint
temporal honestFairness: bool = and {
  PARTIES.forall(p => weakFair(stepBy(p), allVars)),
  weakFair(openNextWindow, allVars),
  strongFair(finalizeClaims, allVars),
}
```

Strong fairness on `finalizeClaims` corresponds to the §2.4 finality
assumption (Δ blocks elapse eventually); weak fairness on
`openNextWindow` captures "time eventually advances when the
protocol is stuck"; weak fairness on `stepBy(p)` for every party
forces both honest progress and eventual dishonest commitment.

#### `honestSettled`

Every honest party's deposit eventually leaves `Locked` for good:

```quint
temporal honestSettled: bool =
  honestFairness implies
    HONEST.forall(p => eventually(always(
      assets.get(p) != Locked
    )))
```

The settled state is one of: claimed by the next party, refunded
after the depositor's own window opens, or never-locked
(`OwnedBy(p)` permanently — an honest party that couldn't complete
Setup because of a dishonest co-signer).  The
`eventually(always(…))` shape rules out transient `PendingClaim`
states satisfying the property only to be reverted by a later reorg;
the `MAX_REORGS` bound (A2) ensures `PendingClaim` stabilises after
finitely many reorgs.

```bash
quint verify --backend=tlc --temporal=honestSettled \
             --max-steps=25 protocolFlat.qnt
```

Discharges across all seven non-empty `HONEST` profiles when
`MAX_REORGS = 1`, in about six seconds each.

#### `atomicOutcome`

A strictly stronger property: every honest party eventually
stabilises in one of three *atomic-correct* shapes:

```quint
pure def claimedByNext(p: Party, assets: Party -> AssetStatus): bool =
  match assets.get(p) {
    | Claimed(q)      => q == recipientOf(p)
    | PendingClaim(q) => q == recipientOf(p)
    | _               => false
  }
pure def iClaimedIncoming(p: Party, assets: Party -> AssetStatus): bool =
  match assets.get(senderOf(p)) {
    | Claimed(q)      => q == p
    | PendingClaim(q) => q == p
    | _               => false
  }

temporal atomicOutcome: bool =
  honestFairness implies
    HONEST.forall(p => eventually(always(
      (claimedByNext(p, assets) and iClaimedIncoming(p, assets))
      or assets.get(p) == Refunded(p)
      or assets.get(p) == OwnedBy(p)
    )))
```

The three disjuncts:

- **Swap completed for $p$** — $p$'s deposit is claimed by the
  legitimate next party AND $p$'s incoming is claimed by $p$.
- **Refunded** — $p$ recovered its own deposit.
- **Never locked** — $p$ never put principal at risk (e.g. dishonest
  peer stalled Setup so $p$ couldn't lock).

`honestSettled` forbids only "stuck `Locked` forever"; it admits
asymmetric outcomes where, say, $p$'s deposit goes to the next party
but $p$'s own claim is lost.  `atomicOutcome` rules those out.  The
spec's §9 ("Partial claim") acknowledges that adversarial timing
*can* produce asymmetric outcomes against parties that don't fulfil
their liveness obligation; the stutter-style time progression (C1)
is what closes the claim-versus-refund race in the abstract model
and makes `atomicOutcome` discharge.

```bash
quint verify --backend=tlc --temporal=atomicOutcome \
             --max-steps=25 protocolFlat.qnt
```

Discharges across all seven non-empty `HONEST` profiles when
`MAX_REORGS = 1`, in about six seconds each.

#### Caveats

- TLC's `--max-steps` bounds search depth, not time-of-search; the
  proof covers the entire reachable state space discovered up to
  that depth, including detection of fair lassos.
- For the abstracted `protocolFlat` model the reachable state space
  is a few thousand states with `SECRETS = 1.to(2)` — comfortably
  within TLC's range.  Larger instantiations (more parties, more
  secrets) may hit memory pressure.
- TLC requires the model to be effectively finite.  The type-anchor
  bounds (`PARTIES.powerset()`, `0.to(MAX_REORGS)`, …) ensure that.

### Why `MAX_REORGS = 1`

See [A2](#a2--bounded-reorg-model-max_reorgs--1) for the protocol
argument: one reorg is what exercises the
secret-survives-reorg behaviour the spec's choice of Δ is designed
to absorb, and a second reorg adds nothing adversarially because the
secret is already revealed.

Operationally, `MAX_REORGS = 1` is also what makes both liveness
properties discharge under TLC.  After at most one reorg per claim,
the next publication finalises; strong fairness on `finalizeClaims`
drives `claimsFinalized` to true and the
`PendingClaim → Claimed` confirmation closes the loop.  For deposits
whose recipient never publishes, weak fairness on `openNextWindow`
opens the depositor's refund path.  Either way `assets.get(p)`
settles, and both `honestSettled` and `atomicOutcome` discharge.

Setting `MAX_REORGS` back to a large value reproduces a
`claim → reorg → claim → reorg → …` lasso under TLC.  Under
unbounded reorgs, `eventually(always(…))` requires permanent escape
from `Locked` and the lasso visits `Locked` infinitely often, so
both liveness properties fail.  This is a faithful counter-example
*relative to the unbounded-reorg chain model* — it does not falsify
the protocol; it confirms that the protocol's safety depends on the
spec's §2.4 finality assumption holding.

---

## Files

- `protocolFlat.qnt` — the canonical verification model.  `quint run`
  simulates; `quint verify --inductive-invariant=…` discharges safety;
  `quint verify --backend=tlc --temporal=…` discharges liveness.
- `types.qnt`, `parameters.qnt` — types and protocol constants.
- `spells/` — Quint stdlib helper modules.
- `scripts/`, `results/` — the profile-sweep automation and its
  output (see [`README.md`](README.md)).
