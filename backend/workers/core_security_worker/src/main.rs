use redis::{Commands, Client, Connection};
use serde::{Deserialize, Serialize};
use std::env;
use std::fs;
use subprocess::{Exec, Redirection};
use uuid::Uuid;
use home::home_dir;

#[derive(Serialize, Deserialize, Debug)]
struct AnalysisJob {
    job_id: String,
    source_code: String,
}

#[derive(Serialize, Deserialize, Debug)]
struct AnalysisResult {
    job_id: String,
    worker_name: String,
    output: serde_json::Value,
}


fn main() -> redis::RedisResult<()> {
    println!("Starting Core Security Worker...");

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
                        let result = process_slither_job(&parsed_job);
                        publish_result(con, result);
                    }
                    Err(e) => eprintln!("Error parsing job JSON: {}", e),
                }
            }
            Err(e) => eprintln!("Error receiving job from Redis: {}", e),
        }
    }
}

fn process_slither_job(job: &AnalysisJob) -> AnalysisResult {
    let unique_id = Uuid::new_v4();
    let temp_dir = env::temp_dir();

    let contract_filename = format!("{}.sol", unique_id);
    let contract_path = temp_dir.join(&contract_filename);
    let json_output_filename = format!("{}.json", unique_id);
    let json_output_path = temp_dir.join(&json_output_filename);
    
    let existing_path = env::var("PATH").unwrap_or_else(|_| "".to_string());
    
    let new_path = match home_dir() {
        Some(path) => {
            let solc_select_path = path.join(".solc-select").to_string_lossy().to_string();
            let local_bin_path = path.join(".local/bin").to_string_lossy().to_string();
            format!("{}:{}:{}", solc_select_path, local_bin_path, existing_path)
        },
        None => existing_path,
    };

    println!("Using augmented PATH: {}", new_path);

    if let Err(e) = fs::write(&contract_path, &job.source_code) {
        eprintln!("Failed to write to temporary file: {}", e);
        return create_error_result(job, "Failed to create temporary contract file.");
    }

    println!("Running Slither on temporary file: {:?}", contract_path);
    
    // Execute the command, but we will no longer immediately trust the exit code
    let capture_result = Exec::cmd("python3")
        .arg("-m")
        .arg("slither")
        .arg(&contract_path)
        .arg("--json")
        .arg(&json_output_path)
        .env("PATH", &new_path)
        .stdout(Redirection::Pipe)
        .stderr(Redirection::Pipe)
        .capture();

    // --- START OF THE FINAL LOGIC FIX ---
    // Our new definition of success: was the JSON file created?
    let result = match fs::read_to_string(&json_output_path) {
        Ok(json_str) => {
            // YES! The file exists. The run was a success.
            println!("Slither analysis successful. Found JSON output file.");
            let slither_json: serde_json::Value = serde_json::from_str(&json_str)
                .unwrap_or_else(|_| serde_json::json!({ "error": "Failed to parse Slither JSON output from file." }));
            
            AnalysisResult {
                job_id: job.job_id.clone(),
                worker_name: "CoreSecurityWorker".to_string(),
                output: slither_json,
            }
        },
        Err(_) => {
            // NO. The file does not exist. A true failure occurred.
            // Now we can use the stderr from the process to report the error.
            let error_msg = match capture_result {
                Ok(data) => String::from_utf8_lossy(&data.stderr).to_string(),
                Err(e) => e.to_string(),
            };
            eprintln!("Slither execution truly failed. No JSON file found. Error: {}", error_msg);
            create_error_result(job, &format!("Slither execution failed: {}", error_msg))
        }
    };
    // --- END OF THE FINAL LOGIC FIX ---

    if let Err(e) = fs::remove_file(&contract_path) {
        eprintln!("Warning: Failed to remove temporary contract file: {}", e);
    }
    // Use `if let` here to avoid a panic if the file doesn't exist
    if let Err(e) = fs::remove_file(&json_output_path) {
        eprintln!("Warning: Failed to remove temporary json file: {}", e);
    }

    result
}

fn create_error_result(job: &AnalysisJob, error_message: &str) -> AnalysisResult {
    AnalysisResult {
        job_id: job.job_id.clone(),
        worker_name: "CoreSecurityWorker".to_string(),
        output: serde_json::json!({
            "success": false, "error": "WorkerError", "results": { "detectors": [{"check": "WorkerInternalError", "description": error_message, "impact": "High"}] }
        }),
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
