FROM rust:1.70.0-slim-bullseye AS build
ARG APP_NAME

WORKDIR /app

RUN apt update && apt install -y protobuf-compiler libprotobuf-dev

RUN --mount=type=bind,source=src,target=src \
    --mount=type=bind,source=build.rs,target=build.rs \
    --mount=type=bind,source=replica.proto,target=replica.proto \
    --mount=type=bind,source=Cargo.toml,target=Cargo.toml \
#    --mount=type=bind,source=Cargo.lock,target=Cargo.lock \
    --mount=type=cache,target=/app/target/ \
    --mount=type=cache,target=/usr/local/cargo/registry/ \
    <<EOF
set -e
cargo build --bin $APP_NAME --release
cp ./target/release/$APP_NAME /bin/server
EOF

FROM debian:bullseye-slim AS final

ARG UID=10001
ENV HOME="/home/appuser"
RUN adduser \
    --disabled-password \
    --gecos "" \
    --home "${HOME}" \
    --shell "/sbin/nologin" \
    --uid "${UID}" \
    appuser
USER appuser

COPY log-config.yml $HOME/
COPY --from=build /bin/server /bin/

EXPOSE 10000

CMD ["/bin/server"]
