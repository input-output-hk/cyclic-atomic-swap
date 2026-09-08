#!/usr/bin/env bash
# Full regression suite with live dashboard enabled.
# Each test gets a fresh network to avoid state bleed between runs.
# Open http://localhost:5173 (npm run dev in the dashboard directory) to
# watch the swap ring and state transitions live. The dashboard stays
# alive for 120 s after each test completes.
#
# The dockerised Bitcoin + Cardano network lives in test-env/ at the root of
# this repository. Set TESTENV_DIR to override that location.
set -e

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
TESTENV="${TESTENV_DIR:-$REPO_ROOT/test-env}"
DOLOS_REST_URL=http://localhost:50051

if [ ! -x "$TESTENV/testenv" ]; then
    cat >&2 <<EOF
error: test network control script not found at
         $TESTENV/testenv

It should be in test-env/ at the root of this repository; see
test-env/README.md. Set TESTENV_DIR if you keep it elsewhere.

To run only the tests that need no network:  cargo test
EOF
    exit 1
fi

wait_for_dolos() {
    echo "waiting for Dolos to be ready..."
    until curl -sf "$DOLOS_REST_URL/blocks/latest" > /dev/null 2>&1; do
        sleep 2
    done
    echo "Dolos responding — waiting for chain tip to stabilise..."
    local prev_slot=""
    local attempts=0
    while [ $attempts -lt 40 ]; do
        local curr_slot
        curr_slot=$(curl -sf "$DOLOS_REST_URL/blocks/latest" | grep -o '"slot":[0-9]*' | grep -o '[0-9]*$')
        if [ -n "$curr_slot" ] && [ "$curr_slot" = "$prev_slot" ]; then
            echo "Dolos ready at slot $curr_slot."
            return
        fi
        prev_slot="$curr_slot"
        attempts=$((attempts + 1))
        sleep 3
    done
    echo "warning: Dolos slot did not stabilise after $((40 * 3))s, proceeding anyway"
}

"$TESTENV"/testenv start
wait_for_dolos
cargo test --features regtest,dashboard --test completed_regression_test -- --nocapture

"$TESTENV"/testenv start
wait_for_dolos
cargo test --features regtest,dashboard --test 20_party_completed_regression_test -- --nocapture

"$TESTENV"/testenv start
wait_for_dolos
cargo test --features regtest,dashboard --test refunded_regression_test -- --nocapture

"$TESTENV"/testenv start
wait_for_dolos
cargo test --features regtest,dashboard --test failed_regression_test -- --nocapture

"$TESTENV"/testenv start
wait_for_dolos
cargo test --features regtest --test early_refund_rejection_regression_test -- --nocapture
