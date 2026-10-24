# Keep the MVP loopback contract for v1

The Translation Proxy will listen only on `127.0.0.1:5000` and retain the MVP's `GET` query parameters and raw UTF-8 plain-text response. This avoids changing the game's integration while preventing LAN exposure; a versioned API remains a later concern.
