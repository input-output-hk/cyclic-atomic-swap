#!/usr/bin/env bash
#
# run-profile.sh PROFILE_NAME HONEST_EXPR
#
# Runs all model-checking checks against protocolFlat.qnt under one HONEST
# profile.  Mutates parameters.qnt's HONEST line in place; restores the
# original content on exit (success, failure, or interruption).
#
# Writes one TSV row per check to results/summary.tsv.
# Exits 0 if every check in this profile passed, 1 otherwise.

set -uo pipefail

if [[ $# -ne 2 ]]; then
  echo "usage: $0 PROFILE_NAME HONEST_EXPR" >&2
  exit 2
fi

PROFILE="$1"
HONEST_EXPR="$2"

# Locate the spec directory (parent of scripts/) so this script works
# whether invoked from formal-methods/, scripts/, or elsewhere.
SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
SPEC_DIR="$(cd "$SCRIPT_DIR/.." && pwd)"
cd "$SPEC_DIR"

SPEC=protocolFlat.qnt
PARAMS=parameters.qnt
RESULTS=results/summary.tsv
mkdir -p results

# Save current parameters.qnt to a sibling backup file and arrange
# unconditional restore.  We use a file rather than a $(cat ...) variable
# because bash's command-substitution strips trailing newlines, which
# would silently corrupt the source file across runs.
PARAMS_BACKUP="${PARAMS}.runprofile.bak"

# Defensive: a leftover backup means a previous run was killed before
# its restore trap could fire (e.g. SIGKILL).  Don't overwrite it —
# instead refuse and tell the user how to recover.
if [[ -f "$PARAMS_BACKUP" ]]; then
  echo "ERROR: $PARAMS_BACKUP already exists from a previous run." >&2
  echo "       To recover the original parameters.qnt, run:" >&2
  echo "         mv '$PARAMS_BACKUP' '$PARAMS'" >&2
  echo "       Or, if you have already manually restored the file:" >&2
  echo "         rm '$PARAMS_BACKUP'" >&2
  exit 3
fi

cp "$PARAMS" "$PARAMS_BACKUP"
restore_params() {
  if [[ -f "$PARAMS_BACKUP" ]]; then
    mv "$PARAMS_BACKUP" "$PARAMS"
  fi
}
trap restore_params EXIT INT TERM

# Substitute the HONEST line.  -i.bak is portable across GNU/BSD sed;
# we delete that backup ourselves (separate from PARAMS_BACKUP above).
sed -i.bak "s|^  val HONEST: Set\\[Party\\] =.*|  val HONEST: Set[Party] = $HONEST_EXPR|" "$PARAMS"
rm -f "${PARAMS}.bak"

# ---- Helper: run one check, append a TSV row, return 0/1 ----
run_check () {
  local label="$1"; shift
  local timeout_s="$1"; shift
  local start end secs status
  start=$(date +%s)
  if env JVM_ARGS=-Xmx12G timeout "$timeout_s" "$@" 2>&1 \
       | grep -qE '^\[ok\] No violation found'; then
    status="✓"
  else
    status="✗"
  fi
  end=$(date +%s)
  secs=$((end - start))
  printf "%s\t%s\t%s\t%ds\n" "$PROFILE" "$label" "$status" "$secs" >> "$RESULTS"
  echo "  [$status] $label (${secs}s)"
  [[ "$status" == "✓" ]]
}

echo "=== Profile: $PROFILE  (HONEST = $HONEST_EXPR) ==="

failed=0

run_check invHonestNoLoss 600 \
  quint verify --inductive-invariant=invHonestNoLossInductive --invariant=invHonestNoLoss "$SPEC" \
  || failed=1

# `atomicOutcome` strictly subsumes `honestSettled` — its three valid
# terminal shapes (Claimed/PendingClaim, Refunded, OwnedBy) all exclude
# Locked, so atomicOutcome ⇒ honestSettled.  Only the stronger property
# is checked here.  `temporal honestSettled` is kept in protocolFlat.qnt
# for ad-hoc debugging when atomicOutcome fails.
run_check atomicOutcome 600 \
  quint verify --backend=tlc --temporal=atomicOutcome --max-steps=25 "$SPEC" \
  || failed=1

# Atomicity is only meaningful when every party is honest.
if [[ "$HONEST_EXPR" == "PARTIES" ]]; then
  run_check invAtomicity 600 \
    quint verify --inductive-invariant=invAtomicityInductive --invariant=invAtomicity "$SPEC" \
    || failed=1
fi

exit $failed
