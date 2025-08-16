use redis::{Commands, Client, Connection};
use serde::{Deserialize, Serialize};
use regex::Regex;

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
    println!("Starting Staking Precompile Worker [V2]...");

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
                        let result = analyze_staking_precompiles_v2(&parsed_job);
                        publish_result(con, result);
                    }
                    Err(e) => eprintln!("Error parsing job JSON: {}", e),
                }
            }
            Err(e) => eprintln!("Error receiving job from Redis: {}", e),
        }
    }
}

fn analyze_staking_precompiles_v2(job: &AnalysisJob) -> AnalysisResult {
    let mut issues: Vec<PrecompileIssue> = Vec::new();
    let code = &job.source_code;

    // Regex to find a function definition (name, visibility, modifiers)
    let function_regex = Regex::new(r"function\s+([a-zA-Z0-9_]+)\s*\((.*?)\)\s*(public|external|internal|private)\s*(.*?)\s*\{").unwrap();
    // Regex to find payable modifier
    let payable_modifier_regex = Regex::new(r"\bpayable\b").unwrap();
    // Regex to find low-level calls and their return value usage
    let low_level_call_regex = Regex::new(r"(?:([a-zA-Z0-9_]+)\s*,\s*)?\s*\)\s*=\s*(?:[a-zA-Z0-9_]+\.)?(call|delegatecall|staticcall)").unwrap();
    // Regex for common access control patterns
    let access_control_regex = Regex::new(r"\b(onlyOwner|onlyRole|require\(msg\.sender\s*==\s*[a-zA-Z0-9_]+\)|_checkRole)\b").unwrap();


    for (i, line_content) in job.source_code.lines().enumerate() {
        let line_num = (i + 1) as u32;

        for (address, name) in STAKING_PRECOMPILES {
            let regex_str = format!(r"(?i)\b{}\b", address);
            let re = Regex::new(&regex_str).unwrap();

            if re.is_match(line_content) {
                issues.push(PrecompileIssue {
                    line: line_num,
                    issue_type: "P-Chain Precompile Interaction".to_string(),
                    description: format!("Direct interaction with the {} precompile detected.", name),
                    recommendation: "This is a powerful, low-level operation. Review its correctness and security properties. Specific checks below.".to_string(),
                });

                // --- V2 CHECKS START HERE ---

                // Find the containing function
                // This is a simplified approach; a full AST would be more precise.
                // We'll scan backward to find the function definition.
                let mut current_func_start_line = 0;
                let mut current_func_signature = "";
                for j in (0..=i).rev() {
                    if let Some(func_match) = function_regex.captures(job.source_code.lines().nth(j).unwrap_or_default()) {
                        current_func_start_line = (j + 1) as u32;
                        current_func_signature = func_match.get(0).unwrap().as_str(); // The entire function line
                        break;
                    }
                }

                // V2 Check 1: Missing payable
                if !payable_modifier_regex.is_match(current_func_signature) {
                    issues.push(PrecompileIssue {
                        line: line_num, // Report on the line of the call, but attribute to func context
                        issue_type: "Missing Payable Modifier".to_string(),
                        description: format!("Interaction with a staking precompile requires `payable`, but the containing function ('{}') is not marked `payable`.", current_func_signature),
                        recommendation: "Ensure functions interacting with staking precompiles that send AVAX (e.g., delegate, addLiquidity) are marked `payable`.".to_string(),
                    });
                }

                // V2 Check 2: Unchecked Return Value (of low-level call)
                if low_level_call_regex.is_match(line_content) {
                    // Check if the low-level call is part of a `require` or variable assignment.
                    if !line_content.contains("require(") && !line_content.contains("=") {
                        issues.push(PrecompileIssue {
                            line: line_num,
                            issue_type: "Unchecked Return Value".to_string(),
                            description: "The return value of a low-level call to a precompile is not explicitly checked.".to_string(),
                            recommendation: "Always check the `success` boolean return value of low-level calls (`(bool success, bytes memory data) = addr.call(...)`). Use `require(success, \"Call failed\")` to prevent silent failures.".to_string(),
                        });
                    }
                }

                // V2 Check 3: Weak Access Control
                // Simplified: check if function is public/external AND lacks common access control.
                if current_func_signature.contains("public") || current_func_signature.contains("external") {
                    if !access_control_regex.is_match(current_func_signature) {
                        // Scan forward in the function body for internal access control
                        let mut has_internal_ac = false;
                        for k in i..job.source_code.lines().count() { // Scan till end of file or end of function (simplified)
                            let body_line = job.source_code.lines().nth(k).unwrap_or_default();
                            if body_line.contains("}") { break; } // Simple heuristic for end of function
                            if access_control_regex.is_match(body_line) {
                                has_internal_ac = true;
                                break;
                            }
                        }
                        if !has_internal_ac {
                            issues.push(PrecompileIssue {
                                line: line_num,
                                issue_type: "Weak Access Control".to_string(),
                                description: "A public/external function interacting with a staking precompile lacks explicit access control.".to_string(),
                                recommendation: "Functions that can alter staking state should be strictly controlled (e.g., `onlyOwner`, multi-sig, or DAO). Public access is a major security risk.".to_string(),
                            });
                        }
                    }
                }
                // --- END OF V2 CHECKS ---
            }
        }
    }
    
    println!("Analysis complete. Found {} precompile interactions for Job ID: {}", issues.len(), job.job_id);

    AnalysisResult {
        job_id: job.job_id.clone(),
        worker_name: "StakingPrecompileWorkerV2".to_string(),
        output: issues,
    }
}

fn publish_result(con: &mut Connection, result: AnalysisResult) {
    let channel = "sentinel_results";
    match serde_json::to_string(&result) {
        Ok(result_json) => {
            println!("Publishing V2 result for Job ID: {}", result.job_id);
            if let Err(e) = con.rpush::<_, _, ()>(channel, result_json) {
                eprintln!("Failed to publish result to Redis: {}", e);
            }
        }
        Err(e) => eprintln!("Failed to serialize result to JSON: {}", e),
    }
}
