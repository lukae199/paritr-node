# syntax=docker/dockerfile:1.7
FROM rust:1.98.1-bookworm AS rust-builder
WORKDIR /src
COPY Cargo.toml Cargo.lock rust-toolchain.toml rustfmt.toml ./
COPY src ./src
RUN cargo build --release --locked

FROM debian:bookworm AS randomx-builder
ARG RANDOMX_TAG=v1.2.3
ARG RANDOMX_COMMIT=12f2c2ffe2108d6cf54c391fee33c8bc3646cdab
RUN apt-get update && apt-get install -y --no-install-recommends ca-certificates cmake g++ git make \
    && rm -rf /var/lib/apt/lists/*
RUN git clone --quiet --depth 1 --branch "$RANDOMX_TAG" https://github.com/tevador/RandomX.git /randomx \
    && test "$(git -C /randomx rev-parse HEAD)" = "$RANDOMX_COMMIT" \
    && cmake -S /randomx -B /randomx/build -DCMAKE_BUILD_TYPE=Release -DBUILD_SHARED_LIBS=ON \
    && cmake --build /randomx/build --config Release --parallel

FROM debian:bookworm-slim
RUN apt-get update && apt-get install -y --no-install-recommends ca-certificates curl libgcc-s1 libstdc++6 \
    && rm -rf /var/lib/apt/lists/* \
    && groupadd --gid 10001 paritr \
    && useradd --uid 10001 --gid paritr --home-dir /var/lib/paritr --create-home --shell /usr/sbin/nologin paritr
COPY --from=rust-builder /src/target/release/paritr-node /usr/local/bin/paritr-node
COPY --from=randomx-builder /randomx/build/librandomx.so /usr/local/lib/librandomx.so
ENV PARITR_RANDOMX_LIBRARY=/usr/local/lib/librandomx.so
WORKDIR /var/lib/paritr
RUN chown -R paritr:paritr /var/lib/paritr
USER 10001:10001
EXPOSE 5050
VOLUME ["/var/lib/paritr"]
HEALTHCHECK --interval=30s --timeout=5s --start-period=30s --retries=3 CMD curl -fsS http://127.0.0.1:5050/health || exit 1
ENTRYPOINT ["paritr-node", "--config", "/var/lib/paritr/config.json"]
CMD ["run"]
