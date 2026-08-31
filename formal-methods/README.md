# Cyclic Atomic Swap — Quint Specification

This directory contains the Quint formalisation of the N-party cyclic
atomic swap.  The active verification model is `protocolFlat.qnt`; the
prose protocol description lives in [`../docs/protocol-spec.tex`](../docs/protocol-spec.tex).

For the technical write-up of how the model was adapted to verify
inductively under Apalache (and bounded-explicit under TLC), see
[`model-checking-notes.md`](model-checking-notes.md).

## Layout

```
formal-methods/
  protocolFlat.qnt           # the canonical verification model
  parameters.qnt             # PARTIES, LEADER, HONEST, WINDOWS, DELTA, MAX_REORGS
  types.qnt                  # Party, Phase, AssetStatus
  spells/                    # Quint stdlib helpers
  model-checking-notes.md    # how the model was adapted for verify
  README.md                  # this file
  scripts/
    run-profile.sh           # one HONEST profile, all checks
    verify-all.sh            # sweep all 7 profiles
  results/
    summary.tsv              # written by run-profile.sh
```

## Verifying the model

### One-shot: all seven profiles

From this directory:

```bash
./scripts/verify-all.sh
```

Runs:

| profile | `HONEST` | what is checked |
|---|---|---|
| `all_honest` | `PARTIES` | `invAtomicity` + `invHonestNoLoss` + `atomicOutcome` |
| `no_C` | `Set("A","B")` | `invHonestNoLoss` + `atomicOutcome` |
| `no_B` | `Set("A","C")` | `invHonestNoLoss` + `atomicOutcome` |
| `dishonest_leader` | `PARTIES.exclude(Set(LEADER))` | `invHonestNoLoss` + `atomicOutcome` |
| `only_A` | `Set("A")` | `invHonestNoLoss` + `atomicOutcome` |
| `only_B` | `Set("B")` | `invHonestNoLoss` + `atomicOutcome` |
| `only_C` | `Set("C")` | `invHonestNoLoss` + `atomicOutcome` |

Wall-clock: ~5 minutes total.  Each `invHonestNoLoss` / `invAtomicity`
job is ~30 s under Apalache; each `atomicOutcome` job is ~10 s under TLC.

The script writes one TSV row per check to `results/summary.tsv` and
prints a Markdown-style summary table at the end.  Exits 0 if every
check passed, 1 otherwise.

### One profile at a time

```bash
./scripts/run-profile.sh PROFILE_NAME 'HONEST_EXPR'
```

For example:

```bash
./scripts/run-profile.sh only_B 'Set("B")'
./scripts/run-profile.sh dishonest_leader 'PARTIES.exclude(Set(LEADER))'
```

### One check, ad hoc

If you want to run a single check directly (no profile sweep, no scripts):

```bash
# Edit parameters.qnt's HONEST line manually, then:

# Safety (Apalache, unbounded inductive proof):
quint verify --inductive-invariant=invHonestNoLossInductive --invariant=invHonestNoLoss protocolFlat.qnt
quint verify --inductive-invariant=invAtomicityInductive    --invariant=invAtomicity    protocolFlat.qnt

# Liveness (TLC, bounded explicit-state):
quint verify --backend=tlc --temporal=atomicOutcome --max-steps=25 protocolFlat.qnt

# Weaker settled-liveness variant, kept for debugging when atomicOutcome fails:
quint verify --backend=tlc --temporal=honestSettled --max-steps=25 protocolFlat.qnt

# Simulation:
quint run --invariant=invHonestNoLoss --max-steps=200 --max-samples=300 protocolFlat.qnt
```

The scripts just automate this loop and restore `parameters.qnt`.

## How the scripts work

`run-profile.sh` mutates `parameters.qnt` to set the `HONEST` line and
restores it afterwards.  The mutation is byte-precise: a copy of the
original is saved to `parameters.qnt.runprofile.bak` and `mv`'d back on
exit (success, failure, `Ctrl-C`, or `SIGTERM`).

`SIGKILL` cannot be caught, so a forcibly-killed run will leave both
`parameters.qnt` (modified) and `parameters.qnt.runprofile.bak` (the
original) on disk.  The next invocation of `run-profile.sh` detects
this and refuses to start; recover with:

```bash
mv parameters.qnt.runprofile.bak parameters.qnt
```

Likewise if you Ctrl-C during a sweep and see anything unexpected, the
backup is the safe source of truth.

## Requirements

- [Quint](https://github.com/informalsystems/quint) (`npm install -g @informalsystems/quint`)
- Java 17+ (for the Apalache and TLC backends Quint downloads)
- ~12 GB free RAM for Apalache jobs (`JVM_ARGS=-Xmx12G` is set by the scripts)
- `bash`, `sed`, `awk` (standard on macOS / Linux)

## Properties at a glance

| property | meaning | backend | mode |
|---|---|---|---|
| `invAtomicity` | no claim and refund coexist | Apalache | inductive (unbounded) |
| `invHonestNoLoss` | every honest party recovers either deposit or claim | Apalache | inductive (unbounded) |
| `atomicOutcome` | every honest party eventually stabilises in an atomic-correct shape: swap completed for them, refunded, or never locked | TLC | temporal (BMC, ≤25 steps) |
| `honestSettled` | every honest party's deposit eventually leaves Locked for good | TLC | temporal (BMC, ≤25 steps) |

`atomicOutcome` strictly subsumes `honestSettled`, so the scripts run
only the former; `honestSettled` is kept in the model for ad-hoc
debugging when `atomicOutcome` fails.

Atomicity is meaningful only for `HONEST = PARTIES`; with any
adversary, that adversary can refund unilaterally while honest parties
claim, breaking atomicity but not honest-no-loss.  See
[`model-checking-notes.md`](model-checking-notes.md) for the reasoning
behind each invariant's strengthening, the `MAX_REORGS = 1` choice for
liveness, and which model changes were driven by which backend.

## Editing `HONEST` for ad-hoc work

`parameters.qnt` ships with `HONEST = PARTIES`.  To explore other
profiles without using the scripts, just edit that line.  The model
typechecks for any `Set[Party]` value you put there; the seven values
in the table above are the ones with verified results.

If you want to scale up (e.g. N=4 parties), more parameters need to
change in tandem: `PARTIES`, `LEADER`, `TRANSFERS`, `INVTRANSFERS`,
`WINDOWS` (preserving the staggering invariant `windowsOrdered`).
The scripts do not currently parameterise the party count — they only
sweep `HONEST` for the existing 3-party instantiation.
