#!/usr/bin/env bash
# Full regression suite with live dashboard enabled.
# Each test gets a fresh network to avoid state bleed between runs.
# Open http://localhost:5173 (npm run dev in the dashboard directory) to
# watch the swap ring and state transitions live. The dashboard stays
# alive for 120 s after each test completes.
#
# The blockchain test environment lives in a separate repository
# (input-output-hk/btc-defi-atomic-swaps-test-env, currently IOG-internal).
# Set TESTENV_DIR to point at your clone; it defaults to a sibling directory
# of this repository.
set -e

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
TESTENV="${TESTENV_DIR:-$REPO_ROOT/../btc-defi-atomic-swaps-test-env}"
DOLOS_REST_URL=http://localhost:50051

if [ ! -x "$TESTENV/cli/testenv" ]; then
    cat >&2 <<EOF
error: blockchain test environment not found at
         $TESTENV

The regression suite needs the dockerised Bitcoin + Cardano network from
input-output-hk/btc-defi-atomic-swaps-test-env (currently an IOG-internal
repository — open an issue on this repository to request access).

  git clone git@github.com:input-output-hk/btc-defi-atomic-swaps-test-env.git
  export TESTENV_DIR=/path/to/btc-defi-atomic-swaps-test-env

Without it, run the tests that need no private network:  cargo test
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

echo y | $TESTENV/cli/testenv start
wait_for_dolos
cargo test --features regtest,dashboard --test completed_regression_test -- --nocapture

echo y | $TESTENV/cli/testenv start
wait_for_dolos
cargo test --features regtest,dashboard --test 20_party_completed_regression_test -- --nocapture

echo y | $TESTENV/cli/testenv start
wait_for_dolos
cargo test --features regtest,dashboard --test refunded_regression_test -- --nocapture

echo y | $TESTENV/cli/testenv start
wait_for_dolos
cargo test --features regtest,dashboard --test failed_regression_test -- --nocapture

echo y | $TESTENV/cli/testenv start
wait_for_dolos
cargo test --features regtest --test early_refund_rejection_regression_test -- --nocapture
