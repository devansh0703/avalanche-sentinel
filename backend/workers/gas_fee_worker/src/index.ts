import { createClient } from 'redis';
import solc from 'solc';

interface AnalysisJob {
    job_id: string;
    source_code: string;
}

interface FunctionGasProfile {
    functionName: string;
    gasCost: string;
    sensitivity: 'Low' | 'Medium' | 'High';
}

interface GasAnalysisOutput {
    contractName: string;
    deploymentCost: string;
    functionProfiles: FunctionGasProfile[];
}

interface AnalysisResult {
    job_id: string;
    worker_name: string;
    output: GasAnalysisOutput[] | { error: string }; // Can be an error object
}

const HIGH_GAS_THRESHOLD = 50000;
const MEDIUM_GAS_THRESHOLD = 20000;

async function main() {
    console.log('Starting Gas & Fee Worker...');
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

                const result = analyzeGas(job);
                await publishResult(redisClient, result);
            }
        } catch (error) {
            console.error('Error processing job:', error);
        }
    }
}

function analyzeGas(job: AnalysisJob): AnalysisResult {
    const fileName = 'contract.sol';
    const input = {
        language: 'Solidity',
        sources: {
            [fileName]: {
                content: job.source_code,
            },
        },
        settings: {
            outputSelection: {
                '*': {
                    '*': ['evm.gasEstimates'],
                },
            },
        },
    };

    try {
        const output = JSON.parse(solc.compile(JSON.stringify(input)));
        
        if (output.errors) {
            const compilationErrors = output.errors.filter((e: any) => e.severity === 'error');
            if (compilationErrors.length > 0) {
                throw new Error(compilationErrors.map((e: any) => e.formattedMessage).join('\n'));
            }
        }

        const results: GasAnalysisOutput[] = [];

        for (const contractName in output.contracts[fileName]) {
            const contract = output.contracts[fileName][contractName];
            const gasEstimates = contract.evm.gasEstimates;

            const functionProfiles: FunctionGasProfile[] = [];
            for (const funcName in gasEstimates.external) {
                const gasCost = gasEstimates.external[funcName];
                const gasCostNum = parseInt(gasCost, 10);
                
                let sensitivity: 'Low' | 'Medium' | 'High' = 'Low';
                if (gasCostNum > HIGH_GAS_THRESHOLD) {
                    sensitivity = 'High';
                } else if (gasCostNum > MEDIUM_GAS_THRESHOLD) {
                    sensitivity = 'Medium';
                }

                functionProfiles.push({
                    functionName: funcName,
                    gasCost: gasCost,
                    sensitivity: sensitivity
                });
            }
            
            results.push({
                contractName: contractName,
                deploymentCost: gasEstimates.creation.totalCost,
                functionProfiles: functionProfiles
            });
        }
        
        console.log(`Gas analysis successful for Job ID: ${job.job_id}`);
        return {
            job_id: job.job_id,
            worker_name: "GasFeeWorker",
            output: results,
        };

    } catch (error: any) {
        console.error(`Gas analysis failed for Job ID: ${job.job_id}`, error);
        return {
            job_id: job.job_id,
            worker_name: "GasFeeWorker",
            output: { error: error.message || "Failed to compile or analyze contract." },
        };
    }
}

async function publishResult(client: any, result: AnalysisResult) {
    const channel = 'sentinel_results';
    const resultJson = JSON.stringify(result);
    await client.rPush(channel, resultJson);
    console.log(`Published result for Job ID: ${result.job_id}`);
}

main().catch(console.error);
