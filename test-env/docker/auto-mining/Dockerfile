FROM python:3.11-slim

# Install system dependencies
RUN apt-get update && \
    apt-get install -y --no-install-recommends \
    build-essential \
    libzmq3-dev \
    && rm -rf /var/lib/apt/lists/*

# Install Python dependencies
RUN pip install --no-cache-dir \
    pyzmq==25.1.2 \
    requests==2.31.0 \
    pyyaml==6.0.1

# Create app directory
WORKDIR /app

# Copy auto-miner script
COPY auto_miner.py /app/

# Create config volume mount point
RUN mkdir -p /config

# Run as non-root user
RUN useradd -m -u 1000 autominer && \
    chown -R autominer:autominer /app
USER autominer

# Set entrypoint
ENTRYPOINT ["python3", "/app/auto_miner.py"]
