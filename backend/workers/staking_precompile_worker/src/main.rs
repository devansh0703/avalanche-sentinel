use redis::{Commands, Client, Connection};
use serde::{Deserialize, Serialize};
use regex::Regex; // We will now USE this import.

#[derive(Serialize, Deserialize, Debug)]
struct AnalysisJob {
    job_id: String,
    source_code: String,
}

#[derive(Serialize, Deserialize, Debug)]
struct PrecompileIssue {
    line: u32,
    issue_type: String,
    description: String,
    recommendation: String,
}

#[derive(Serialize, Deserialize, Debug)]
struct AnalysisResult {
    job_id: String,
    worker_name: String,
    output: Vec<PrecompileIssue>,
}

const STAKING_PRECOMPILES: &[(&str, &str)] = &[
    ("0x0100000000000000000000000000000000000000", "P-Chain Handler"),
];

fn main() -> redis::RedisResult<()> {
    println!("Starting Staking Precompile Worker...");

    let redis_client = Client::open("redis://127.0.0.1/")?;
    let mut redis_con = redis_client.get_connection()?;
    println!("Successfully connected to Redis.");

    listen_for_jobs(&mut redis_con);
    Ok(())
}

fn listen_for_jobs(con: &mut Connection) {
    let channel = "staking_precompile_jobs";
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
                        let result = analyze_staking_precompiles(&parsed_job);
                        publish_result(con, result);
                    }
                    Err(e) => eprintln!("Error parsing job JSON: {}", e),
                }
            }
            Err(e) => eprintln!("Error receiving job from Redis: {}", e),
        }
    }
}

fn analyze_staking_precompiles(job: &AnalysisJob) -> AnalysisResult {
    let mut issues: Vec<PrecompileIssue> = Vec::new();

    for (i, line_content) in job.source_code.lines().enumerate() {
        let line_num = (i + 1) as u32;

        for (address, name) in STAKING_PRECOMPILES {
            // --- THE FIX ---
            // We now build a specific, case-insensitive regex for the address.
            // The `\b` are "word boundaries" which prevent matching substrings.
            // For example, it will match `0x0100...` but not `1230x0100...`
            let regex_str = format!(r"(?i)\b{}\b", address);
            let re = Regex::new(&regex_str).unwrap();

            // Use the compiled regex to check the line.
            if re.is_match(line_content) {
                 issues.push(PrecompileIssue {
                    line: line_num,
                    issue_type: "P-Chain Precompile Interaction".to_string(),
                    description: format!("Direct interaction with the {} precompile detected.", name),
                    recommendation: "This is a powerful, low-level operation. Ensure all arguments are correctly formatted and that the function is protected against re-entrancy, as it involves cross-chain state.".to_string(),
                });
            }
        }
    }
    
    println!("Analysis complete. Found {} precompile interactions for Job ID: {}", issues.len(), job.job_id);

    AnalysisResult {
        job_id: job.job_id.clone(),
        worker_name: "StakingPrecompileWorker".to_string(),
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
