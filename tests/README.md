# Remote ADB Integration Tests

This directory contains comprehensive integration tests for the Remote ADB Logging System. The tests verify client-server communication, end-to-end workflows, and system reliability under various conditions.

## Test Structure

### Test Categories

1. **Integration Tests** (`integration_tests.rs`)
   - Client-server connection establishment
   - Device listing workflows
   - Authentication mechanisms
   - Multiple client connection handling

2. **End-to-End Tests** (`end_to_end_tests.rs`)
   - Complete device discovery workflows
   - Log monitoring and streaming
   - Error recovery and resilience testing
   - Log filtering and processing

3. **Performance Tests**
   - Connection performance benchmarks
   - Concurrent load handling
   - Memory usage monitoring
   - Resource utilization tests

4. **Test Infrastructure**
   - `test_runner.rs`: Test execution framework
   - `main.rs`: Test orchestrator
   - `test_config.toml`: Configuration file

## Running Tests

### Prerequisites

1. Ensure the Remote ADB system is built:
   ```bash
   cargo build --workspace
   ```

2. (Optional) Configure test parameters by editing `test_config.toml`

### Running All Tests

Run the complete integration test suite:

```bash
# From the workspace root
cargo run --bin integration-test-runner --manifest-path tests/Cargo.toml
```

### Running Specific Test Categories

Run individual test files:

```bash
# Integration tests only
cargo test --test integration_tests --manifest-path tests/Cargo.toml

# End-to-end tests only
cargo test --test end_to_end_tests --manifest-path tests/Cargo.toml

# Test runner tests
cargo test --test test_runner --manifest-path tests/Cargo.toml
```

### Running with Verbose Output

For detailed test output:

```bash
cargo test --test integration_tests --manifest-path tests/Cargo.toml -- --nocapture
```

## Test Configuration

The `test_config.toml` file allows customization of test behavior:

```toml
[test_environment]
default_test_timeout = 30        # Individual test timeout (seconds)
suite_timeout = 300             # Total suite timeout (seconds)
server_host = "127.0.0.1"       # Test server host
auth_token = "test-token"       # Authentication token

[test_categories]
integration_tests = true        # Enable integration tests
end_to_end_tests = true        # Enable end-to-end tests
performance_tests = true       # Enable performance tests
stress_tests = false           # Enable stress tests (disabled by default)

[performance_thresholds]
max_connection_time_ms = 5000   # Maximum acceptable connection time
max_concurrent_connections = 10 # Maximum concurrent connections to test
```

## Test Environment

### Mock Environment

The tests use a mock ADB environment that simulates:
- Multiple Android devices (emulators and physical devices)
- Log entry generation with various levels and tags
- Network conditions and error scenarios
- Authentication and authorization flows

### Test Isolation

Each test runs in isolation with:
- Separate server instances (using random ports)
- Independent client connections
- Isolated mock data
- Proper cleanup after each test

## Understanding Test Results

### Success Indicators

- ✓ All tests pass without errors
- Connection establishment works within timeout
- Device listing returns expected data structure
- Log streaming functions correctly
- Error recovery mechanisms work as expected

### Common Issues

1. **Timeout Errors**: May indicate slow system or network issues
   - Solution: Increase timeout values in `test_config.toml`

2. **Connection Failures**: Expected in mock environment
   - Tests verify error handling rather than actual connections

3. **Port Conflicts**: Multiple test runs may conflict
   - Solution: Tests use random ports to avoid conflicts

## Test Coverage

The integration tests cover:

### Functional Requirements
- ✅ Remote ADB connection establishment (Requirement 1)
- ✅ Real-time log stream acquisition (Requirement 2)
- ✅ Network configuration and security (Requirement 3)
- ✅ Connection management and monitoring (Requirement 4)
- ✅ Log processing and display (Requirement 5)
- ✅ Error handling and recovery (Requirement 6)

### Non-Functional Requirements
- ✅ Performance under load
- ✅ Memory usage monitoring
- ✅ Concurrent connection handling
- ✅ Error resilience
- ✅ Resource cleanup

## Extending Tests

### Adding New Tests

1. Create test functions in appropriate test files
2. Use the `TestExecutionContext` for timeout management
3. Follow the existing pattern for setup/teardown
4. Add configuration options to `test_config.toml` if needed

### Mock Data

Extend mock data in the test files:
- Add new device configurations
- Create additional log entry patterns
- Simulate different error conditions

### Performance Benchmarks

Add new performance tests by:
1. Defining thresholds in `test_config.toml`
2. Implementing measurement logic
3. Comparing results against thresholds

## Troubleshooting

### Debug Mode

Run tests with debug logging:

```bash
RUST_LOG=debug cargo test --test integration_tests --manifest-path tests/Cargo.toml
```

### Test Logs

Test execution logs are written to `tests/test_execution.log` (if configured).

### Common Solutions

1. **Build Errors**: Ensure all dependencies are built
   ```bash
   cargo build --workspace
   ```

2. **Missing Dependencies**: Update Cargo.toml files
   ```bash
   cargo update
   ```

3. **Port Issues**: Tests use random ports, but conflicts may occur
   - Restart tests if port conflicts persist

## CI/CD Integration

For continuous integration, run tests with:

```bash
# Non-interactive mode with timeout
timeout 600 cargo run --bin integration-test-runner --manifest-path tests/Cargo.toml
```

The test runner exits with:
- Code 0: All tests passed
- Code 1: Some tests failed

## Contributing

When adding new integration tests:

1. Follow the existing test structure
2. Use appropriate timeout values
3. Include both positive and negative test cases
4. Document any new configuration options
5. Ensure tests are deterministic and isolated