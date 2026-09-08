# busybox supplies sed/grep/awk, which the cardano-node image does not ship.
# Taken from the official image so the architecture matches the build platform
# and no binary is committed to this repository.
FROM busybox:1.36-uclibc AS busybox

FROM ghcr.io/intersectmbo/cardano-node:9.2.1
COPY --from=busybox /bin/busybox /busybox

COPY entrypoint.sh /entrypoint.sh
RUN chmod +x /entrypoint.sh

WORKDIR /data
ENTRYPOINT ["/entrypoint.sh"]
