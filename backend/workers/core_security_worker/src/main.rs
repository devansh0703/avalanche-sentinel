use redis::{Commands, Client, Connection};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::env;
use std::fs;
use subprocess::{Exec, Redirection};
use uuid::Uuid;
use home::home_dir;

#[derive(Serialize, Deserialize, Debug, Clone)]
struct InformationalFinding {
    finding_type: String,
    message: String,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
struct V2AnalysisResult {
    informational_findings: Vec<InformationalFinding>,
    slither_report: Value,
}

#[derive(Serialize, Deserialize, Debug)]
struct AnalysisJob {
    job_id: String,
    source_code: String,
}

#[derive(Serialize, Deserialize, Debug)]
struct FinalResult {
    job_id: String,
    worker_name: String,
    output: V2AnalysisResult,
}

fn main() -> redis::RedisResult<()> {
    println!("Starting Core Security Worker [V2.1 DEFINITIVE]...");
    let redis_client = Client::open("redis://127.0.0.1/")?;
    let mut redis_con = redis_client.get_connection()?;
    println!("Successfully connected to Redis.");
    listen_for_jobs(&mut redis_con);
    Ok(())
}

fn listen_for_jobs(con: &mut Connection) {
    let channel = "core_security_jobs";
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
                        let result = tokio::runtime::Runtime::new().unwrap().block_on(process_job_v2(&parsed_job));
                        publish_result(con, result);
                    }
                    Err(e) => eprintln!("Error parsing job JSON: {}", e),
                }
            }
            Err(e) => eprintln!("Error receiving job from Redis: {}", e),
        }
    }
}

async fn run_slither(contract_path: &std::path::Path) -> Result<(Value, Vec<InformationalFinding>), String> {
    println!("Running Slither for full analysis...");
    let json_output_filename = format!("{}.json", Uuid::new_v4());
    let json_output_path = env::temp_dir().join(&json_output_filename);

    let existing_path = env::var("PATH").unwrap_or_else(|_| "".to_string());
    let new_path = match home_dir() {
        Some(path) => format!("{}:{}:{}:{}", path.join(".foundry/bin").to_string_lossy(), path.join(".solc-select").to_string_lossy(), path.join(".local/bin").to_string_lossy(), existing_path),
        None => existing_path,
    };

    let capture = Exec::cmd("python3")
        .arg("-m").arg("slither")
        .arg(contract_path)
        .arg("--json").arg(&json_output_path)
        .env("PATH", &new_path)
        .stdout(Redirection::Pipe).stderr(Redirection::Pipe)
        .capture();

    let mut informational_findings = Vec::new();

    match capture {
        Ok(data) => {
            let stderr_str = String::from_utf8_lossy(&data.stderr);
            for line in stderr_str.lines() {
                if line.contains("Warning:") {
                    informational_findings.push(InformationalFinding {
                        finding_type: "Compiler Warning".to_string(),
                        message: line.trim().to_string(),
                    });
                }
            }
            
            if json_output_path.exists() {
                let json_str = fs::read_to_string(&json_output_path).map_err(|e| e.to_string())?;
                fs::remove_file(&json_output_path).ok();
                let slither_json: Value = serde_json::from_str(&json_str).map_err(|e| e.to_string())?;
                println!("Slither analysis successful.");
                Ok((slither_json, informational_findings))
            } else {
                Err("Slither failed to produce an output file.".to_string())
            }
        }
        Err(e) => Err(format!("Failed to execute Slither command: {}", e)),
    }
}

async fn process_job_v2(job: &AnalysisJob) -> FinalResult {
    let unique_id = Uuid::new_v4();
    let contract_filename = format!("{}.sol", unique_id);
    let contract_path = env::temp_dir().join(&contract_filename);

    if let Err(e) = fs::write(&contract_path, &job.source_code) {
        return create_error_result(job, &format!("Failed to create temporary file: {}", e));
    }

    let (slither_report, informational_findings) = run_slither(&contract_path).await.unwrap_or_else(|err_str| {
        let error_report = serde_json::json!({ "success": false, "error": err_str, "results": {} });
        (error_report, Vec::new())
    });

    fs::remove_file(&contract_path).ok();

    FinalResult {
        job_id: job.job_id.clone(),
        worker_name: "CoreSecurityWorkerV2.1".to_string(),
        output: V2AnalysisResult {
            informational_findings,
            slither_report,
        },
    }
}

fn create_error_result(job: &AnalysisJob, error_message: &str) -> FinalResult {
     FinalResult {
        job_id: job.job_id.clone(),
        worker_name: "CoreSecurityWorkerV2.1".to_string(),
        output: V2AnalysisResult {
            informational_findings: vec![InformationalFinding{ finding_type: "error".to_string(), message: error_message.to_string() }],
            slither_report: Value::Null,
        },
    }
}

fn publish_result(con: &mut Connection, result: FinalResult) {
    let channel = "sentinel_results";
    match serde_json::to_string(&result) {
        Ok(result_json) => {
            println!("Publishing V2.1 result for Job ID: {}", result.job_id);
            if let Err(e) = con.rpush::<_, _, ()>(channel, result_json) {
                eprintln!("Failed to publish result to Redis: {}", e);
            }
        }
        Err(e) => eprintln!("Failed to serialize result to JSON: {}", e),
    }
}
