import { createClient } from 'redis';

interface AnalysisJob {
    job_id: string;
    source_code: string;
}

interface DependencyIssue {
    line: number;
    issue_type: 'Outdated Version' | 'Unknown Source';
    description: string;
    recommendation: string;
}

interface AnalysisResult {
    job_id: string;
    worker_name: string;
    output: DependencyIssue[];
}

// THIS IS OUR CURATED AVALANCHE KNOWLEDGE BASE
// In a real-world app, this would come from a database or API.
const KNOWN_LIBRARIES: { [key: string]: string } = {
    '@openzeppelin/contracts': '5.0.0', // General purpose
    '@openzeppelin/contracts-upgradeable': '5.0.0', // General purpose
    '@avalabs/core': '0.5.5', // Avalanche-specific
    '@traderjoe-xyz/core': '2.1.0' // Avalanche DeFi
};

async function main() {
    console.log('Starting Ecosystem & Dependency Worker...');
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

                const result = analyzeDependencies(job);
                await publishResult(redisClient, result);
            }
        } catch (error) {
            console.error('Error processing job:', error);
        }
    }
}

function analyzeDependencies(job: AnalysisJob): AnalysisResult {
    let issues: DependencyIssue[] = [];
    // Regex to capture import paths like "library/path/Contract.sol" or "@library/path@version/..."
    const importRegex = /import\s+["']([^"']+)["'];/g;

    const lines = job.source_code.split('\n');
    for (let i = 0; i < lines.length; i++) {
        const lineContent = lines[i];
        let match;
        while ((match = importRegex.exec(lineContent)) !== null) {
            const importPath = match[1];

            // Check for unknown sources (e.g., direct github links)
            if (importPath.startsWith('http:') || importPath.startsWith('https:')) {
                issues.push({
                    line: i + 1,
                    issue_type: 'Unknown Source',
                    description: `Import from a direct URL (${importPath}) is detected.`,
                    recommendation: 'Importing from URLs is risky. Always use a package manager with version pinning (like Foundry or Hardhat) to ensure dependency integrity.',
                });
                continue; // Move to next import
            }

            // Check for outdated versions of known libraries
            for (const libName in KNOWN_LIBRARIES) {
                if (importPath.includes(libName)) {
                    // Simple version check, looks for @x.y.z in the path
                    const versionMatch = importPath.match(/@(\d+\.\d+\.\d+)/);
                    if (versionMatch) {
                        const importedVersion = versionMatch[1];
                        const latestVersion = KNOWN_LIBRARIES[libName];
                        
                        // NOTE: This is a basic string comparison. A real app would use semantic versioning.
                        if (importedVersion < latestVersion) {
                             issues.push({
                                line: i + 1,
                                issue_type: 'Outdated Version',
                                description: `An outdated version of ${libName} (${importedVersion}) is imported.`,
                                recommendation: `The recommended version is ${latestVersion}. Outdated libraries may contain known vulnerabilities.`,
                            });
                        }
                    }
                }
            }
        }
    }

    console.log(`Analysis complete. Found ${issues.length} dependency issues for Job ID: ${job.job_id}`);
    return {
        job_id: job.job_id,
        worker_name: "EcosystemDependencyWorker",
        output: issues,
    };
}

async function publishResult(client: any, result: AnalysisResult) {
    const channel = 'sentinel_results';
    const resultJson = JSON.stringify(result);
    await client.rPush(channel, resultJson);
    console.log(`Published result for Job ID: ${result.job_id}`);
}

main().catch(console.error);
