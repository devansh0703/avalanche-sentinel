import { createClient } from 'redis';
import { v4 as uuidv4 } from 'uuid';
import { parse, ASTNode } from 'solidity-parser-antlr';

// --- Type Definitions ---
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

// --- Main Worker Logic ---
async function main() {
    console.log('Starting AWM Interoperability Worker...');
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

                const result = analyzeAWM(job);
                await publishResult(redisClient, result);
            }
        } catch (error) {
            console.error('Error processing job:', error);
        }
    }
}

function analyzeAWM(job: AnalysisJob): AnalysisResult {
    let issues: AWM_Issue[] = [];
    try {
        const ast = parse(job.source_code, { loc: true });

        let importsAWM = false;
        let receiveFunctionNode: ASTNode | null = null;

        // Traverse the AST to find key information
        visit(ast, {
            ImportDirective: (node) => {
                if (node.path.includes("IAvalancheWarpMessenger.sol")) {
                    importsAWM = true;
                }
            },
            FunctionDefinition: (node) => {
                if (node.name === 'receive') {
                    receiveFunctionNode = node;
                }
            }
        });

        // --- Check 1: AWM Interface Import ---
        if (!importsAWM) {
            issues.push({
                issue_type: "Missing Import",
                description: "The contract does not import `IAvalancheWarpMessenger.sol`.",
                recommendation: "Ensure you import the official AWM interface to properly handle incoming Warp messages.",
            });
        }
        
        // --- Check 2: Existence of `receive` function ---
        if (!receiveFunctionNode) {
            issues.push({
                issue_type: "Missing `receive` Function",
                description: "The contract is missing the `receive` function required to accept Warp messages.",
                recommendation: "Implement a `receive(bytes calldata signedMessage)` function to process incoming messages.",
            });
        } else {
            // --- Check 3: Security Validations within `receive` function ---
            const functionBody = JSON.stringify(receiveFunctionNode.body);
            
            const checksSourceChainId = functionBody.includes('sourceChainId');
            const checksSender = functionBody.includes('sender');

            if (!checksSourceChainId) {
                issues.push({
                    issue_type: "Critical Security Risk",
                    description: "The `receive` function does not appear to validate `warpMessage.sourceChainId`.",
                    recommendation: "ALWAYS verify the source chain ID to prevent messages from unauthorized or malicious Subnets. E.g., `require(warpMessage.sourceChainId == expectedChainId, ...)`",
                });
            }
            if (!checksSender) {
                 issues.push({
                    issue_type: "High Security Risk",
                    description: "The `receive` function does not appear to validate `warpMessage.sender`.",
                    recommendation: "ALWAYS verify the sender address to ensure the message originates from a trusted contract or user on the source chain.",
                });
            }
        }
        console.log(`Analysis complete. Found ${issues.length} AWM issues for Job ID: ${job.job_id}`);

    } catch (error) {
        issues.push({
            issue_type: "Parsing Error",
            description: "The Solidity code could not be parsed. Check for syntax errors.",
            recommendation: "Ensure the contract is compilable before submitting for analysis.",
        });
    }

    return {
        job_id: job.job_id,
        worker_name: "AWM_InteropWorker",
        output: issues,
    };
}

async function publishResult(client: any, result: AnalysisResult) {
    const channel = 'sentinel_results';
    const resultJson = JSON.stringify(result);
    await client.rPush(channel, resultJson);
    console.log(`Published result for Job ID: ${result.job_id}`);
}

// Helper from solidity-parser-antlr to traverse the AST
function visit(node: ASTNode, visitor: { [nodeType: string]: (node: ASTNode) => void }) {
    if (visitor[node.type]) {
        visitor[node.type](node);
    }
    for (const key in node) {
        if (node.hasOwnProperty(key)) {
            const child = (node as any)[key];
            if (child instanceof Array) {
                for (const item of child) {
                    if (item && typeof item.type === 'string') {
                        visit(item, visitor);
                    }
                }
            } else if (child && typeof child.type === 'string') {
                visit(child, visitor);
            }
        }
    }
}


main().catch(console.error);
