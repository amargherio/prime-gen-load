FROM ubuntu:latest
WORKDIR /app

COPY target/release/prime-sieve ./prime-sieve
ENTRYPOINT [ "./prime-sieve" ]