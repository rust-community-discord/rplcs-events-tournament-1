# filepath: /e:/projects/rplcs_server_events/rplcs-tournament-1-http/submissions/example_submission_1/Dockerfile
FROM rust:1.85 as builder

WORKDIR /usr/src/app
# Copy rplcs_events first to improve caching
COPY rplcs_events ./rplcs_events
# Then copy the rest of the files
COPY ./Cargo.toml ./Cargo.toml
COPY ./src ./src
RUN cargo build --release
RUN ls -la target/release

FROM debian:12-slim
COPY --from=builder /usr/src/app/target/release/example_submission_3 /usr/local/bin/example_submission_3

EXPOSE 3000
CMD ["example_submission_3"]
