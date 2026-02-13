FROM rust:1.93.1-bookworm AS build-env
LABEL maintainer="yanorei32"

SHELL ["/bin/bash", "-o", "pipefail", "-c"]

WORKDIR /usr/src
COPY . /usr/src/discord-ip-miner/
WORKDIR /usr/src/discord-ip-miner
RUN cargo build --release && cargo install cargo-license && cargo license \
	--authors \
	--do-not-bundle \
	--avoid-dev-deps \
	--avoid-build-deps \
	--filter-platform "$(rustc -vV | sed -n 's|host: ||p')" \
	> CREDITS

FROM debian:bookworm-slim

RUN apt-get update; \
	apt-get install -y --no-install-recommends \
		libssl3 ca-certificates; \
	apt-get clean;

WORKDIR /

COPY --chown=root:root --from=build-env \
	/usr/src/discord-ip-miner/CREDITS \
	/usr/src/discord-ip-miner/LICENSE \
	/usr/share/licenses/discord-ip-miner/

COPY --chown=root:root --from=build-env \
	/usr/src/discord-ip-miner/target/release/discord-ip-miner \
	/usr/bin/discord-ip-miner

CMD ["/usr/bin/discord-ip-miner"]
