import { createClient } from 'redis';
import { execSync } from 'child_process';
import * as fs from 'fs';
import * as os from 'os';
import * as path from 'path';

// --- Type Definitions ---
interface AnalysisJob {
    job_id: string;
    source_code: string;
    subnet_genesis?: Genesis;
}
interface Genesis { config: { feeConfig: { minBaseFee?: number } } }
interface FunctionGasProfile {
    functionName: string;
    gasCost: string;
    subnetFeeComparison?: string;
}
interface GasAnalysisOutput {
    contractName: string;
    deploymentCost: string;
    functionProfiles: FunctionGasProfile[];
    optimizationIssues: GasOptimizationIssue[];
}
interface GasOptimizationIssue {
    line: number;
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
    console.log('Starting Gas & Fee Worker [V3 DEFINITIVE, Regex]...');
    const redisClient = createClient();
    await redisClient.connect();
    console.log('Successfully connected to Redis.');

    const channel = 'gas_fee_jobs';
    console.log(`Listening for jobs on channel: '${channel}'`);

    while (true) {
        try {
            const jobData = await redisClient.blPop(channel, 0);
            if (jobData) {
                const job: AnalysisJob = JSON.parse(jobData.element);
                console.log(`\nProcessing Job ID: ${job.job_id}`);
                const result = analyzeGasV3(job);
                await publishResult(redisClient, result);
            }
        } catch (error) {
            console.error('Error processing job:', error);
        }
    }
}

function analyzeGasV3(job: AnalysisJob): AnalysisResult {
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

        const inputJson = {
            language: 'Solidity',
            sources: { 'contract.sol': { urls: ['contract.sol'] } },
            settings: { outputSelection: { '*': { '*': ['evm.gasEstimates'] } } }
        };
        
        execSync(`solc --standard-json --allow-paths . > ${outputPath}`, { ...execOptions, input: JSON.stringify(inputJson) });

        const outputJsonString = fs.readFileSync(outputPath, 'utf-8');
        const output = JSON.parse(outputJsonString);

        if (output.errors) {
            const compilationErrors = output.errors.filter((e: any) => e.severity === 'error');
            if (compilationErrors.length > 0) {
                throw new Error(compilationErrors.map((e: any) => e.formattedMessage).join('\n'));
            }
        }
        
        const fileName = 'contract.sol';
        const results: GasAnalysisOutput[] = [];
        let optimizationIssues: GasOptimizationIssue[] = [];
        
        // --- THE DEFINITIVE FIX: Use Regex on the source code ---
        const lines = job.source_code.split('\n');
        // Regex to find function definitions with dynamic memory parameters
        const griefingRegex = /function\s+([a-zA-Z0-9_]+)\s*\([^)]*?(?:string|bytes)\s+memory\s+[^)]*?\)/g;

        for(let i = 0; i < lines.length; i++) {
            const line = lines[i];
            let match;
            while ((match = griefingRegex.exec(line)) !== null) {
                optimizationIssues.push({
                    line: i + 1,
                    issue_type: "Griefing Vector Hazard",
                    description: `Function '${match[1]}' accepts a dynamic 'string' or 'bytes' memory parameter.`,
                    recommendation: "On Subnets with low/zero fees, an attacker can pass a very large input to this function, forcing the contract to perform expensive operations (hashing, copying) at no cost to them. Implement strict size limits on dynamic inputs.",
                });
            }
        }
        // --- END OF FIX ---

        const contractsOutput = output.contracts[fileName];
        if (contractsOutput) {
            for (const contractName in contractsOutput) {
                const contract = contractsOutput[contractName];
                const gasEstimates = contract.evm && contract.evm.gasEstimates;
                const functionProfiles: FunctionGasProfile[] = [];

                if (gasEstimates && gasEstimates.external) {
                    for (const funcName in gasEstimates.external) {
                        const gasCostStr = gasEstimates.external[funcName];
                        if (gasCostStr === 'infinite') continue;
                        const gasCost = parseInt(gasCostStr, 10);
                        
                        let subnetFeeComparison: string | undefined = undefined;
                        if (job.subnet_genesis && job.subnet_genesis.config.feeConfig.minBaseFee) {
                            const minFee = job.subnet_genesis.config.feeConfig.minBaseFee;
                            const subnetCost = gasCost * minFee;
                            const cChainCostGwei = 25;
                            const cChainCost = gasCost * cChainCostGwei;
                            subnetFeeComparison = `Estimated cost on Subnet: ${subnetCost} nAVAX vs. ~${cChainCost} nAVAX on C-Chain.`;
                        }

                        functionProfiles.push({
                            functionName: funcName,
                            gasCost: gasCostStr,
                            subnetFeeComparison,
                        });
                    }
                }
                
                results.push({
                    contractName: contractName,
                    deploymentCost: (gasEstimates && gasEstimates.creation) ? gasEstimates.creation.totalCost : 'N/A',
                    functionProfiles: functionProfiles,
                    optimizationIssues: optimizationIssues,
                });
            }
        }
        
        console.log(`V3 gas analysis successful for Job ID: ${job.job_id}`);
        return { job_id: job.job_id, worker_name: "GasFeeWorkerV3", output: results };

    } catch (error: any) {
        console.error(`Gas analysis failed for Job ID: ${job.job_id}`, error);
        return { job_id: job.job_id, worker_name: "GasFeeWorkerV3", output: { error: error.message || "Failed to analyze contract." } };
    } finally {
        fs.rmSync(tempDir, { recursive: true, force: true });
    }
}

async function publishResult(client: any, result: AnalysisResult) {
    const channel = 'sentinel_results';
    const resultJson = JSON.stringify(result);
    await client.rPush(channel, resultJson);
    console.log(`Published V3 result for Job ID: ${result.job_id}`);
}

main().catch(console.error);
