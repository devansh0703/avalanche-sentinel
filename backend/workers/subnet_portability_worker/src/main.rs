use redis::{Commands, Client, Connection};
use serde::{Deserialize, Serialize};
use regex::Regex;
use serde_json::Value;
use std::collections::HashSet; // V3 FIX: Import HashSet for deduplication

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq, Hash)] // V3 FIX: Add traits for HashSet
struct PortabilityIssue {
    line: u32,
    issue_type: String,
    description: String,
    recommendation: String,
}

// --- V3: Structs for parsing the subnet genesis file (Unchanged) ---
#[derive(Serialize, Deserialize, Debug)]
#[serde(rename_all = "camelCase")]
struct FeeConfig {
    gas_limit: Option<u64>,
}
#[derive(Serialize, Deserialize, Debug)]
#[serde(rename_all = "camelCase")]
struct ChainConfig {
    fee_config: FeeConfig,
    precompile_validator_allow_list: Option<serde_json::Map<String, Value>>,
}
#[derive(Serialize, Deserialize, Debug)]
#[serde(rename_all = "camelCase")]
struct Genesis {
    config: ChainConfig,
}

// --- V3: The job payload (Unchanged) ---
#[derive(Serialize, Deserialize, Debug)]
struct AnalysisJob {
    job_id: String,
    source_code: String,
    subnet_genesis: Option<Genesis>,
}

#[derive(Serialize, Deserialize, Debug)]
struct AnalysisResult {
    job_id: String,
    worker_name: String,
    output: Vec<PortabilityIssue>,
}

const CCHAIN_ONLY_ADDRESSES: &[(&str, &str)] = &[
    ("0x9Ad6C38BE94206cA50bb0d90783181662f0Cfa10", "Trader Joe V1 Router"),
    ("0x60aE616a2155Ee3d9A68541Ba4544862310933d4", "Trader Joe V2 Router"),
    ("0xE54Ca86531e17Ef3616d22Ca28b0D458b6C89106", "Pangolin Router"),
    ("0xd00ae08403B959254dbA1188b832b412A4461b95", "Benqi Lending Market (qiAVAX)"),
    ("0x2b2C81e08f1Af8835a78Bb2A90AE924ACE0eA4be", "Aave V2 Lending Pool"),
];

const COMMON_PRECOMPILES: &[(&str, &str)] = &[
    ("0x0100000000000000000000000000000000000000", "P-Chain Handler"),
    ("0x0200000000000000000000000000000000000000", "Contract Deployer Allow List"),
    ("0x0200000000000000000000000000000000000001", "Contract Native Minter"),
    ("0x0200000000000000000000000000000000000002", "Fee Manager"),
];

fn main() -> redis::RedisResult<()> {
    println!("Starting Subnet Portability Worker [V3]...");
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
                        let result = analyze_portability_v3(&parsed_job);
                        publish_result(con, result);
                    }
                    Err(e) => eprintln!("Error parsing job JSON: {}", e),
                }
            }
            Err(e) => eprintln!("Error receiving job from Redis: {}", e),
        }
    }
}

fn analyze_portability_v3(job: &AnalysisJob) -> AnalysisResult {
    let mut issues: Vec<PortabilityIssue> = Vec::new();

    let chainid_regex = Regex::new(r"\bchainid\b").unwrap();
    let msg_value_regex = Regex::new(r"\bmsg\.value\b").unwrap();
    let balance_regex = Regex::new(r"\.balance\b").unwrap();
    let hardcoded_gas_regex = Regex::new(r"\.call\s*\{\s*gas:").unwrap();

    let subnet_gas_limit = job.subnet_genesis.as_ref().and_then(|g| g.config.fee_config.gas_limit);
    let enabled_precompiles: Option<Vec<String>> = job.subnet_genesis.as_ref()
        .and_then(|g| g.config.precompile_validator_allow_list.as_ref())
        .map(|p| p.keys().cloned().collect());
    
    if subnet_gas_limit.is_some() || enabled_precompiles.is_some() {
        println!("Analyzing with provided Subnet Genesis context.");
    }

    for (i, line_content) in job.source_code.lines().enumerate() {
        let line_num = (i + 1) as u32;

        if chainid_regex.is_match(line_content) { issues.push(PortabilityIssue{line: line_num, issue_type: "Hardcoded Chain Assumption".to_string(), description: "The `chainid` opcode was used.".to_string(), recommendation: "Avoid using `chainid` for core logic. On a new Subnet, this value will be different and may break your contract.".to_string()}); }
        if msg_value_regex.is_match(line_content) { issues.push(PortabilityIssue{line: line_num, issue_type: "Native Token Assumption".to_string(), description: "The `msg.value` keyword was used, assuming a native, value-bearing token.".to_string(), recommendation: "Be aware that many Subnets may use a valueless native token for gas, or may not use a native token at all (e.g., in favor of an ERC20 for fees). Logic relying on `msg.value > 0` may not be portable.".to_string()}); }
        if balance_regex.is_match(line_content) { issues.push(PortabilityIssue{line: line_num, issue_type: "Native Token Assumption".to_string(), description: "The `.balance` property was used, assuming a native, value-bearing token.".to_string(), recommendation: "Similar to `msg.value`, be aware that the native token on a custom Subnet may not be AVAX and could have different properties. Logic checking `address.balance` might behave as expected.".to_string()}); }
        if hardcoded_gas_regex.is_match(line_content) { issues.push(PortabilityIssue{line: line_num, issue_type: "Hardcoded Gas Amount".to_string(), description: "A low-level call with a hardcoded gas amount (`.call{gas: ...}`) was detected.".to_string(), recommendation: "This is a fragile pattern. Gas costs for opcodes can change, and Subnets may have different gas semantics. Avoid hardcoding gas unless absolutely necessary.".to_string()}); }
        for (address, name) in CCHAIN_ONLY_ADDRESSES { if line_content.to_lowercase().contains(&address.to_lowercase()) { issues.push(PortabilityIssue{line: line_num, issue_type: "C-Chain Dependency".to_string(), description: format!("A hardcoded address for a known C-Chain protocol ({}) was found.", name), recommendation: "This contract will not exist on a new Subnet. Pass protocol addresses in the constructor or a setter function to make your contract portable.".to_string()}); }}

        if let Some(ref precompiles) = enabled_precompiles {
            for (addr, name) in COMMON_PRECOMPILES {
                if line_content.to_lowercase().contains(&addr.to_lowercase()) {
                    let is_enabled = precompiles.iter().any(|p| p.eq_ignore_ascii_case(addr));
                    if !is_enabled {
                        issues.push(PortabilityIssue {
                            line: line_num,
                            issue_type: "Precompile Mismatch".to_string(),
                            description: format!("Contract interacts with the '{}' precompile, but it is NOT enabled in the provided Subnet genesis.", name),
                            recommendation: "Ensure your target Subnet's genesis file enables all precompiles your contracts require.".to_string(),
                        });
                    }
                }
            }
        }
    }
    
    if let Some(limit) = subnet_gas_limit {
        let simulated_function_cost = 1_000_000;
        if simulated_function_cost > limit {
            issues.push(PortabilityIssue {
                line: 0,
                issue_type: "Gas Limit Violation Prediction".to_string(),
                description: format!("A function in this contract has an estimated cost of {} gas, which exceeds the target Subnet's blockGasLimit of {}.", simulated_function_cost, limit),
                recommendation: "Optimize expensive functions or deploy to a Subnet with a higher block gas limit.".to_string(),
            });
        }
    }
    
    println!("Analysis complete. Found {} portability issues for Job ID: {}", issues.len(), job.job_id);

    // --- V3 FIX: Use HashSet for robust deduplication ---
    let unique_issues: HashSet<PortabilityIssue> = issues.into_iter().collect();
    let output: Vec<PortabilityIssue> = unique_issues.into_iter().collect();
    // --- END OF FIX ---

    AnalysisResult {
        job_id: job.job_id.clone(),
        worker_name: "SubnetPortabilityWorkerV3".to_string(),
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
