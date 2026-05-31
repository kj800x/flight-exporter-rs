FROM rust:1.96

WORKDIR /usr/src/flight-exporter-rs
COPY . .

RUN cargo install --path .

CMD ["flight-exporter-rs"]
