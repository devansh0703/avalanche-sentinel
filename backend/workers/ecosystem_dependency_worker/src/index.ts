import { createClient } from 'redis';
import * as semver from 'semver'; // V2: Import the semver library

interface AnalysisJob {
    job_id: string;
    source_code: string;
}

interface DependencyIssue {
    line: number;
    issue_type: 'Outdated Version' | 'Unknown Source' | 'Floating Pragma';
    description: string;
    recommendation: string;
}

interface AnalysisResult {
    job_id: string;
    worker_name: string;
    output: DependencyIssue[];
}

// V1: Curated knowledge base
const KNOWN_LIBRARIES: { [key: string]: string } = {
    '@openzeppelin/contracts': '5.0.0',
    '@openzeppelin/contracts-upgradeable': '5.0.0',
    '@avalabs/core': '0.5.5',
    '@traderjoe-xyz/core': '2.1.0'
};

async function main() {
    console.log('Starting Ecosystem & Dependency Worker [V2]...');
    const redisClient = createClient();
    await redisClient.connect();
    console.log('Successfully connected to Redis.');

    const channel = 'ecosystem_jobs';
    console.log(`Listening for jobs on channel: '${channel}'`);

    while (true) {
        try {
            const jobData = await redisClient.blPop(channel, 0);
            if (jobData) {
                console.log('\nReceived new job.');
                const job: AnalysisJob = JSON.parse(jobData.element);
                console.log(`Processing Job ID: ${job.job_id}`);

                const result = analyzeDependenciesV2(job);
                await publishResult(redisClient, result);
            }
        } catch (error) {
            console.error('Error processing job:', error);
        }
    }
}

function analyzeDependenciesV2(job: AnalysisJob): AnalysisResult {
    let issues: DependencyIssue[] = [];
    const importRegex = /import\s+["']([^"']+)["'];/g;
    const pragmaRegex = /pragma\s+solidity\s*([^\s;]+)/g; // V2: Regex for pragmas

    const lines = job.source_code.split('\n');
    for (let i = 0; i < lines.length; i++) {
        const lineContent = lines[i];
        const lineNum = i + 1;
        let match;

        // V1 Check: Imports
        while ((match = importRegex.exec(lineContent)) !== null) {
            const importPath = match[1];

            if (importPath.startsWith('http:') || importPath.startsWith('https:')) {
                issues.push({
                    line: lineNum,
                    issue_type: 'Unknown Source',
                    description: `Import from a direct URL (${importPath}) is detected.`,
                    recommendation: 'Importing from URLs is risky. Use a package manager with version pinning to ensure dependency integrity.',
                });
                continue;
            }

            for (const libName in KNOWN_LIBRARIES) {
                if (importPath.includes(libName)) {
                    const versionMatch = importPath.match(/@([\d\.]+-?[A-Za-z\.\d]*)/); // More flexible version regex
                    if (versionMatch) {
                        const importedVersion = semver.coerce(versionMatch[1]); // V2: Use semver to parse
                        const latestVersion = KNOWN_LIBRARIES[libName];
                        
                        // V2: Use semver.lt for proper comparison
                        if (importedVersion && semver.lt(importedVersion, latestVersion)) {
                             issues.push({
                                line: lineNum,
                                issue_type: 'Outdated Version',
                                description: `An outdated version of ${libName} (${importedVersion.version}) is imported.`,
                                recommendation: `The recommended version is ${latestVersion}. Outdated libraries may contain known vulnerabilities.`,
                            });
                        }
                    }
                }
            }
        }
        
        // --- V2 UPGRADE: Floating Pragma Check ---
        // Reset regex state for the next check on the same line
        pragmaRegex.lastIndex = 0; 
        while ((match = pragmaRegex.exec(lineContent)) !== null) {
            const versionConstraint = match[1];
            if (versionConstraint.startsWith('^') || versionConstraint.startsWith('~')) {
                issues.push({
                    line: lineNum,
                    issue_type: 'Floating Pragma',
                    description: `A floating pragma ('pragma solidity ${versionConstraint}') was detected.`,
                    recommendation: 'While useful for development, floating pragmas can lead to non-deterministic builds. For production contracts, pin to an exact Solidity version (e.g., `pragma solidity 0.8.20;`) to ensure verifiability and prevent unexpected behavior from future compiler versions.',
                });
            }
        }
        // --- END OF V2 UPGRADE ---
    }

    console.log(`V2 analysis complete. Found ${issues.length} dependency issues for Job ID: ${job.job_id}`);
    
    // Filter out duplicate issues
    const uniqueIssues = issues.filter((issue, index, self) =>
        index === self.findIndex((t) => (t.description === issue.description))
    );

    return {
        job_id: job.job_id,
        worker_name: "EcosystemDependencyWorkerV2",
        output: uniqueIssues,
    };
}

async function publishResult(client: any, result: AnalysisResult) {
    const channel = 'sentinel_results';
    const resultJson = JSON.stringify(result);
    await client.rPush(channel, resultJson);
    console.log(`Published V2 result for Job ID: ${result.job_id}`);
}

main().catch(console.error);
