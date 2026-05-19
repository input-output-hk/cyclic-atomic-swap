#!/usr/bin/env bash
#
# verify-all.sh
#
# Sweep all seven non-empty HONEST profiles for the 3-party model.
# Calls run-profile.sh for each.  Resets results/summary.tsv at the
# start, prints a Markdown-style summary at the end.
#
# Exits 0 if every check in every profile passed, 1 otherwise.

set -uo pipefail

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
SPEC_DIR="$(cd "$SCRIPT_DIR/.." && pwd)"
cd "$SPEC_DIR"

mkdir -p results
SUMMARY=results/summary.tsv
printf "profile\tcheck\tresult\ttime\n" > "$SUMMARY"

# (display name, HONEST expression).  Display names are short tokens used
# in the summary table; the expressions are pasted into parameters.qnt.
PROFILES=(
  "all_honest|PARTIES"
  "no_C|Set(\"A\", \"B\")"
  "no_B|Set(\"A\", \"C\")"
  "dishonest_leader|PARTIES.exclude(Set(LEADER))"
  "only_A|Set(\"A\")"
  "only_B|Set(\"B\")"
  "only_C|Set(\"C\")"
)

failed=0
for entry in "${PROFILES[@]}"; do
  name="${entry%%|*}"
  expr="${entry#*|}"
  ./scripts/run-profile.sh "$name" "$expr" || failed=1
  echo
done

# ---- Pretty-print summary ----
echo "================================================================"
echo "Summary"
echo "================================================================"
awk -F '\t' '
  NR == 1 { next }                      # skip header row
  { results[$1, $2] = $3
    times[$1, $2]   = $4
    profiles[$1]    = 1
    checks[$2]      = 1 }
  END {
    # Stable column order
    n_checks = split("invHonestNoLoss atomicOutcome invAtomicity",
                     check_list, " ")
    n_profs = split("all_honest no_C no_B dishonest_leader only_A only_B only_C",
                    prof_list, " ")
    # Header
    printf "%-20s", "profile"
    for (i = 1; i <= n_checks; i++) printf " | %-16s", check_list[i]
    print ""
    printf "%-20s", "--------------------"
    for (i = 1; i <= n_checks; i++) printf "-+-%-16s", "----------------"
    print ""
    for (j = 1; j <= n_profs; j++) {
      p = prof_list[j]
      printf "%-20s", p
      for (i = 1; i <= n_checks; i++) {
        c = check_list[i]
        if ((p, c) in results)
          printf " | %s %-14s", results[p, c], times[p, c]
        else
          printf " | %-16s", "-"
      }
      print ""
    }
  }
' "$SUMMARY"

if [[ $failed -ne 0 ]]; then
  echo
  echo "FAIL: at least one check did not pass"
  exit 1
fi
echo
echo "OK: every check passed"
