use redis::{Commands, Client, Connection};
use serde::{Deserialize, Serialize};
use regex::Regex;
use std::collections::HashSet; // V3 FIX: Import HashSet for deduplication

#[derive(Serialize, Deserialize, Debug)]
struct AnalysisJob {
    job_id: String,
    source_code: String,
}

// V3 FIX: Add traits for HashSet
#[derive(Serialize, Deserialize, Debug, PartialEq, Eq, Hash, Clone)]
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
    println!("Starting Staking Precompile Worker [V3]...");

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
                let job: Result<AnalysisJob, _> = serde_json::from_str(job_json);
                match job {
                    Ok(parsed_job) => {
                        println!("\nProcessing Job ID: {}", parsed_job.job_id);
                        let result = analyze_staking_precompiles_v3(&parsed_job);
                        publish_result(con, result);
                    }
                    Err(e) => eprintln!("Error parsing job JSON: {}", e),
                }
            }
            Err(e) => eprintln!("Error receiving job from Redis: {}", e),
        }
    }
}

fn analyze_staking_precompiles_v3(job: &AnalysisJob) -> AnalysisResult {
    let mut issues: Vec<PrecompileIssue> = Vec::new();
    let code = &job.source_code;

    let function_regex = Regex::new(r"function\s+([a-zA-Z0-9_]+)\s*\((.*?)\)\s*(public|external|internal|private)\s*(.*?)\s*\{").unwrap();
    let payable_modifier_regex = Regex::new(r"\bpayable\b").unwrap();
    let low_level_call_regex = Regex::new(r"\.call\b").unwrap();
    let access_control_regex = Regex::new(r"\b(onlyOwner|onlyRole|require\(msg\.sender\s*==\s*[a-zA-Z0-9_]+\)|_checkRole)\b").unwrap();
    let reward_withdrawal_regex = Regex::new(r"function\s+(withdraw|claim|distribute|release)Rewards\b").unwrap();
    let validator_id_regex = Regex::new(r"NodeID-[a-zA-Z0-9]+").unwrap();

    let mut interacts_with_staking = false;

    for (i, line_content) in code.lines().enumerate() {
        let line_num = (i + 1) as u32;

        for (address, name) in STAKING_PRECOMPILES {
            let regex_str = format!(r"(?i)\b{}\b", address);
            let re = Regex::new(&regex_str).unwrap();

            if re.is_match(line_content) {
                interacts_with_staking = true;
                
                issues.push(PrecompileIssue {
                    line: line_num,
                    issue_type: "P-Chain Precompile Interaction".to_string(),
                    description: format!("Direct interaction with the {} precompile detected.", name),
                    recommendation: "This is a powerful, low-level operation. Review its correctness and security properties. Specific checks below.".to_string(),
                });

                let mut current_func_start_line = 0;
                let mut current_func_signature = "";
                for j in (0..=i).rev() {
                    if let Some(func_match) = function_regex.captures(code.lines().nth(j).unwrap_or_default()) {
                        current_func_start_line = (j + 1) as u32;
                        current_func_signature = func_match.get(0).unwrap().as_str();
                        break;
                    }
                }

                if !payable_modifier_regex.is_match(current_func_signature) {
                    issues.push(PrecompileIssue { line: current_func_start_line, issue_type: "Missing Payable Modifier".to_string(), description: format!("The function interacting with a staking precompile is not marked `payable`."), recommendation: "Ensure functions that may send AVAX for staking/delegation are marked `payable`.".to_string()});
                }

                if low_level_call_regex.is_match(line_content) && !line_content.contains("require(") && !line_content.contains("=") {
                    issues.push(PrecompileIssue { line: line_num, issue_type: "Unchecked Return Value".to_string(), description: "The return value of a low-level call to a precompile is not checked.".to_string(), recommendation: "Always check the `success` boolean from low-level calls using `require(success, ...)` to prevent silent failures.".to_string()});
                }

                if (current_func_signature.contains("public") || current_func_signature.contains("external")) && !access_control_regex.is_match(current_func_signature) {
                     issues.push(PrecompileIssue { line: current_func_start_line, issue_type: "Weak Access Control".to_string(), description: "A public/external function interacting with a staking precompile lacks explicit access control.".to_string(), recommendation: "Functions that can alter staking state should be strictly controlled (e.g., `onlyOwner`).".to_string()});
                }
            }
        }
        
        if validator_id_regex.is_match(line_content) {
             issues.push(PrecompileIssue {
                line: line_num,
                issue_type: "Hardcoded Validator Dependency".to_string(),
                description: "A hardcoded validator NodeID was found.".to_string(),
                recommendation: "This creates a dependency on a single validator. Implement off-chain monitoring for this validator's health (uptime, fees, status) and have a contingency plan if it becomes unreliable or malicious.".to_string(),
            });
        }
    }

    if interacts_with_staking && !reward_withdrawal_regex.is_match(code) {
        issues.push(PrecompileIssue {
            line: 0,
            issue_type: "Locked Rewards Hazard".to_string(),
            description: "The contract interacts with staking precompiles but appears to lack a function for withdrawing or distributing staking rewards.".to_string(),
            recommendation: "Ensure your contract has a clear and secure mechanism (e.g., a `claimRewards()` or `distribute()` function) for users or administrators to access the staking rewards earned by the contract.".to_string(),
        });
    }
    
    println!("V3 analysis complete. Found {} precompile issues for Job ID: {}", issues.len(), job.job_id);
    
    // --- V3 FIX: Use HashSet for robust deduplication ---
    let unique_issues: HashSet<PrecompileIssue> = issues.into_iter().collect();
    let output: Vec<PrecompileIssue> = unique_issues.into_iter().collect();
    // --- END OF FIX ---

    AnalysisResult {
        job_id: job.job_id.clone(),
        worker_name: "StakingPrecompileWorkerV3".to_string(),
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
