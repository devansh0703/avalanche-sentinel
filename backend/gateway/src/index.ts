import express from 'express';
import http from 'http';
import { WebSocketServer, WebSocket, RawData } from 'ws';
import { createClient } from 'redis';
import { v4 as uuidv4 } from 'uuid';

// The request now includes the type of analysis to perform.
interface AnalysisJobRequest {
    source_code: string;
    analysis_type: 'security' | 'portability' | 'awm' | 'staking' | 'gas' | 'upgrade' | 'ecosystem' | 'consensus'; 
    subnet_genesis?:any;// Enforce specific types
}

interface AnalysisJob {
    job_id: string;
    source_code: string;
}

const PORT = process.env.PORT || 8080;

async function main() {
    const app = express();
    const server = http.createServer(app);
    const wss = new WebSocketServer({ server });
    console.log('WebSocket and HTTP server created.');

    const publisher = createClient();
    await publisher.connect();
    console.log('Redis Publisher connected.');

    const subscriber = publisher.duplicate();
    await subscriber.connect();
    console.log('Redis Subscriber connected and ready for results.');

    const clientJobMap = new Map<string, WebSocket>();
    console.log('Server is ready to accept connections.');

    wss.on('connection', (ws: WebSocket) => {
        console.log('Client connected.');

        ws.on('message', async (message: RawData) => {
            console.log('\nReceived message from client.');
            try {
                const messageString = message.toString('utf-8').trim();
                const request: AnalysisJobRequest = JSON.parse(messageString);

                // Basic validation
                if (!request.source_code || !request.analysis_type) {
                    ws.send(JSON.stringify({ error: '`source_code` and `analysis_type` fields are required.' }));
                    return;
                }

                const jobId = uuidv4();
                console.log(`Generated Job ID: ${jobId}`);
                clientJobMap.set(jobId, ws);

                const job: AnalysisJob = {
                    job_id: jobId,
                    source_code: request.source_code,
                    subnet_genesis: request.subnet_genesis
                };
                
                // --- DISPATCHER LOGIC ---
                let targetQueue: string;
                switch (request.analysis_type) {
                    case 'portability':
                        targetQueue = 'subnet_portability_jobs';
                        break;
                    case 'security':
                        targetQueue = 'core_security_jobs';
                        break;
                    case 'awm': // <-- ADD THIS CASE BLOCK
                        targetQueue = 'awm_interop_jobs';
                        break;
                    case 'staking': // <-- ADD THIS CASE BLOCK
                        targetQueue = 'staking_precompile_jobs';
                        break;
                    case 'gas': // <-- ADD THIS CASE BLOCK
                        targetQueue = 'gas_fee_jobs';
                        break;
                    case 'upgrade': // <-- ADD THIS CASE BLOCK
                        targetQueue = 'upgradeability_jobs';
                        break;
                    case 'ecosystem': // <-- ADD THIS CASE BLOCK
                        targetQueue = 'ecosystem_jobs';
                        break;
                    case 'consensus': // <-- ADD THIS CASE BLOCK
                        targetQueue = 'consensus_jobs';
                        break;
                    default:
                        ws.send(JSON.stringify({ error: `Unknown analysis_type: '${request.analysis_type}'` }));
                        return;
                }
                
                await publisher.lPush(targetQueue, JSON.stringify(job));
                console.log(`Job ${jobId} dispatched to queue: ${targetQueue}`);

                ws.send(JSON.stringify({ status: "Job Dispatched, Awaiting Analysis...", jobId: jobId }));

            } catch (error) {
                console.error('Failed to process message:', error);
                ws.send(JSON.stringify({ error: 'Invalid JSON message.' }));
            }
        });

        ws.on('close', () => console.log('Client disconnected.'));
    });

    // The result listener does not need to change, as all workers publish to the same channel.
    async function listenForResults() {
        console.log("Result listener started. Waiting for results on 'sentinel_results' list...");
        while (true) {
            try {
                const resultMessage = await subscriber.blPop('sentinel_results', 0);
                if (resultMessage) {
                    const message = resultMessage.element;
                    console.log(`\nReceived result payload from a worker.`);
                    const result = JSON.parse(message);
                    const { job_id } = result;

                    if (job_id && clientJobMap.has(job_id)) {
                        const clientWs = clientJobMap.get(job_id);
                        if (clientWs && clientWs.readyState === WebSocket.OPEN) {
                            console.log(`Forwarding result for Job ID ${job_id} to client.`);
                            clientWs.send(message); 
                        }
                        clientJobMap.delete(job_id);
                    } else {
                        console.warn(`Received result for an unknown or disconnected Job ID: ${job_id}`);
                    }
                }
            } catch (error) {
                console.error("Critical error in result listener:", error);
                await new Promise(resolve => setTimeout(resolve, 5000));
            }
        }
    }

    listenForResults();
    
    server.listen(PORT, () => console.log(`Gateway Server is listening on http://localhost:${PORT}`));
}

main().catch(console.error);
