FROM rust:1.90.0-alpine AS builder

RUN apk add --no-cache musl-dev

WORKDIR /app

COPY Cargo.toml Cargo.lock ./

RUN mkdir src && echo "fn main() {}" > src/main.rs

RUN cargo build --release && rm -rf src

COPY src ./src

RUN cargo build --release

FROM alpine:3.19

RUN apk add --no-cache ca-certificates

RUN addgroup -g 1000 app && adduser -D -s /bin/sh -u 1000 -G app app

COPY --from=builder /app/target/release/openvibe-server /usr/local/bin/openvibe-server

USER app

EXPOSE 3000

ENV SERVER_PORT=3000

# Run the application
CMD ["openvibe-server"]