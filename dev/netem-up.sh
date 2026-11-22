#!/usr/bin/env bash
# A throwaway LiveKit server and token service for impairing the network in
# tests (#332): the server in its own container on its own docker network, so
# `tc netem` run in that container's namespace (it has NET_ADMIN of its own)
# delays and drops only its traffic, never the host's. The e2e test
# `the_voice_is_not_chopped_by_a_bad_network` points the apps at it with
# MORPHEUS_LIVEKIT_SERVICE. `dev/netem-up.sh down` removes it.
set -euo pipefail
if [ "${1:-}" = down ]; then
    docker rm -f morpheus-netem-lk morpheus-netem-jwt >/dev/null 2>&1 || true
    docker network rm morpheus-netem >/dev/null 2>&1 || true
    exit 0
fi
docker network inspect morpheus-netem >/dev/null 2>&1 || docker network create --subnet 172.29.77.0/24 morpheus-netem >/dev/null
docker inspect morpheus-netem-lk >/dev/null 2>&1 || docker run -d --name morpheus-netem-lk --net morpheus-netem --ip 172.29.77.2 --cap-add NET_ADMIN \
    livekit/livekit-server:latest --dev --bind 0.0.0.0 --node-ip 172.29.77.2 >/dev/null
docker inspect morpheus-netem-jwt >/dev/null 2>&1 || docker run -d --name morpheus-netem-jwt --network host \
    -e LIVEKIT_URL=ws://172.29.77.2:7880 -e LIVEKIT_KEY=devkey -e LIVEKIT_SECRET=secret -e LIVEKIT_JWT_BIND=:8082 \
    -e LIVEKIT_FULL_ACCESS_HOMESERVERS=localhost -e LIVEKIT_INSECURE_SKIP_VERIFY_TLS=YES_I_KNOW_WHAT_I_AM_DOING \
    ghcr.io/element-hq/lk-jwt-service:latest >/dev/null
echo "up: token service http://localhost:8082, server ws://172.29.77.2:7880"
