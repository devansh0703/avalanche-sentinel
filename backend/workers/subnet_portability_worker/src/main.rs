use redis::{Commands, Client, Connection};
use serde::{Deserialize, Serialize};
use regex::Regex;

#[derive(Serialize, Deserialize, Debug)]
struct AnalysisJob {
    job_id: String,
    source_code: String,
}

#[derive(Serialize, Deserialize, Debug)]
struct PortabilityIssue {
    line: u32,
    issue_type: String,
    description: String,
    recommendation: String,
}

#[derive(Serialize, Deserialize, Debug)]
struct AnalysisResult {
    job_id: String,
    worker_name: String,
    output: Vec<PortabilityIssue>,
}

// A list of well-known C-Chain contract addresses that won't exist on other Subnets.
const CCHAIN_ONLY_ADDRESSES: &[(&str, &str)] = &[
    ("0x9Ad6C38BE94206cA50bb0d90783181662f0Cfa10", "Trader Joe V1 Router"),
    ("0x60aE616a2155Ee3d9A68541Ba4544862310933d4", "Trader Joe V2 Router"),
    ("0xE54Ca86531e17Ef3616d22Ca28b0D458b6C89106", "Pangolin Router"),
    ("0xd00ae08403B959254dbA1188b832b412A4461b95", "Benqi Lending Market"),
];

fn main() -> redis::RedisResult<()> {
    println!("Starting Subnet Portability Worker...");

    let redis_client = Client::open("redis://127.0.0.1/")?;
    let mut redis_con = redis_client.get_connection()?;
    println!("Successfully connected to Redis.");

    listen_for_jobs(&mut redis_con);
    Ok(())
}

fn listen_for_jobs(con: &mut Connection) {
    let channel = "subnet_portability_jobs";
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
                        let result = analyze_portability(&parsed_job);
                        publish_result(con, result);
                    }
                    Err(e) => eprintln!("Error parsing job JSON: {}", e),
                }
            }
            Err(e) => eprintln!("Error receiving job from Redis: {}", e),
        }
    }
}

fn analyze_portability(job: &AnalysisJob) -> AnalysisResult {
    let mut issues: Vec<PortabilityIssue> = Vec::new();

    // Regex to find the standalone word `chainid`. `\b` is a word boundary.
    let chainid_regex = Regex::new(r"\bchainid\b").unwrap();

    // Iterate through each line of the source code to get line numbers.
    for (i, line_content) in job.source_code.lines().enumerate() {
        let line_num = (i + 1) as u32;

        // --- Check 1: Use of `chainid` opcode ---
        if chainid_regex.is_match(line_content) {
            issues.push(PortabilityIssue {
                line: line_num,
                issue_type: "Hardcoded Chain Assumption".to_string(),
                description: "The `chainid` opcode was used.".to_string(),
                recommendation: "Avoid using `chainid` for core logic. On a new Subnet, this value will be different and may break your contract.".to_string(),
            });
        }

        // --- Check 2: Hardcoded C-Chain Addresses ---
        for (address, name) in CCHAIN_ONLY_ADDRESSES {
            // Case-insensitive search for the address
            if line_content.to_lowercase().contains(&address.to_lowercase()) {
                 issues.push(PortabilityIssue {
                    line: line_num,
                    issue_type: "C-Chain Dependency".to_string(),
                    description: format!("A hardcoded address for a known C-Chain protocol ({}) was found.", name),
                    recommendation: "This contract will not exist on a new Subnet. Pass protocol addresses in the constructor or a setter function to make your contract portable.".to_string(),
                });
            }
        }
    }
    
    println!("Analysis complete. Found {} portability issues for Job ID: {}", issues.len(), job.job_id);

    AnalysisResult {
        job_id: job.job_id.clone(),
        worker_name: "SubnetPortabilityWorker".to_string(),
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
