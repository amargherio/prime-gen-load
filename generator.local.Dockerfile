FROM ubuntu:latest
WORKDIR /app

COPY target/release/pod-generator ./pod-generator
ENTRYPOINT [ "./pod-generator" ]