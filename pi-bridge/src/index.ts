import { AuthStorage, ModelRegistry } from "@earendil-works/pi-coding-agent";
import { parseArgs, createConsoleLogger } from "./config.js";
import { BridgeServer } from "./server.js";

async function main(): Promise<void> {
  const logger = createConsoleLogger();
  const config = parseArgs(process.argv.slice(2));

  if (config.agentDir) {
    logger.info("agentDir:", config.agentDir);
  }

  const authStorage = AuthStorage.create();
  const modelRegistry = ModelRegistry.create(authStorage);

  const server = new BridgeServer({ config, modelRegistry, logger });
  await server.start();
}

main().catch((e) => {
  console.error("[pi-bridge] fatal:", e);
  process.exit(1);
});
