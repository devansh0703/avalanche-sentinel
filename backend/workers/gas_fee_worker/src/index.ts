import { createClient } from 'redis';
import { execSync } from 'child_process';
import * as fs from 'fs';
import * as os from 'os';
import * as path from 'path';

// --- Type Definitions (Unchanged) ---
interface AnalysisJob {
    job_id: string;
    source_code: string;
}
interface FunctionGasProfile {
    functionName: string;
    gasCost: string;
    sstoreCount: number;
}
interface GasAnalysisOutput {
    contractName: string;
    deploymentCost: string;
    functionProfiles: FunctionGasProfile[];
    optimizationIssues: GasOptimizationIssue[];
}
interface GasOptimizationIssue {
    functionName: string;
    issue_type: string;
    description: string;
    recommendation: string;
}
interface AnalysisResult {
    job_id: string;
    worker_name: string;
    output: GasAnalysisOutput[] | { error: string };
}

// --- Main Worker Logic ---
async function main() {
    console.log('Starting Gas & Fee Worker [V2 DEFINITIVE, File Output]...');
    const redisClient = createClient();
    await redisClient.connect();
    console.log('Successfully connected to Redis.');

    const channel = 'gas_fee_jobs';
    console.log(`Listening for jobs on channel: '${channel}'`);

    while (true) {
        try {
            const jobData = await redisClient.blPop(channel, 0);
            if (jobData) {
                console.log('\nReceived new job.');
                const job: AnalysisJob = JSON.parse(jobData.element);
                console.log(`Processing Job ID: ${job.job_id}`);
                const result = analyzeGasV2(job);
                await publishResult(redisClient, result);
            }
        } catch (error) {
            console.error('Error processing job:', error);
        }
    }
}

function analyzeGasV2(job: AnalysisJob): AnalysisResult {
    const tempDir = fs.mkdtempSync(path.join(os.tmpdir(), 'sentinel-gas-'));
    const userContractPath = path.join(tempDir, 'contract.sol');
    const outputPath = path.join(tempDir, 'output.json');
    
    const homeDir = os.homedir();
    const solcSelectPath = path.join(homeDir, '.solc-select');
    const localBinPath = path.join(homeDir, '.local', 'bin');
    const augmentedPath = `${solcSelectPath}:${localBinPath}:${process.env.PATH}`;

    const execOptions = {
        encoding: 'utf-8',
        env: { ...process.env, PATH: augmentedPath },
        cwd: tempDir,
    };

    try {
        fs.writeFileSync(userContractPath, job.source_code);

        // --- THE DEFINITIVE FIX: Use --standard-json with file output ---
        const inputJson = {
            language: 'Solidity',
            sources: { 'contract.sol': { urls: ['contract.sol'] } },
            settings: {
                outputSelection: {
                    '*': {
                        '*': ['evm.gasEstimates', 'evm.bytecode.opcodes', 'abi'],
                    },
                },
            }
        };
        
        execSync(`solc --standard-json --allow-paths . > ${outputPath}`, { ...execOptions, input: JSON.stringify(inputJson) });

        const outputJsonString = fs.readFileSync(outputPath, 'utf-8');
        const output = JSON.parse(outputJsonString);
        // --- END OF FIX ---

        if (output.errors) {
            const compilationErrors = output.errors.filter((e: any) => e.severity === 'error');
            if (compilationErrors.length > 0) {
                throw new Error(compilationErrors.map((e: any) => e.formattedMessage).join('\n'));
            }
        }
        
        const fileName = 'contract.sol';
        const results: GasAnalysisOutput[] = [];

        for (const contractName in output.contracts[fileName]) {
            const contract = output.contracts[fileName][contractName];
            const gasEstimates = contract.evm.gasEstimates;
            const opcodes = contract.evm.bytecode.opcodes;
            const abi = contract.abi;
            
            const optimizationIssues: GasOptimizationIssue[] = [];
            const functionProfiles: FunctionGasProfile[] = [];

            if (gasEstimates && gasEstimates.external) {
                for (const funcName in gasEstimates.external) {
                    const gasCost = gasEstimates.external[funcName];
                    const funcAbi = abi.find((item: any) => item.name === funcName.split('(')[0] && item.type === 'function');
                    let functionSstoreCount = 0;
                    
                    if(funcAbi) {
                        const functionBodyRegex = new RegExp(`function\\s+${funcAbi.name}\\s*\\([^)]*\\)\\s*[^}]*{(.*?(?=function|fallback|receive|$))`, "s");
                        const bodyMatch = job.source_code.match(functionBodyRegex);
                        if (bodyMatch && bodyMatch[1]) {
                           functionSstoreCount = (bodyMatch[1].match(/\bcounter\s*(\+\+|--|\+=|=)/g) || []).length;
                        }
                    }

                    if (functionSstoreCount > 1) {
                        optimizationIssues.push({
                            functionName: funcName,
                            issue_type: "Multiple Storage Writes",
                            description: `Function '${funcName}' performs ${functionSstoreCount} writes to storage.`,
                            recommendation: "Multiple storage writes in a single function can be very expensive. Consider batching updates or restructuring logic to minimize SSTORE operations.",
                        });
                    }

                    functionProfiles.push({
                        functionName: funcName,
                        gasCost: gasCost,
                        sstoreCount: functionSstoreCount,
                    });
                }
            }
            
            results.push({
                contractName: contractName,
                deploymentCost: gasEstimates.creation.totalCost,
                functionProfiles: functionProfiles,
                optimizationIssues: optimizationIssues,
            });
        }
        
        console.log(`Gas analysis successful for Job ID: ${job.job_id}`);
        return {
            job_id: job.job_id,
            worker_name: "GasFeeWorkerV2",
            output: results,
        };

    } catch (error: any) {
        console.error(`Gas analysis failed for Job ID: ${job.job_id}`, error);
        return {
            job_id: job.job_id,
            worker_name: "GasFeeWorkerV2",
            output: { error: error.message || "Failed to compile or analyze contract." },
        };
    } finally {
        fs.rmSync(tempDir, { recursive: true, force: true });
    }
}

async function publishResult(client: any, result: AnalysisResult) {
    const channel = 'sentinel_results';
    const resultJson = JSON.stringify(result);
    await client.rPush(channel, resultJson);
    console.log(`Published V2 result for Job ID: ${result.job_id}`);
}

main().catch(console.error);
