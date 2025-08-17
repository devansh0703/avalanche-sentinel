import { createClient } from 'redis';
import * as semver from 'semver';
import axios from 'axios';

// --- Type Definitions (Unchanged) ---
interface AnalysisJob {
    job_id: string;
    source_code: string;
}
interface DependencyIssue {
    line: number;
    issue_type: 'Outdated Version' | 'Unknown Source' | 'Floating Pragma' | 'Unverified Contract' | 'Interface Mismatch';
    description: string;
    recommendation: string;
}
interface AnalysisResult {
    job_id: string;
    worker_name: string;
    output: DependencyIssue[];
}

// V1/V2 Knowledge Base
const KNOWN_LIBRARIES: { [key: string]: string } = {
    '@openzeppelin/contracts': '5.0.0',
    '@openzeppelin/contracts-upgradeable': '5.0.0',
    '@avalabs/core': '0.5.5',
};

// V3 Knowledge Base
const KNOWN_INTERFACES: { [address: string]: { name: string, functions: string[] } } = {
    "0x60ae616a2155ee3d9a68541ba4544862310933d4": {
        name: "Trader Joe V2.1 Router",
        functions: ["swapExactTokensForTokens", "addLiquidity", "removeLiquidity"]
    }
};

async function main() {
    console.log('Starting Ecosystem & Dependency Worker [V3 FINAL]...');
    const redisClient = createClient();
    await redisClient.connect();
    console.log('Successfully connected to Redis.');

    const channel = 'ecosystem_jobs';
    console.log(`Listening for jobs on channel: '${channel}'`);

    while (true) {
        try {
            const jobData = await redisClient.blPop(channel, 0);
            if (jobData) {
                const job: AnalysisJob = JSON.parse(jobData.element);
                console.log(`\nProcessing Job ID: ${job.job_id}`);
                const result = await analyzeDependenciesV3(job);
                await publishResult(redisClient, result);
            }
        } catch (error) {
            console.error('Error processing job:', error);
        }
    }
}

async function analyzeDependenciesV3(job: AnalysisJob): Promise<AnalysisResult> {
    let issues: DependencyIssue[] = [];
    const importRegex = /import\s+["']([^"']+)["'];/g;
    const pragmaRegex = /pragma\s+solidity\s*([^\s;]+)/g;
    const addressRegex = /(0x[a-fA-F0-9]{40})/g;

    const lines = job.source_code.split('\n');
    let addressesFound: { address: string, line: number }[] = [];

    for (let i = 0; i < lines.length; i++) {
        const lineContent = lines[i];
        const lineNum = i + 1;
        let match;

        // --- V1/V2: Outdated Version & Unknown Source Checks (RESTORED) ---
        importRegex.lastIndex = 0;
        while ((match = importRegex.exec(lineContent)) !== null) {
            const importPath = match[1];

            if (importPath.startsWith('http:') || importPath.startsWith('https:')) {
                issues.push({ line: lineNum, issue_type: 'Unknown Source', description: `Import from a direct URL is detected.`, recommendation: 'Use a package manager with version pinning to ensure dependency integrity.' });
                continue;
            }

            for (const libName in KNOWN_LIBRARIES) {
                if (importPath.includes(libName)) {
                    const versionMatch = importPath.match(/@([\d\.]+-?[A-Za-z\.\d]*)/);
                    if (versionMatch) {
                        const importedVersion = semver.coerce(versionMatch[1]);
                        const latestVersion = KNOWN_LIBRARIES[libName];
                        if (importedVersion && semver.lt(importedVersion, latestVersion)) {
                             issues.push({ line: lineNum, issue_type: 'Outdated Version', description: `An outdated version of ${libName} (${importedVersion.version}) is imported.`, recommendation: `The recommended version is ${latestVersion}. Outdated libraries may contain known vulnerabilities.` });
                        }
                    }
                }
            }
        }
        // --- END OF V1/V2 ---
        
        // --- V2: Floating Pragma Check (RESTORED) ---
        pragmaRegex.lastIndex = 0; 
        while ((match = pragmaRegex.exec(lineContent)) !== null) {
            const versionConstraint = match[1];
            if (versionConstraint.startsWith('^') || versionConstraint.startsWith('~')) {
                issues.push({ line: lineNum, issue_type: 'Floating Pragma', description: `A floating pragma ('pragma solidity ${versionConstraint}') was detected.`, recommendation: 'For production contracts, pin to an exact Solidity version (e.g., `pragma solidity 0.8.20;`) to ensure verifiability.' });
            }
        }
        // --- END OF V2 ---
        
        // --- V3: Collect all hardcoded addresses ---
        addressRegex.lastIndex = 0;
        while ((match = addressRegex.exec(lineContent)) !== null) {
            if (match[1] !== '0x0000000000000000000000000000000000000000' && !addressesFound.some(a => a.address.toLowerCase() === match[1].toLowerCase())) {
                addressesFound.push({ address: match[1], line: lineNum });
            }
        }
    }

    // --- V3: Perform live on-chain checks ---
    if (addressesFound.length > 0) {
        console.log(`Found ${addressesFound.length} unique addresses to check...`);
        for (const { address, line } of addressesFound) {
            try {
                const apiUrl = `https://api.snowtrace.io/api?module=contract&action=getsourcecode&address=${address}`;
                const response = await axios.get(apiUrl, { timeout: 5000 });
                
                if (response.data.message && response.data.message.includes("rate limit")) {
                    console.warn(`Snowtrace API rate limit reached. Skipping remaining address checks.`);
                    break;
                }
                
                const result = response.data.result[0];
                
                if (!result || !result.SourceCode || result.SourceCode === '') {
                    issues.push({ line: line, issue_type: 'Unverified Contract', description: `Interaction with an unverified contract at address ${address}.`, recommendation: "Interacting with unverified source code is a major security risk. Proceed with extreme caution." });
                }
                
                const knownContract = KNOWN_INTERFACES[address.toLowerCase()];
                if (knownContract) {
                    const lineContent = lines[line - 1];
                    const functionCallMatch = lineContent.match(/\.([a-zA-Z0-9_]+)\s*\(/);
                    if (functionCallMatch) {
                        const functionName = functionCallMatch[1];
                        if (!knownContract.functions.includes(functionName)) {
                             issues.push({ line: line, issue_type: 'Interface Mismatch', description: `Attempt to call non-existent function '${functionName}' on known contract '${knownContract.name}'.`, recommendation: `This is likely due to an outdated interface or a typo and will cause transactions to revert.` });
                        }
                    }
                }
            } catch (error: any) {
                console.warn(`Could not check address ${address}: ${error.message}`);
            }
        }
    }

    console.log(`V3 analysis complete. Found ${issues.length} dependency issues for Job ID: ${job.job_id}`);
    
    const uniqueIssues = issues.filter((issue, index, self) => index === self.findIndex((t) => (t.description === issue.description)));

    return {
        job_id: job.job_id,
        worker_name: "EcosystemDependencyWorkerV3",
        output: uniqueIssues,
    };
}

async function publishResult(client: any, result: AnalysisResult) {
    const channel = 'sentinel_results';
    const resultJson = JSON.stringify(result);
await client.rPush(channel, resultJson);
    console.log(`Published V3 result for Job ID: ${result.job_id}`);
}

main().catch(console.error);
