FROM ubuntu:latest
WORKDIR /app

COPY target/release/instance-service ./instance-service
ENTRYPOINT [ "./instance-service" ]