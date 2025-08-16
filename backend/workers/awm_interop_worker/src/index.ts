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
interface AWM_Issue {
    issue_type: string;
    description: string;
    recommendation: string;
}
interface AnalysisResult {
    job_id: string;
    worker_name: string;
    output: AWM_Issue[];
}

// --- Robust AST Visitor (Unchanged) ---
function visit(node: any, visitor: { [nodeType: string]: (node: any) => void }) {
    if (!node || typeof node !== 'object') return;
    if (node.nodeType && visitor[node.nodeType]) visitor[node.nodeType](node);
    for (const key in node) {
        if (node.hasOwnProperty(key)) {
            const child = node[key];
            if (child instanceof Array) {
                for (const item of child) visit(item, visitor);
            } else if (child) {
                visit(child, visitor);
            }
        }
    }
}

// --- Main Worker Logic (Unchanged) ---
async function main() {
    console.log('Starting AWM Interoperability Worker [V2 DEFINITIVE]...');
    const redisClient = createClient();
    await redisClient.connect();
    console.log('Successfully connected to Redis.');

    const channel = 'awm_interop_jobs';
    console.log(`Listening for jobs on channel: '${channel}'`);

    while (true) {
        try {
            const jobData = await redisClient.blPop(channel, 0);
            if (jobData) {
                console.log('\nReceived new job.');
                const job: AnalysisJob = JSON.parse(jobData.element);
                console.log(`Processing Job ID: ${job.job_id}`);
                const result = analyzeAWM_V2(job);
                await publishResult(redisClient, result);
            }
        } catch (error) {
            console.error('Error processing job:', error);
        }
    }
}

// --- V2: Final, Definitive Analysis Function with Robust solc Output Parsing ---
function analyzeAWM_V2(job: AnalysisJob): AnalysisResult {
    let issues: AWM_Issue[] = [];
    const tempDir = fs.mkdtempSync(path.join(os.tmpdir(), 'sentinel-awm-'));
    const userContractPath = path.join(tempDir, 'contract.sol');
    const interfacePath = path.join(tempDir, 'IAvalancheWarpMessenger.sol');
    
    const homeDir = os.homedir();
    const solcSelectPath = path.join(homeDir, '.solc-select');
    const localBinPath = path.join(homeDir, '.local', 'bin');
    const augmentedPath = `${solcSelectPath}:${localBinPath}:${process.env.PATH}`;

    const execOptions = {
        encoding: 'utf-8',
        env: {
            ...process.env,
            PATH: augmentedPath,
        },
    };

    try {
        const interfaceContent = `pragma solidity ^0.8.10; interface IAvalancheWarpMessenger { function send(bytes calldata message) external returns (bytes32 messageId); }`;
        fs.writeFileSync(interfacePath, interfaceContent);
        fs.writeFileSync(userContractPath, job.source_code);

        // --- THE DEFINITIVE FIX IS HERE ---
        // We capture stderr separately and only try to parse the last line of stdout
        // as the AST JSON, as solc often prints warnings/errors to stdout before the JSON.
        const command = `solc --ast-compact-json ${userContractPath}`;
        let stdout: string;
        let stderr: string;

        try {
            const output = execSync(command, execOptions);
            stdout = output.toString();
        } catch (e: any) {
            // If command failed, its output is in the error message
            stdout = e.stdout ? e.stdout.toString() : '';
            stderr = e.stderr ? e.stderr.toString() : '';
            // If there's an actual compilation error, we'll throw it
            if (stderr.includes("Error:") || stdout.includes("Error:")) {
                 throw new Error(`Compilation failed: ${stderr || stdout}`);
            }
        }
        
        // Split by lines, find the last non-empty line, and try to parse it as JSON
        const lines = stdout.split('\n').filter(line => line.trim() !== '');
        let astJsonString = '';
        if (lines.length > 0) {
            astJsonString = lines[lines.length - 1]; // Assume AST is the last non-empty line
        }

        if (!astJsonString.startsWith('{') || !astJsonString.endsWith('}')) {
             throw new Error("Could not find valid AST JSON in solc output.");
        }

        const ast = JSON.parse(astJsonString);
        // --- END OF FIX ---

        let receiveFunctionNode: any | null = null;
        let hasReplayProtectionMapping = false;
        let sendInTryCatch = false;
        let totalSendCalls = 0;

        visit(ast, {
            FunctionDefinition: (node) => {
                if (node.name === 'receive') receiveFunctionNode = node;
            },
            VariableDeclaration: (node) => {
                if (node.typeName && node.typeName.nodeType === 'Mapping') {
                    if (node.typeName.keyType.name === 'bytes32' && node.typeName.valueType.name === 'bool') {
                        hasReplayProtectionMapping = true;
                    }
                }
            },
            TryStatement: (node) => {
                if (JSON.stringify(node.body).includes('send')) sendInTryCatch = true;
            },
            FunctionCall: (node) => {
                if (node.expression.nodeType === 'MemberAccess' && node.expression.memberName === 'send') totalSendCalls++;
            }
        });

        if (receiveFunctionNode) {
            const body = JSON.stringify(receiveFunctionNode.body);
            if (!body.includes('sourceChainId')) issues.push({ issue_type: "Critical Security Risk", description: "The `receive` function does not validate `warpMessage.sourceChainId`.", recommendation: "ALWAYS verify the source chain ID to prevent messages from unauthorized Subnets." });
            if (!body.includes('sender')) issues.push({ issue_type: "High Security Risk", description: "The `receive` function does not validate `warpMessage.sender`.", recommendation: "ALWAYS verify the sender address to ensure the message is from a trusted source." });
            if (!hasReplayProtectionMapping || !body.includes('executedMessages')) issues.push({ issue_type: "Critical Security Risk", description: "No replay protection mechanism (like a nonce or message hash mapping) was found or used in the `receive` function.", recommendation: "To prevent replay attacks, create a mapping `mapping(bytes32 => bool) public executedMessages;` and check `require(!executedMessages[warpMessage.id], ...)` at the start of your `receive` function." });
        } else {
            if (job.source_code.includes("IAvalancheWarpMessenger")) issues.push({ issue_type: "Missing `receive` Function", description: "The contract imports the AWM interface but is missing the `receive` function.", recommendation: "Implement `receive(bytes calldata signedMessage)` to process Warp messages." });
        }

        if (totalSendCalls > 0 && !sendInTryCatch) {
            issues.push({ issue_type: "Robustness Issue", description: "A call to the AWM `.send()` function was detected outside of a `try/catch` block.", recommendation: "Wrap your `.send()` call in a `try/catch` block to handle potential failures gracefully." });
        }
        
        console.log(`V2 analysis complete. Found ${issues.length} AWM issues for Job ID: ${job.job_id}`);

    } catch (error: any) {
        issues.push({ issue_type: "Analysis Error", description: error.message || "The contract could not be analyzed.", recommendation: "Ensure the contract is compilable." });
    } finally {
        fs.rmSync(tempDir, { recursive: true, force: true });
    }

    return {
        job_id: job.job_id,
        worker_name: "AWM_InteropWorkerV2",
        output: issues,
    };
}

async function publishResult(client: any, result: AnalysisResult) {
    const channel = 'sentinel_results';
    const resultJson = JSON.stringify(result);
    await client.rPush(channel, resultJson);
    console.log(`Published V2 result for Job ID: ${result.job_id}`);
}
main().catch(console.error);
