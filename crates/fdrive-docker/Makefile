ROOT := $(abspath ../..)
PLUGIN := machines/fdrive
BUILD := $(ROOT)/target/docker-plugin

all: build start

build:
	cargo build -p fdrive-docker

start:
	sudo $(ROOT)/target/debug/fdrive-docker

plugin:
	-docker plugin rm -f $(PLUGIN)
	cargo build --release -p fdrive-docker
	tar -c -C docker Dockerfile -C $(ROOT)/target/release fdrive-docker | docker build -t fdrive-rootfs -
	rm -rf $(BUILD) && mkdir -p $(BUILD)/rootfs && cp docker/config.json $(BUILD)/
	id=$$(docker create fdrive-rootfs) && docker export $$id | tar -x -C $(BUILD)/rootfs && docker rm $$id
	docker plugin create $(PLUGIN) $(BUILD)
	docker plugin push $(PLUGIN)
