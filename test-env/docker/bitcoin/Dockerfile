FROM bitcoin/bitcoin:27.0

# Install additional utilities if needed
RUN apt-get update && \
    apt-get install -y --no-install-recommends \
    curl \
    jq \
    && rm -rf /var/lib/apt/lists/*

# Create bitcoin user and data directory
RUN mkdir -p /home/bitcoin/.bitcoin && \
    chown -R bitcoin:bitcoin /home/bitcoin/.bitcoin

# Copy configuration files
COPY bitcoin.conf /home/bitcoin/.bitcoin/bitcoin.conf
COPY entrypoint.sh /entrypoint.sh

RUN chmod +x /entrypoint.sh && \
    chown bitcoin:bitcoin /home/bitcoin/.bitcoin/bitcoin.conf

# Set volume for blockchain data
VOLUME ["/home/bitcoin/.bitcoin"]

# Expose regtest RPC port, P2P port, and ZMQ ports
EXPOSE 18443 18444 28332 28333

# Switch to bitcoin user
USER bitcoin

# Set working directory
WORKDIR /home/bitcoin

# Use custom entrypoint
ENTRYPOINT ["/entrypoint.sh"]
