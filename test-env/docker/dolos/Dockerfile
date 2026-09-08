# v1.6.0 or later is required: 1.0.0-rc.5 passes the validity range to the
# Plutus script context in seconds where milliseconds are expected, which makes
# it reject valid time-locked refunds.
FROM ghcr.io/txpipe/dolos:v1.6.0

COPY entrypoint.sh /entrypoint.sh
RUN chmod +x /entrypoint.sh

ENTRYPOINT ["/entrypoint.sh"]
