FROM rust:1.91

WORKDIR /usr/src/flight-exporter-rs
COPY . .

RUN cargo install --path .

CMD ["flight-exporter-rs"]
