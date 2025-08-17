use redis::{Commands, Client, Connection};
use serde::{Deserialize, Serialize};
use regex::Regex;
use std::collections::HashSet;

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq, Hash)]
struct ConsensusIssue {
    line: u32,
    issue_type: String,
    description: String,
    recommendation: String,
}

#[derive(Serialize, Deserialize, Debug)]
struct AnalysisJob {
    job_id: String,
    source_code: String,
}

#[derive(Serialize, Deserialize, Debug)]
struct AnalysisResult {
    job_id: String,
    worker_name: String,
    output: Vec<ConsensusIssue>,
}

fn main() -> redis::RedisResult<()> {
    println!("Starting Consensus Compliance Worker [V3 FINAL - Unsafe Randomness]...");
    let redis_client = Client::open("redis://127.0.0.1/")?;
    let mut redis_con = redis_client.get_connection()?;
    println!("Successfully connected to Redis.");
    listen_for_jobs(&mut redis_con);
    Ok(())
}

fn listen_for_jobs(con: &mut Connection) {
    let channel = "consensus_jobs";
    println!("Listening for jobs on channel: '{}'", channel);
    loop {
        let job_data: Result<Vec<String>, _> = con.blpop(channel, 0);
        match job_data {
            Ok(data) => {
                let job_json = &data[1];
                let job: Result<AnalysisJob, _> = serde_json::from_str(job_json);
                match job {
                    Ok(parsed_job) => {
                        println!("\nProcessing Job ID: {}", parsed_job.job_id);
                        let result = analyze_consensus_safety_v3(&parsed_job);
                        publish_result(con, result);
                    }
                    Err(e) => eprintln!("Error parsing job JSON: {}", e),
                }
            }
            Err(e) => eprintln!("Error receiving job from Redis: {}", e),
        }
    }
}

fn analyze_consensus_safety_v3(job: &AnalysisJob) -> AnalysisResult {
    let mut issues: Vec<ConsensusIssue> = Vec::new();
    let code = &job.source_code;

    // V1 Regexes
    let commit_regex = Regex::new(r"function\s+(commit|register|submit)\s*\(\s*bytes32").unwrap();
    let reveal_regex = Regex::new(r"function\s+(reveal|claim|solve)\s*\(").unwrap();
    let block_number_regex = Regex::new(r"\bblock\.number\b").unwrap();
    
    // V2 Regexes
    let spot_price_regex = Regex::new(r"(?i)\.(getReserves|token0|token1|balanceOf)\s*\(\s*\)").unwrap();
    let is_price_feed_contract_regex = Regex::new(r"contract\s+[a-zA-Z0-9_]+\s+(?:is|implements)\s+(?:AggregatorV3Interface|Chainlink|PriceOracle)").unwrap();
    let critical_setter_regex = Regex::new(r"function\s+(set|change)(Admin|Owner|Pauser|Operator)\s*\(").unwrap();
    let critical_variable_regex = Regex::new(r"\b(admin|owner|pauser|operator)\b").unwrap();
    let time_lock_regex = Regex::new(r"\b(block\.timestamp|block\.number)\b.*\b(>=|>)\b").unwrap();
    
    // V3 Regex
    let unsafe_randomness_regex = Regex::new(r"\b(blockhash|block\.timestamp|block\.difficulty|block\.coinbase|block\.number)\b").unwrap();
    let keccak_regex = Regex::new(r"\bkeccak256\b").unwrap();

    // --- V1 Check: Unsafe Commit-Reveal ---
    let has_commit_function = commit_regex.is_match(code);
    let has_reveal_function = reveal_regex.is_match(code);
    if has_commit_function && has_reveal_function {
        let is_reorg_safe = block_number_regex.is_match(code);
        if !is_reorg_safe {
            let mut line_num = 0;
            for (i, line) in code.lines().enumerate() {
                if reveal_regex.is_match(line) { line_num = (i + 1) as u32; break; }
            }
            issues.push(ConsensusIssue {
                line: line_num,
                issue_type: "Reorg Safety Hazard (Implicit Finality Assumption)".to_string(),
                description: "A commit-reveal scheme was detected, but it does not appear to use `block.number` to enforce a delay between the commit and reveal phases.".to_string(),
                recommendation: "While safe on Avalanche due to fast finality, this pattern is vulnerable to reorgs on other chains. To ensure universal compatibility, use `block.number` to enforce a delay.".to_string(),
            });
        }
    }

    // --- V2 Check: Multi-Transaction State Dependency without Time-Lock ---
    let has_critical_setter = critical_setter_regex.is_match(code);
    let has_critical_variable_usage = critical_variable_regex.is_match(code);
    let has_time_lock = time_lock_regex.is_match(code);
    if has_critical_setter && has_critical_variable_usage && !has_time_lock {
        let mut line_num = 0;
        for (i, line) in code.lines().enumerate() {
            if critical_setter_regex.is_match(line) { line_num = (i + 1) as u32; break; }
        }
        issues.push(ConsensusIssue {
            line: line_num,
            issue_type: "Multi-Transaction Dependency Hazard".to_string(),
            description: "A critical state variable (e.g., owner, admin) can be set and immediately used without a time-lock. This is vulnerable to front-running and reorgs on slower-finality chains.".to_string(),
            recommendation: "Implement a time-lock or a two-step process for critical state changes. E.g., `proposeNewAdmin(address)` in one tx, `acceptAdmin()` in a later tx after a time delay (`block.timestamp + DELAY`).".to_string(),
        });
    }
    
    // --- Line-by-Line Analysis for V2 and V3 ---
    for (i, line_content) in code.lines().enumerate() {
        let line_num = (i + 1) as u32;

        // V2 Check: Spot Price Oracle Usage
        if spot_price_regex.is_match(line_content) && !is_price_feed_contract_regex.is_match(code) {
            issues.push(ConsensusIssue {
                line: line_num,
                issue_type: "Spot Price Oracle Hazard".to_string(),
                description: "Direct read of spot price from a DEX (e.g., `getReserves()`) detected. This is vulnerable to flash loan manipulation on slower-finality chains.".to_string(),
                recommendation: "Always use a Time-Weighted Average Price (TWAP) oracle or a decentralized oracle network (like Chainlink) for robust price feeds, especially when interacting with chains susceptible to reorgs.".to_string(),
            });
        }
        
        // V3 Check: Unsafe On-Chain Randomness
        if keccak_regex.is_match(line_content) && unsafe_randomness_regex.is_match(line_content) {
            issues.push(ConsensusIssue {
                line: line_num,
                issue_type: "Unsafe On-Chain Randomness".to_string(),
                description: "The contract appears to be generating a random number using a predictable or manipulatable on-chain value (e.g., block.timestamp, blockhash).".to_string(),
                recommendation: "Never use on-chain data for randomness in applications where value is at stake. This is a critical vulnerability. Use a secure off-chain solution like Chainlink VRF (Verifiable Random Function) to get provably fair random numbers.".to_string(),
            });
        }
    }
    
    println!("V3 analysis complete. Found {} consensus issues for Job ID: {}", issues.len(), job.job_id);

    let unique_issues: HashSet<ConsensusIssue> = issues.into_iter().collect();
    let output: Vec<ConsensusIssue> = unique_issues.into_iter().collect();

    AnalysisResult {
        job_id: job.job_id.clone(),
        worker_name: "ConsensusComplianceWorkerV3".to_string(),
        output,
    }
}

fn publish_result(con: &mut Connection, result: AnalysisResult) {
    let channel = "sentinel_results";
    match serde_json::to_string(&result) {
        Ok(result_json) => {
            println!("Publishing V3 result for Job ID: {}", result.job_id);
            if let Err(e) = con.rpush::<_, _, ()>(channel, result_json) {
                eprintln!("Failed to publish result to Redis: {}", e);
            }
        }
        Err(e) => eprintln!("Failed to serialize result to JSON: {}", e),
    }
}
