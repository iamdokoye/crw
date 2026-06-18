FROM debian:bookworm-slim

RUN apt-get update && apt-get install -y --no-install-recommends \
    ca-certificates curl && rm -rf /var/lib/apt/lists/*

ARG CRW_VERSION=0.16.0

# Download pre-built ARM64 binary — ~30s vs 10-min Rust compile (avoids Coolify SSH timeout)
RUN curl -fsSL \
    "https://github.com/us/crw/releases/download/v${CRW_VERSION}/crw-server-linux-arm64.tar.gz" \
    | tar -xz -C /usr/local/bin/ \
    && chmod +x /usr/local/bin/crw-server

COPY config.default.toml /app/config.default.toml
COPY config.docker.toml  /app/config.docker.toml

WORKDIR /app

LABEL io.modelcontextprotocol.server.name="io.github.us/crw"

EXPOSE 3000

ENV CRW_CONFIG=config.docker \
    RUST_LOG=info

CMD ["crw-server"]
