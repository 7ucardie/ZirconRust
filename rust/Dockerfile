# Rust Zircon game server image.
#
#   docker build -t zircon-server -f rust/Dockerfile rust
#   docker run -p 7000:7000 -v ~/zircon-assets:/assets:ro zircon-server
#
# The image contains only the server binary. Game data is mounted at runtime:
#   /assets/Client/Map/*.map        maps
#   /assets/Database/System.db      static game content

FROM rust:1-bookworm AS build
WORKDIR /src
COPY Cargo.toml Cargo.lock ./
COPY crates ./crates
# The client needs windowing libraries; only build the server crates here.
RUN cargo build --release -p mir-server

FROM debian:bookworm-slim
RUN useradd --system --create-home zircon && mkdir -p /data && chown zircon /data
VOLUME /data
COPY --from=build /src/target/release/mir-server /usr/local/bin/mir-server
USER zircon
ENV ZIRCON_ASSETS=/assets/Client \
    ZIRCON_DB=/assets/Database/System.db \
    ZIRCON_PORT=7000 \
    ZIRCON_DATA=/data \
    RUST_LOG=info
EXPOSE 7000
ENTRYPOINT ["/usr/local/bin/mir-server"]
