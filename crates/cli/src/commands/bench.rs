use agent_terminal_core::{ipc::IpcClient, lock::LockFile};
use anyhow::{bail, Context, Result};
use std::time::{Duration, Instant};
use tokio::time::sleep;

const CYAN: &str = "\x1b[36m";
const YELLOW: &str = "\x1b[33m";
const GREEN: &str = "\x1b[32m";
const RED: &str = "\x1b[31m";
const MAGENTA: &str = "\x1b[35m";
const RESET: &str = "\x1b[0m";

/// Benchmark result for a single operation
#[derive(Debug)]
struct BenchmarkResult {
    name: String,
    duration_ms: u128,
    success: bool,
}

pub async fn run(session_id: &str) -> Result<()> {
    let lock = match LockFile::find_active(session_id) {
        Some(l) => l,
        None => bail!("no active session found with id/prefix: {}", session_id),
    };

    let mut client = IpcClient::connect(&lock.socket_path)
        .await
        .context("connect to session")?;

    println!("{}╔════════════════════════════════════════════════════════════╗{}", CYAN, RESET);
    println!("{}║       Performance Benchmark for Terminal Rendering       ║{}", CYAN, RESET);
    println!("{}╚════════════════════════════════════════════════════════════╝{}", CYAN, RESET);
    println!();

    let mut results = Vec::new();

    // Benchmark 1: Simple echo command
    results.push(benchmark_echo(&mut client).await);

    // Benchmark 2: Vim startup time
    results.push(benchmark_vim_startup(&mut client).await?);

    // Benchmark 3: Screen capture latency
    results.push(benchmark_screen_capture(&mut client).await);

    // Benchmark 4: Input write latency
    results.push(benchmark_input_write(&mut client).await);

    // Print results
    print_results(&results);

    Ok(())
}

async fn benchmark_echo(client: &mut IpcClient) -> BenchmarkResult {
    let test_string = "ECHO_TEST_12345";
    let start = Instant::now();

    // Send echo command
    let result = async {
        client.write_input(&format!("echo {}", test_string)).await.ok()?;
        client.write_input("\n").await.ok()?;

        // Wait for output with timeout
        let deadline = Instant::now() + Duration::from_secs(2);
        while Instant::now() < deadline {
            let (_raw, screen) = client.get_output().await.ok()?;
            if screen.contains(test_string) {
                return Some(());
            }
            sleep(Duration::from_millis(50)).await;
        }
        None
    }
    .await;

    let duration = start.elapsed();

    BenchmarkResult {
        name: "Echo command latency".to_string(),
        duration_ms: duration.as_millis(),
        success: result.is_some(),
    }
}

async fn benchmark_vim_startup(client: &mut IpcClient) -> Result<BenchmarkResult> {
    // Create a test file
    let test_content = "Line 1: Benchmark test\nLine 2: Second line\nLine 3: Third line";
    std::fs::write("/tmp/bench_vim.txt", test_content)?;

    let start = Instant::now();

    // Start vim
    client.write_input("vim /tmp/bench_vim.txt").await?;
    client.write_input("\n").await?;

    // Wait for vim to fully render
    let deadline = Instant::now() + Duration::from_secs(5);
    let mut success = false;

    while Instant::now() < deadline {
        let (_raw, screen) = client.get_output().await?;
        // Check for vim indicators
        if screen.contains("~") && screen.contains("Line 1") {
            success = true;
            break;
        }
        sleep(Duration::from_millis(100)).await;
    }

    let duration = start.elapsed();

    // Exit vim
    sleep(Duration::from_millis(500)).await;
    client.write_input("\x1b:q!\n").await.ok();
    sleep(Duration::from_millis(500)).await;

    Ok(BenchmarkResult {
        name: "Vim startup time (full render)".to_string(),
        duration_ms: duration.as_millis(),
        success,
    })
}

async fn benchmark_screen_capture(client: &mut IpcClient) -> BenchmarkResult {
    let iterations = 10;
    let start = Instant::now();

    for _ in 0..iterations {
        let _ = client.get_output().await;
    }

    let total_duration = start.elapsed();
    let avg_duration = total_duration / iterations;

    BenchmarkResult {
        name: format!("Screen capture latency (avg of {})", iterations),
        duration_ms: avg_duration.as_millis(),
        success: true,
    }
}

async fn benchmark_input_write(client: &mut IpcClient) -> BenchmarkResult {
    let iterations = 20;
    let start = Instant::now();

    for i in 0..iterations {
        let result = client.write_input(&format!("{}", i)).await;
        if result.is_err() {
            return BenchmarkResult {
                name: format!("Input write latency (failed at iteration {})", i),
                duration_ms: start.elapsed().as_millis(),
                success: false,
            };
        }
    }

    let total_duration = start.elapsed();
    let avg_duration = total_duration / iterations;

    BenchmarkResult {
        name: format!("Input write latency (avg of {})", iterations),
        duration_ms: avg_duration.as_millis(),
        success: true,
    }
}

fn print_results(results: &[BenchmarkResult]) {
    println!("\n{}Benchmark Results:{}", CYAN, RESET);
    println!("{}", "─".repeat(60));

    for result in results {
        let status = if result.success {
            format!("{}✓{}", GREEN, RESET)
        } else {
            format!("{}✗{}", RED, RESET)
        };

        let color = if result.duration_ms < 100 {
            GREEN
        } else if result.duration_ms < 500 {
            YELLOW
        } else {
            RED
        };

        println!(
            "{} {:<40} {}{:>6} ms{}",
            status, result.name, color, result.duration_ms, RESET
        );
    }

    println!("{}", "─".repeat(60));
    println!();

    // Performance summary
    let total: u128 = results.iter().map(|r| r.duration_ms).sum();
    let avg = total / results.len() as u128;

    println!("{}Performance Summary:{}", MAGENTA, RESET);
    println!("  Total time: {} ms", total);
    println!("  Average:    {} ms", avg);

    if avg < 200 {
        println!("  {}✓ Excellent performance{}", GREEN, RESET);
    } else if avg < 500 {
        println!("  {}~ Good performance{}", YELLOW, RESET);
    } else {
        println!("  {}! Performance needs improvement{}", RED, RESET);
    }

    println!();
    println!("{}Notes:{}", YELLOW, RESET);
    println!("  - Echo latency: Time for command to appear on screen");
    println!("  - Vim startup: Time for vim to fully render UI");
    println!("  - Screen capture: Time to fetch current screen state");
    println!("  - Input write: Time to send input to session");
}
