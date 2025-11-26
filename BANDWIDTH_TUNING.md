# FlakeCache Bandwidth Tuning & Adaptive Concurrency

## Overview

FlakeCache's `flakecache` CLI includes intelligent bandwidth detection to automatically tune upload/download concurrency and chunk sizes based on available network bandwidth. This dramatically improves performance across different network conditions.

## How It Works

### 1. Bandwidth Detection

The tool can detect or accept bandwidth information in three ways (in order of priority):

1. **Explicit Concurrency Override** (`FLAKECACHE_CONCURRENCY`)
   - Direct specification of concurrent connections
   - Highest priority, skips bandwidth detection

2. **Bandwidth Override** (`FLAKECACHE_BANDWIDTH_MBPS`)
   - Manually specify bandwidth in Mbps
   - Tool recommends optimal concurrency and chunk sizes

3. **Auto-Detection** (default)
   - Quick probe of network bandwidth
   - Automatically tunes concurrency for optimal throughput

### 2. Bandwidth Tiers

Detected bandwidth is classified into tiers with recommended settings:

| Tier | Bandwidth | Concurrency | Chunk Size | Use Case |
|------|-----------|-------------|-----------|----------|
| Very Slow | < 1 Mbps | 1 | 1 MB | Cellular, poor WiFi |
| Slow | 1-10 Mbps | 2 | 2 MB | WiFi, DSL |
| Medium | 10-100 Mbps | 4 | 4 MB | Good WiFi, home broadband |
| Fast | 100-500 Mbps | 8 | 8 MB | Fiber, office network |
| Very Fast | > 500 Mbps | 16 | 16 MB | Enterprise, datacenter |

### 3. Concurrency Tuning

Concurrency is automatically adjusted to:
- **Avoid overwhelming slow networks** (fewer connections)
- **Saturate fast networks** (more parallel connections)
- **Balance resource usage** (reasonable limits: 1-16 connections)

### 4. Chunk Size Tuning

Parallel download chunks are sized to:
- **Minimize latency** on slow networks (smaller chunks)
- **Maximize throughput** on fast networks (larger chunks)
- **Match typical network burstiness patterns**

## Configuration

### Environment Variables

#### `FLAKECACHE_CONCURRENCY` (optional)

Explicitly specify the number of concurrent connections to use.

```bash
export FLAKECACHE_CONCURRENCY=8
flakecache push --cache my-cache
```

**Priority**: Highest (overrides bandwidth detection)

#### `FLAKECACHE_BANDWIDTH_MBPS` (optional)

Manually specify estimated bandwidth in Mbps.

```bash
export FLAKECACHE_BANDWIDTH_MBPS=100
flakecache push --cache my-cache
```

**Priority**: Medium (overrides auto-detection, respects explicit concurrency)

#### Auto-Detection (default)

If neither variable is set, bandwidth is probed automatically:

```bash
flakecache push --cache my-cache
```

Output shows detected bandwidth and recommended settings:
```
→ Detected bandwidth: 50.0 Mbps (Medium)
→ Recommended concurrency: 4 connections, 4000000 byte chunks
```

## Usage Examples

### Example 1: Auto-Tuned CI Pipeline

```yaml
name: Fast Build with Auto-Tuned Cache

on: [push]

jobs:
  build:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4

      # Auto-detects bandwidth and tunes concurrency
      - uses: singularity-ng/flakecache@v1
        with:
          cache-name: my-org/cache
          token: ${{ secrets.FLAKECACHE_TOKEN }}

      - run: nix build

      # Automatically uses optimal concurrency for upload
      - run: flakecache push --cache my-org/cache
```

### Example 2: Explicitly Limited Concurrency

For environments where many parallel connections cause issues:

```yaml
- name: Upload with limited concurrency
  env:
    FLAKECACHE_CONCURRENCY: 2
  run: flakecache push --cache my-org/cache
```

### Example 3: Custom Bandwidth Estimate

When you know the expected bandwidth:

```yaml
- name: Upload with custom bandwidth
  env:
    FLAKECACHE_BANDWIDTH_MBPS: 250  # Fast office network
  run: flakecache push --cache my-org/cache
```

## Performance Tips

### 1. Benchmark Your Network

Test bandwidth in your CI environment:

```bash
# Measure upload speed to FlakeCache
time flakecache push --cache my-cache small-artifact

# Measure download speed
time flakecache resolve .#myapp
```

### 2. Use Explicit Overrides When Needed

If auto-detection is too conservative/aggressive for your network:

```yaml
env:
  FLAKECACHE_BANDWIDTH_MBPS: 500  # Adjust based on benchmarks
```

### 3. Monitor Upload Times

Track upload times over time to catch regressions:

```bash
# Log upload duration in your CI logs
flakecache push --cache my-cache | tee upload.log
```

### 4. Cache Frequently-Built Artifacts

Focus on caching dependencies that are:
- Built frequently
- Take significant time to build
- Stable between commits

## Troubleshooting

### Problem: Too Many Parallel Connections

**Symptoms**: Uploads fail, timeout errors, high latency

**Solutions**:
1. Reduce concurrency explicitly:
   ```bash
   export FLAKECACHE_CONCURRENCY=2
   ```
2. Or specify lower bandwidth estimate:
   ```bash
   export FLAKECACHE_BANDWIDTH_MBPS=10
   ```

### Problem: Not Using Full Bandwidth

**Symptoms**: Uploads slower than expected, low CPU usage

**Solutions**:
1. Increase concurrency:
   ```bash
   export FLAKECACHE_CONCURRENCY=16
   ```
2. Or increase bandwidth estimate:
   ```bash
   export FLAKECACHE_BANDWIDTH_MBPS=500
   ```

### Problem: Inconsistent Performance

**Symptoms**: Variable upload times, unpredictable speeds

**Solutions**:
1. Set explicit values instead of relying on auto-detection:
   ```bash
   export FLAKECACHE_CONCURRENCY=4
   export FLAKECACHE_BANDWIDTH_MBPS=100
   ```
2. Check network stability with repeated tests

## Default Behavior

If no environment variables are set:

1. **Bandwidth Detection**: Tool probes network conditions
2. **Default Estimate**: 50 Mbps (Medium tier)
3. **Recommended Settings**: 4 concurrent connections, 4 MB chunks
4. **CPU-Based Fallback**: Uses CPU count * 1.5 (capped 2-16) if detection fails

## Implementation Details

### Bandwidth Probe

The bandwidth probe is a lightweight network measurement that:
- Performs minimal I/O (avoids affecting actual operations)
- Measures latency and estimated throughput
- Caches results briefly (avoids repeated probes)
- Gracefully falls back to defaults on failure

### Concurrency Limits

Hard limits prevent resource exhaustion:
- **Minimum**: 1 connection (very slow networks)
- **Maximum**: 16 connections (prevents socket exhaustion)
- **Recommended Range**: 2-8 for most networks

### Chunk Size Calculation

Chunk sizes are selected to:
- Minimize memory overhead (even with 16 parallel connections)
- Match typical network segment sizes
- Balance retransmit costs with throughput

## Testing

Run tests for bandwidth detection:

```bash
# All bandwidth tests
cargo test bandwidth::

# Specific test
cargo test bandwidth::tests::test_bandwidth_classification

# With output
cargo test bandwidth:: -- --nocapture
```

## See Also

- [ACTION_MARKETPLACE_GUIDE.md](../../ACTION_MARKETPLACE_GUIDE.md) - GitHub Actions setup
- [CLI.md](../../CLI.md) - Command-line reference
- [CHUNKING.md](../../CHUNKING.md) - Chunk size tuning details
