# 🔺 Avalanche Sentinel V3 🔺

**A Hyper-Specialized, Multi-Agent Auditing Platform for the Avalanche Ecosystem**

**Project by: Bitflippers**
(Dhairya Shukla • Devansh Raulo • Ashmit Kinariwala • Aaryan Kamdar)
[![Watch the Demo](https://img.shields.io/badge/Watch_the-Demo-E84142?style=for-the-badge&logo=youtube)](https://youtu.be/UuEp99h5Zto)
---

| Status | License | Platform | Languages |
| :--- | :--- | :--- | :--- |
| [![Status](https://img.shields.io/badge/Status-Complete-brightgreen?style=for-the-badge)](https://github.com/devansh0703/avalanche-sentinel) | [![License](https://img.shields.io/badge/License-MIT-blue?style=for-the-badge)](https://github.com/devansh0703/avalanche-sentinel/blob/main/LICENSE) | ![Avalanche](https://img.shields.io/badge/Avalanche-E84142?style=for-the-badge&logo=avalanche&logoColor=white) | ![Rust](https://img.shields.io/badge/rust-%23000000.svg?style=for-the-badge&logo=rust&logoColor=white) ![Node.js](https://img.shields.io/badge/node.js-6DA55F?style=for-the-badge&logo=node.js&logoColor=white) |

---

## 1. Executive Summary: The Vision

Avalanche's vision is a future of thousands of interconnected, high-performance blockchains. This is a paradigm shift that demands a new generation of developer tooling. **Avalanche Sentinel** is our answer to that demand. It is not just another linter; it is an intelligent, multi-agent auditing platform designed from the ground up to be a native of the Avalanche ecosystem.

While generic tools see a monolithic EVM, Sentinel sees the rich, complex, and interconnected reality of Avalanche. It understands the unique opportunities and the hidden risks of Subnets, Avalanche Warp Messaging, and the P-Chain staking layer. Its mission is to provide developers with the deep, context-aware insights needed to build safely, efficiently, and ambitiously in a multi-chain world.

## 2. The Problem: The Perilous Reality of Multi-Chain Development

The promise of Subnets is unparalleled scalability and customization. But this power introduces a new, invisible landscape of risk:

*   **Environmental Dissonance:** A contract that is perfectly safe and gas-efficient on the C-Chain can be insecure, financially unviable, or completely broken on a custom Subnet with a different fee market, gas limit, or set of enabled precompiles.
*   **Cross-Chain Complexity:** Avalanche Warp Messaging is a powerful tool for interoperability, but it also creates a new class of vulnerabilities—replay attacks, state desynchronization, untrusted relayer risks—that single-chain tools are completely blind to.
*   **Implicit Finality Assumption:** Developers building on Avalanche become accustomed to its sub-second finality. They unknowingly write code that, while safe on Avalanche, is catastrophically vulnerable to the block reorganizations common on slower-finality chains they may wish to bridge to in the future.
*   **Ecosystem Fragmentation:** As the number of protocols on Avalanche explodes, developers face a huge risk of "supply chain" attacks by importing outdated or incorrect interfaces for critical DeFi primitives like DEXs and lending markets.

Current tools were not built for this reality. They are single-chain guardians in a multi-chain universe.

## 3. The Solution: A Fleet of Hyper-Specialized Workers

Avalanche Sentinel solves this with a unique **polyglot microservice architecture**. Instead of one engine trying to do everything, we built a fleet of eight "Workers," each a hyper-specialized expert in a specific domain of Avalanche development.

```
+--------------------------------+
|         Solidity Code          |
|      (via Frontend/Client)     |
+--------------------------------+
                 |
                 v
+--------------------------------+
|  Gateway (Node.js + WebSocket) |
|   (Intelligent Job Dispatcher)   |
+--------------------------------+
                 | (Publishes Jobs)
                 v
+--------------------------------+
|     Message Broker (Redis)     |
+--------------------------------+
                 | (Subscribes to Jobs)
                 v
+-------------------------------------------------+
|          THE SENTINEL WORKER FLEET          |
|-------------------------------------------------|
| [RUST] Core Security     | [RUST] Subnet Portability |
| [NODE] AWM Interop       | [RUST] Staking Precompile |
| [RUST] Consensus         | [NODE] Gas & Fee Market   |
| [NODE] Upgradeability    | [NODE] Ecosystem & Deps   |
+-------------------------------------------------+
                 | (Publishes Results)
                 v
+--------------------------------+
|     Message Broker (Redis)     |
+--------------------------------+
                 | (Subscribes to Results)
                 v
+--------------------------------+
|  Gateway (Node.js + WebSocket) |
+--------------------------------+
                 |
                 v
+--------------------------------+
|      Comprehensive Report      |
|         (to Frontend)          |
+--------------------------------+
```

This decoupled design allows us to use the best tool for each job: **Rust** for its performance and safety in CPU-intensive analysis, and **Node.js** for its rich ecosystem in I/O-intensive analysis involving AST parsing and external API calls.

---

## 4. The Workers: A Deep Dive into Capabilities

Each worker is a complete V2/V3 module, combining multiple layers of analysis. Click the details below to expand each worker's full capabilities.

<details>
<summary><strong>[✓✓✓] 1. The Core Security Worker (V2.1 - Rust)</strong></summary>

*   **Mission:** To provide a comprehensive, industry-standard security baseline for any smart contract deployed on an EVM-compatible chain, including the Avalanche C-Chain.
*   **Checks Performed:**
    *   **Broad Vulnerability Analysis:** Leverages the full power of the industry-standard Slither static analysis engine. This detects a wide range of common and critical smart contract vulnerabilities out of the box, including but not limited to:
        *   Re-entrancy (Checks-Effects-Interactions pattern violations)
        *   Integer Overflows/Underflows (for Solidity versions <0.8.0)
        *   Unprotected `selfdestruct` and `delegatecall`
        *   Transaction Order Dependency
        *   Oracle Manipulation Risks
        *   And dozens more.
    *   **Compiler Warning Ingestion (V2.1 Feature):** Goes beyond standard analysis by capturing and parsing the `stderr` output from the `solc` compiler during Slither's execution. This elevates low-level compiler warnings (e.g., Unused Local Variables, Uninitialized Storage Pointers) to first-class issues, finding potential bugs and code quality problems that are often overlooked by developers and other tools.
*   **Why it's Avalanche-Specific:** While the checks are universal, this worker provides the foundational security layer that every Avalanche C-Chain and Subnet contract *must* pass before considering the more complex, Avalanche-native risks. It establishes a baseline of code quality and safety for the entire ecosystem.

</details>

<details>
<summary><strong>[✓✓✓] 2. The Subnet Portability Worker (V3 - Rust)</strong></summary>

*   **Mission:** To act as a "Subnet Simulator," ensuring a contract is ready for deployment on any custom Subnet by validating it against a specific Subnet's on-chain environment.
*   **Checks Performed:**
    *   **`chainid` Opcode Usage:** Flags any logic that relies on a specific `chainid`, which is a common but critical mistake that breaks contracts when moved from a testnet to a mainnet or between Subnets.
    *   **Native Token Assumptions (`msg.value`, `.balance`):** Warns on any usage of `msg.value` or `address.balance`, reminding the developer that the native token on a custom Subnet may not be AVAX and could have a different value, or no value at all.
    *   **Hardcoded C-Chain Addresses:** Detects dependencies on protocols and tokens (e.g., Trader Joe, Benqi, WAVAX) that only exist on the C-Chain and will not be present on a new Subnet.
    *   **Hardcoded Gas Values:** Flags fragile `.call{gas:...}` patterns, which can break on Subnets with different gas semantics or future opcode repricing.
    *   **Genesis Ingestion (V3 Feature):** Takes a Subnet's `genesis.json` as input to perform deep, context-aware analysis:
        *   **Predicts Gas Limit Violations:** Reads the `blockGasLimit` from the genesis file and cross-references it with a function's estimated gas cost, warning the developer if a transaction is guaranteed to revert on the target Subnet.
        *   **Detects Precompile Mismatches:** Reads the list of enabled precompiles from the genesis and flags any contract that attempts to call a precompile (like the P-Chain handler) that is not explicitly enabled on the target Subnet, preventing a guaranteed revert.

</details>

<details>
<summary><strong>[✓✓✓] 3. The AWM Interoperability Worker (V3 - Node.js)</strong></summary>

*   **Mission:** To secure the most critical and complex aspect of multi-chain applications: Avalanche Warp Messaging.
*   **Checks Performed:**
    *   **Missing `receive` Function:** Checks that a contract importing the AWM interface can actually receive messages.
    *   **Missing `try/catch` on `send`:** Flags calls to `warp.send()` that are not wrapped in a `try/catch` block, which can cause the entire transaction to revert on a send failure, leading to poor user experience.
    *   **Missing Replay Protection:** Audits the `receive` function for a nonce or `executedMessages` mapping to prevent a malicious actor from replaying a valid message multiple times to drain funds or mint tokens.
    *   **Untrusted Relayer Risk (V3 Feature):** Ensures the `receive` function validates that `msg.sender` is the official AWM Precompile address. This is a critical check to ensure the message was delivered through the official, secure Warp protocol and not by a malicious actor who happened to obtain a valid signed message.
    *   **State Desynchronization Hazard (V3 Feature):** Flags "fire-and-forget" state updates (e.g., `setPrice`, `changeOwner`) that are sent via AWM but where the contract lacks a corresponding failure handler or rollback function, preventing a state desync between chains if the message fails to arrive.

</details>

<details>
<summary><strong>[✓✓✓] 4. The Staking Precompile Worker (V3 - Rust)</strong></summary>

*   **Mission:** To audit deep, protocol-level interactions with the Avalanche P-Chain, securing the creation of novel liquid staking and delegation financial products.
*   **Checks Performed:**
    *   **Missing `payable` Modifier:** Flags non-payable functions that call staking precompiles which require a value (AVAX) to be sent.
    *   **Unchecked Return Values:** Detects low-level `.call`s to precompiles whose `success` boolean return value is not checked, which can lead to critical silent failures.
    *   **Weak Access Control:** Warns if public or external functions can alter the staking state of the contract without robust access control like `onlyOwner`.
    *   **Locked Rewards Hazard (V3 Feature):** Detects if a contract is set up to receive staking rewards from the P-Chain but has no apparent `withdraw` or `distribute` function, indicating a high risk of permanently locked reward funds.
    *   **Hardcoded Validator Dependency (V3 Feature):** Flags hardcoded `NodeID`s, recommending that the protocol implement off-chain health monitoring (uptime, fees) for this critical, centralized point of failure.

</details>

<details>
<summary><strong>[✓✓✓] 5. The Consensus Compliance Worker (V3 - Rust)</strong></summary>

*   **Mission:** To build "reorg-safe" contracts that are future-proof for the broader multi-chain world by auditing for logic that implicitly relies on Avalanche's fast finality.
*   **Checks Performed:**
    *   **Unsafe On-Chain Randomness (V3 Feature):** Detects the use of `block.timestamp`, `blockhash`, etc. for randomness in gaming or NFT applications, a critical vulnerability that can be manipulated by validators. Recommends Chainlink VRF.
    *   **Spot Price Oracle Usage:** Flags direct, single-transaction price reads from DEXs (`getReserves`) that are vulnerable to flash loan price manipulation on slower-finality chains. Recommends TWAP oracles.
    *   **Multi-Transaction Dependency Hazard:** Detects critical admin changes (e.g., `setOwner`) that occur without a time-lock, which is a reorg-vulnerable pattern.

</details>

<details>
<summary><strong>[✓✓✓] 6. The Gas & Fee Market Worker (V3 - Node.js)</strong></summary>

*   **Mission:** To be a Subnet-aware gas and economic profiler, helping developers write cheaper and more efficient code.
*   **Checks Performed:**
    *   **Gas Inefficiencies:** Detects common anti-patterns like `SSTORE` in loops, `memory` vs. `calldata` misuse for external function parameters, and inefficient data types in structs.
    *   **Subnet Fee Comparison (V3 Feature):** Ingests a `genesis.json` to provide a powerful comparative cost analysis: "This function costs X on the C-Chain but will cost Y on Subnet Z due to its different `minBaseFee`."
    *   **Griefing Vector Hazard (V3 Feature):** Identifies functions that accept unbounded dynamic data (e.g., `string`, `bytes`), which is a critical attack vector on low-fee Subnets where an attacker can force the contract to perform expensive operations at no cost to themselves.

</details>

<details>
<summary><strong>[✓✓✓] 7. The Upgradeability & Governance Worker (V3 - Node.js)</strong></summary>

*   **Mission:** To secure complex proxies and multi-chain DAOs, preventing devastating and irreversible mistakes.
*   **Checks Performed:**
    *   **Unprotected Initializers:** Finds `initialize` functions that are not protected by an `initializer` modifier, which can be hijacked by an attacker to seize ownership of an implementation contract.
    *   **Storage Layout Collision Risk:** Warns about incorrect state variable ordering in child contracts, a primary cause of proxy storage corruption during upgrades.
    *   **`selfdestruct` in Implementation:** Flags this critical vulnerability in proxy logic that could allow an attacker to destroy the contract's code, bricking all proxies.
    *   **AWM Governance Exploit Risk (V3 Feature):** Specifically audits AWM-based governors to ensure they validate both `sourceChainId` and `sender`, preventing a sophisticated cross-chain spoofing attack where an attacker on another Subnet could take control of the contract.

</details>

<details>
<summary><strong>[✓✓✓] 8. The Ecosystem & Dependency Worker (V3 - Node.js)</strong></summary>

*   **Mission:** To provide live, on-chain supply chain security by validating a contract's external dependencies.
*   **Checks Performed:**
    *   **Floating Pragma Detection:** Recommends pinning to an exact Solidity version for deterministic and verifiable builds.
    *   **Outdated Package Versions:** Uses `semver` to check imported packages against a curated list of recommended versions for major libraries like OpenZeppelin.
    *   **Unverified Contract Interaction (V3 Feature):** Connects to the **Snowtrace API** to warn developers whenever their contract interacts with an unverified, black-box contract that is live on the Avalanche C-Chain.
    *   **Interface Mismatch Detection (V3 Feature):** Checks function calls to known protocol addresses (e.g., Trader Joe Router) against a curated list of function signatures to find integration bugs that would cause transactions to revert.

</details>

---

## 5. How to Run This Project

### Prerequisites
- Node.js (v18+)
- Rust & Cargo
- `redis-server`
- Python3 & `pip`
- `solc-select`
- `slither-analyzer`
- A free Snowtrace API Key (for the Ecosystem worker)

### Installation & Setup

1.  **Clone the repository:**
    ```bash
    git clone https://github.com/devansh0703/avalanche-sentinel.git
    cd avalanche-sentinel
    ```

2.  **Install System Dependencies:**
    ```bash
    # For Ubuntu/Debian
    sudo apt-get update && sudo apt-get install redis-server python3-pip -y
    # Install Python tools
    pip3 install slither-analyzer solc-select
    # Install and set a default solc version
    solc-select install 0.8.20
    solc-select use 0.8.20
    ```

3.  **Configure API Key:**
    Open `backend/workers/ecosystem_dependency_worker/src/index.ts` and replace the placeholder `'YOUR_API_KEY_HERE'` with your actual Snowtrace API key.

### Running the Platform

The backend requires **9 separate terminal windows**.

1.  **Start the Gateway (Terminal 1):**
    ```bash
    cd backend/gateway
    npm install
    npm start
    ```

2.  **Start the Workers (Terminals 2-9):**
    For each of the 8 directories inside `backend/workers/`:
    *   If it's a Rust worker (e.g., `core_security_worker`):
        ```bash
        cd backend/workers/core_security_worker
        cargo run
        ```
    *   If it's a Node.js worker (e.g., `awm_interop_worker`):
        ```bash
        cd backend/workers/awm_interop_worker
        npm install
        npm start
        ```

### Using the Frontend

1.  **Open the UI:**
    Once all backend services are running, simply open the `index.html` file located in the root `avalanche-sentinel` directory in your web browser.

2.  **Run an Analysis:**
    *   Paste your Solidity code into the main text area.
    *   If using the **Portability** or **Gas** workers, you can paste a `subnet_genesis.json` into the optional text area that appears.
    *   Select the desired analysis type from the dropdown.
    *   Click "Analyze".
    *   Results will be displayed in real-time.

---

## 6. Future Roadmap

*   **Full-Featured Frontend:** Rebuild the UI in a modern framework like **React/Next.js** with an embedded **Monaco Editor** for a world-class, VS Code-like developer experience.
*   **CI/CD Integration:** Package Sentinel as a **GitHub Action** to automatically audit contracts on every `git push`, bringing security directly and seamlessly into the developer workflow.
*   **AI-Powered Auditing:** Integrate a Large Language Model (LLM), fine-tuned on our workers' findings, to provide natural language explanations of vulnerabilities and suggest concrete, AI-generated code fixes for developers.
*   **Sentinel for Subnet Deployers:** Create a specialized version of Sentinel designed for Subnet creators to audit their `genesis.json` and precompile configurations for security, economic stability, and best practices before launching their chain.

---

**Built with ❤️ for the Avalanche Ecosystem by Bitflippers.**
