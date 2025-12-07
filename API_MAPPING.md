# FlakeCache CLI to Server API Mapping

## Authentication

### OAuth Flow
```
1. CLI: POST /auth/{provider} (e.g., github)
   - Opens browser to: https://flakecache.com/auth/github
   - User authorizes, redirected to: http://localhost:8080/callback?code=...

2. CLI callback server receives code
   - Exchange code for token via server

3. Store token with expiry in config
   - Use Bearer token in all subsequent requests
   - Format: Authorization: Bearer {token}
```

## Download Operations (Nix Cache Protocol)

### Get Cache Metadata
```
GET /:cache/nix-cache-info

Returns:
  StoreDir: /nix/store
  WantMassQuery: 1
  Priority: 40
  PublicKeys: {key_name}:{public_key}  (if signing enabled)
```

### Get Store Path Info (.narinfo)
```
GET /:cache/{hash}.narinfo

Returns narinfo format:
  StorePath: /nix/store/{hash}-{name}
  URL: nar/{nar_hash}.nar.{compression}
  Compression: xz|zstd|gzip
  FileSize: {size}
  NarSize: {size}
  NarHash: sha256:{hash}
  References: {hash1} {hash2} ...
  Deriver: {hash}.drv
  Sig: {signature}  (if signed)
```

### Download NAR File
```
GET /:cache/nar/{file_hash}.nar.{compression}

Compression options: xz, zstd, gzip
Returns: Binary compressed NAR archive
```

## Upload Operations (API v1 - REST)

### Full Store Path Upload
```
PUT /api/v1/{cache}/upload

Multipart form data:
  - store_path: /nix/store/{hash}-{name}
  - nar_file: Binary compressed NAR
  - compression: xz|zstd|gzip
  - references: Space-separated hashes
  - deriver: Optional .drv path
  - system: Target system (x86_64-linux, etc.)

Returns:
  {
    "store_path": "/nix/store/...",
    "nar_hash": "sha256:...",
    "file_hash": "...",
    "file_size": 1234567,
    "bytes_uploaded": 1234567
  }
```

### Upload NAR Only
```
PUT /api/v1/{cache}/nar/{file_hash}/{compression}

Body: Binary compressed NAR

Returns: Confirmation with file hash
```

### Upload Narinfo Only
```
PUT /api/v1/{cache}/{file_hash}

Body: narinfo format text

Returns: Confirmation
```

## Upload Operations (API v2 - CBOR)

### Full Upload (CBOR)
```
POST /api/v2/cbor/{cache}/upload

Body: CBOR encoded
  {
    "store_path": "/nix/store/...",
    "nar_data": <binary>,
    "compression": "zstd",
    "references": ["hash1", "hash2"],
    "deriver": "hash.drv",
    "system": "x86_64-linux"
  }

Returns: CBOR encoded confirmation
```

### Async Upload
```
POST /api/v1/{cache}/upload/async

Same format as PUT /api/v1/{cache}/upload
Queue for background processing (returns job ID)
```

## Authentication Headers

All API requests (except OAuth) require:
```
Authorization: Bearer {access_token}
```

## Content Types

- **REST API**: `Content-Type: application/json`
- **CBOR API**: `Content-Type: application/cbor`
- **Cache Protocol**: `Content-Type: text/plain` or `text/x-nix-narinfo`

## Rate Limiting

- Upload API: 10 uploads per minute per IP/org
- General API: 5 requests per minute (varies by endpoint)
- Use `X-RateLimit-*` response headers

## Chunking Strategy

The server supports two upload methods:
1. **Full upload** - Single request with entire store path
2. **CBOR chunked** - /api/v2/cbor for better compression (3-5x faster, 30-50% smaller)

The CLI should use:
- `flakecache-chunker` for content-defined chunking on client side
- POST /api/v2/cbor for sending chunks (binary format, faster)
- Or PUT /api/v1 for traditional upload

## Error Codes

- 200 OK - Success
- 400 Bad Request - Invalid parameters
- 401 Unauthorized - Missing/invalid token
- 403 Forbidden - No write permission
- 404 Not Found - Cache or store path not found
- 429 Too Many Requests - Rate limited
- 503 Service Unavailable - Circuit breaker (temporary overload)

## Related Information

- Cache name format: slug-style (e.g., "main", "staging")
- Store paths: /nix/store/{hash}-{name}
- Hash format: base32 or hex depending on context
- Compression: Server supports xz, zstd, gzip
