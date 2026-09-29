FROM debian:trixie-slim
RUN apt-get update && apt-get install -y --no-install-recommends fuse3 && rm -rf /var/lib/apt/lists/*
COPY fdrive-docker /usr/bin/fdrive-docker
