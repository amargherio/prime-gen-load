FROM ubuntu:latest
WORKDIR /app

RUN apt-get update -y && \
    apt-get upgrade -y && \
    apt-get install -y libssl-dev

COPY target/release/pod-generator ./pod-generator
ENTRYPOINT [ "./pod-generator" ]