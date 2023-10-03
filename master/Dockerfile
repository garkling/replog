FROM rust:latest

WORKDIR /usr/src/master

RUN echo "fn main() { }" > dummy.rs
COPY master/Cargo.toml .

# to avoid re-installing of the dependencies on every change in src/
RUN sed -i 's#src/main.rs#dummy.rs#' Cargo.toml
RUN cargo build --release
RUN sed -i 's#dummy.rs#src/main.rs#' Cargo.toml


COPY master/src            ./src
COPY log-config.yml .

RUN cargo build --release

EXPOSE 10000

CMD ["./target/release/master"]
