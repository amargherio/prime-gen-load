FROM ubuntu:latest
WORKDIR /app

RUN apt-get update -y && \
    apt-get upgrade -y && \
    apt-get install -y libssl-dev

COPY target/release/instance-service ./instance-service

EXPOSE 8080
ENTRYPOINT [ "./instance-service" ]