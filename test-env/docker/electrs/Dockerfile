FROM mempool/electrs:latest

# Install curl for healthchecks
USER root
RUN apt-get update && \
    apt-get install -y --no-install-recommends curl netcat-openbsd && \
    rm -rf /var/lib/apt/lists/*

# Use base image defaults for everything else
