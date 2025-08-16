use redis::{Commands, Client, Connection};
use serde::{Deserialize, Serialize};
use regex::Regex;

#[derive(Serialize, Deserialize, Debug)]
struct AnalysisJob {
    job_id: String,
    source_code: String,
}

#[derive(Serialize, Deserialize, Debug)]
struct ConsensusIssue {
    line: u32,
    issue_type: String,
    description: String,
    recommendation: String,
}

#[derive(Serialize, Deserialize, Debug)]
struct AnalysisResult {
    job_id: String,
    worker_name: String,
    output: Vec<ConsensusIssue>,
}

fn main() -> redis::RedisResult<()> {
    println!("Starting Consensus Compliance Worker...");

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
                println!("\nReceived new job.");
                let job: Result<AnalysisJob, _> = serde_json::from_str(job_json);
                match job {
                    Ok(parsed_job) => {
                        println!("Processing Job ID: {}", parsed_job.job_id);
                        let result = analyze_consensus_safety(&parsed_job);
                        publish_result(con, result);
                    }
                    Err(e) => eprintln!("Error parsing job JSON: {}", e),
                }
            }
            Err(e) => eprintln!("Error receiving job from Redis: {}", e),
        }
    }
}

fn analyze_consensus_safety(job: &AnalysisJob) -> AnalysisResult {
    let mut issues: Vec<ConsensusIssue> = Vec::new();
    let code = &job.source_code;

    // Heuristics to detect a commit-reveal scheme
    // 1. Look for a function that seems to "commit" something (takes a bytes32 hash)
    let commit_regex = Regex::new(r"function\s+(commit|register|submit)\s*\(\s*bytes32").unwrap();
    // 2. Look for a function that seems to "reveal" something (takes multiple arguments)
    let reveal_regex = Regex::new(r"function\s+(reveal|claim|solve)\s*\(").unwrap();
    // 3. Look for the critical safety check: usage of `block.number`
    let block_number_regex = Regex::new(r"\bblock\.number\b").unwrap();
    
    let has_commit_function = commit_regex.is_match(code);
    let has_reveal_function = reveal_regex.is_match(code);

    if has_commit_function && has_reveal_function {
        // We have likely found a commit-reveal scheme. Now, check if it's safe.
        // A safe implementation MUST use block.number to enforce a delay between commit and reveal.
        let is_reorg_safe = block_number_regex.is_match(code);

        if !is_reorg_safe {
            // Find the line number of the reveal function to report it
            let mut line_num = 0;
            for (i, line) in code.lines().enumerate() {
                if reveal_regex.is_match(line) {
                    line_num = (i + 1) as u32;
                    break;
                }
            }

            issues.push(ConsensusIssue {
                line: line_num,
                issue_type: "Reorg Safety Hazard (Implicit Finality Assumption)".to_string(),
                description: "A commit-reveal scheme was detected, but it does not appear to use `block.number` to enforce a delay between the commit and reveal phases.".to_string(),
                recommendation: "While safe on Avalanche due to fast finality, this pattern is vulnerable to reorgs on other chains. To ensure universal compatibility, store the commit block number (`commitBlock[hash] = block.number;`) and add a requirement in the reveal function (`require(block.number > commitBlock[hash] + DELAY, ...)`).".to_string(),
            });
        }
    }
    
    println!("Analysis complete. Found {} consensus issues for Job ID: {}", issues.len(), job.job_id);

    AnalysisResult {
        job_id: job.job_id.clone(),
        worker_name: "ConsensusComplianceWorker".to_string(),
        output: issues,
    }
}

fn publish_result(con: &mut Connection, result: AnalysisResult) {
    let channel = "sentinel_results";
    match serde_json::to_string(&result) {
        Ok(result_json) => {
            println!("Publishing result for Job ID: {}", result.job_id);
            if let Err(e) = con.rpush::<_, _, ()>(channel, result_json) {
                eprintln!("Failed to publish result to Redis: {}", e);
            }
        }
        Err(e) => eprintln!("Failed to serialize result to JSON: {}", e),
    }
}
