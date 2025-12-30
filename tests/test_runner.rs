//! Test Runner for Integration Tests
//! 
//! This module provides utilities for running integration tests with proper
//! setup and teardown, including test environment management.

use std::time::Duration;
use tokio::time::timeout;
use endorphin_common::{Result, RemoteAdbError, init_global_status_monitor};

/// Test execution context that manages test lifecycle
/// 
/// Tracks test timing and provides utilities for timeout detection.
/// This struct is part of the test infrastructure and its fields/methods
/// are used by the test framework and individual tests.
#[allow(dead_code)]
pub struct TestExecutionContext {
    test_name: String,
    start_time: std::time::Instant,
    timeout_duration: Duration,
}

impl TestExecutionContext {
    pub fn new(test_name: &str, timeout_duration: Duration) -> Self {
        Self {
            test_name: test_name.to_string(),
            start_time: std::time::Instant::now(),
            timeout_duration,
        }
    }

    /// Get the elapsed time since test started
    #[allow(dead_code)]
    pub fn elapsed(&self) -> Duration {
        self.start_time.elapsed()
    }

    /// Get the remaining time before timeout
    #[allow(dead_code)]
    pub fn remaining_time(&self) -> Duration {
        self.timeout_duration.saturating_sub(self.elapsed())
    }

    /// Check if the test has exceeded its timeout
    #[allow(dead_code)]
    pub fn is_timeout(&self) -> bool {
        self.elapsed() >= self.timeout_duration
    }

    /// Get the name of the test
    #[allow(dead_code)]
    pub fn test_name(&self) -> &str {
        &self.test_name
    }
}

/// Test result summary
#[derive(Debug, Clone)]
pub struct TestResult {
    pub test_name: String,
    pub success: bool,
    pub duration: Duration,
    pub error_message: Option<String>,
}

impl TestResult {
    pub fn success(test_name: String, duration: Duration) -> Self {
        Self {
            test_name,
            success: true,
            duration,
            error_message: None,
        }
    }

    pub fn failure(test_name: String, duration: Duration, error: String) -> Self {
        Self {
            test_name,
            success: false,
            duration,
            error_message: Some(error),
        }
    }
}

/// Test suite runner that manages multiple test executions
pub struct TestSuiteRunner {
    results: Vec<TestResult>,
    total_timeout: Duration,
}

impl TestSuiteRunner {
    pub fn new(total_timeout: Duration) -> Self {
        Self {
            results: Vec::new(),
            total_timeout,
        }
    }

    /// Run a single test with timeout and error handling
    pub async fn run_test<F, Fut>(&mut self, test_name: &str, test_fn: F) -> Result<()>
    where
        F: FnOnce(TestExecutionContext) -> Fut,
        Fut: std::future::Future<Output = Result<()>>,
    {
        let context = TestExecutionContext::new(test_name, self.total_timeout);
        let start_time = std::time::Instant::now();

        println!("Running test: {}", test_name);

        let result = timeout(self.total_timeout, test_fn(context)).await;
        let duration = start_time.elapsed();

        match result {
            Ok(Ok(())) => {
                println!("✓ {} completed successfully in {:?}", test_name, duration);
                self.results.push(TestResult::success(test_name.to_string(), duration));
                Ok(())
            }
            Ok(Err(e)) => {
                println!("✗ {} failed: {} (duration: {:?})", test_name, e, duration);
                self.results.push(TestResult::failure(test_name.to_string(), duration, e.to_string()));
                Err(e)
            }
            Err(_) => {
                let error_msg = format!("Test timed out after {:?}", self.total_timeout);
                println!("✗ {} timed out after {:?}", test_name, self.total_timeout);
                self.results.push(TestResult::failure(test_name.to_string(), duration, error_msg.clone()));
                Err(RemoteAdbError::client(error_msg))
            }
        }
    }

    /// Get test results summary
    #[allow(dead_code)]
    pub fn get_results(&self) -> &[TestResult] {
        &self.results
    }

    /// Print test summary
    pub fn print_summary(&self) {
        let total_tests = self.results.len();
        let successful_tests = self.results.iter().filter(|r| r.success).count();
        let failed_tests = total_tests - successful_tests;

        println!("\n{}", "=".repeat(60));
        println!("Test Summary");
        println!("{}", "=".repeat(60));
        println!("Total tests: {}", total_tests);
        println!("Successful: {}", successful_tests);
        println!("Failed: {}", failed_tests);

        if failed_tests > 0 {
            println!("\nFailed tests:");
            for result in &self.results {
                if !result.success {
                    println!("  ✗ {} - {}", result.test_name, 
                            result.error_message.as_deref().unwrap_or("Unknown error"));
                }
            }
        }

        let total_duration: Duration = self.results.iter().map(|r| r.duration).sum();
        println!("Total execution time: {:?}", total_duration);
        println!("{}", "=".repeat(60));
    }

    /// Check if all tests passed
    pub fn all_tests_passed(&self) -> bool {
        self.results.iter().all(|r| r.success)
    }
}

/// Initialize test environment with proper logging and monitoring
pub async fn initialize_test_environment() -> Result<()> {
    // Initialize global status monitoring
    init_global_status_monitor(Duration::from_secs(60));
    
    // Initialize logging for tests
    endorphin_common::logging::init_logging_with_level("debug")
        .map_err(|e| RemoteAdbError::client(format!("Failed to initialize logging: {}", e)))?;

    // Give systems time to initialize
    tokio::time::sleep(Duration::from_millis(100)).await;

    Ok(())
}

/// Cleanup test environment
pub async fn cleanup_test_environment() -> Result<()> {
    // Allow time for cleanup
    tokio::time::sleep(Duration::from_millis(100)).await;
    Ok(())
}

#[cfg(test)]
mod test_runner_tests {
    use super::*;

    #[tokio::test]
    async fn test_runner_basic_functionality() {
        let mut runner = TestSuiteRunner::new(Duration::from_secs(5));

        // Test successful test
        let result = runner.run_test("test_success", |_ctx| async {
            tokio::time::sleep(Duration::from_millis(10)).await;
            Ok(())
        }).await;

        assert!(result.is_ok(), "Successful test should return Ok");

        // Test failing test
        let result = runner.run_test("test_failure", |_ctx| async {
            Err(RemoteAdbError::client("Test failure".to_string()))
        }).await;

        assert!(result.is_err(), "Failing test should return Err");

        // Check results
        let results = runner.get_results();
        assert_eq!(results.len(), 2, "Should have 2 test results");
        assert!(results[0].success, "First test should be successful");
        assert!(!results[1].success, "Second test should fail");
    }

    #[tokio::test]
    async fn test_runner_timeout_handling() {
        let mut runner = TestSuiteRunner::new(Duration::from_millis(100));

        // Test that times out
        let result = runner.run_test("test_timeout", |_ctx| async {
            tokio::time::sleep(Duration::from_millis(200)).await;
            Ok(())
        }).await;

        assert!(result.is_err(), "Timeout test should return Err");

        let results = runner.get_results();
        assert_eq!(results.len(), 1, "Should have 1 test result");
        assert!(!results[0].success, "Timeout test should fail");
        assert!(results[0].error_message.as_ref().unwrap().contains("timed out"), 
               "Error message should mention timeout");
    }

    #[tokio::test]
    async fn test_execution_context() {
        let ctx = TestExecutionContext::new("test_context", Duration::from_secs(1));
        
        assert_eq!(ctx.test_name(), "test_context");
        assert!(!ctx.is_timeout(), "Should not be timeout initially");
        
        tokio::time::sleep(Duration::from_millis(10)).await;
        
        assert!(ctx.elapsed() >= Duration::from_millis(10), "Should track elapsed time");
        assert!(ctx.remaining_time() < Duration::from_secs(1), "Should calculate remaining time");
    }

    #[tokio::test]
    async fn test_environment_initialization() {
        let result = initialize_test_environment().await;
        assert!(result.is_ok(), "Test environment initialization should succeed");

        let cleanup_result = cleanup_test_environment().await;
        assert!(cleanup_result.is_ok(), "Test environment cleanup should succeed");
    }
}