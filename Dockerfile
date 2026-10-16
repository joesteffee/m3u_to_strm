# syntax=docker/dockerfile:1

# Build. rust:*-alpine images target musl natively, so the binary is fully static: the runtime
# stage needs no libc, no OpenSSL and no CA bundle (rustls compiles the trust roots in).
# buildx runs this stage once per requested platform under emulation, which keeps the recipe free
# of cross-compilation toolchains.
FROM rust:1.99-alpine AS builder
WORKDIR /src
COPY Cargo.toml Cargo.lock ./
COPY src ./src
RUN cargo build --release --locked

# Stage the filesystem we want in the final image, with the library directories already owned by
# the container user so a mounted-but-empty volume still starts clean.
# /tmp is world-writable and sticky: scratch has no filesystem of its own, and the playlist cache
# lives there.
RUN mkdir -p /out/usr/local/bin /out/usr/src/app/movies /out/usr/src/app/series /out/usr/src/app/livetv \
    && mkdir -m 1777 /out/tmp \
    && cp target/release/m3u-to-strm /out/usr/local/bin/m3u-to-strm \
    && chown -R 99:100 /out/usr

# Run. Same paths, same user and same environment contract as the previous Python image, so
# existing mounts and templates keep working.
FROM scratch
COPY --from=builder /out /

ENV USER_ID=99 \
    GROUP_ID=100
USER 99:100
WORKDIR /usr/src/app

LABEL org.opencontainers.image.title="m3u_to_strm" \
      org.opencontainers.image.description="Convert an IPTV M3U playlist into STRM files for Emby/Jellyfin" \
      org.opencontainers.image.source="https://github.com/joesteffee/m3u_to_strm"

ENTRYPOINT ["/usr/local/bin/m3u-to-strm"]
