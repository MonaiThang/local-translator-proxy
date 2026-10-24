# Keep model inference outside the Translation Proxy

The Translation Proxy will be a lightweight Rust loopback service that delegates inference to a separately installed, local Model Runtime. This preserves resources for the game, keeps the game-facing protocol stable while model choices evolve, and avoids embedding model weights and inference dependencies in the proxy.
