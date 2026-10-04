// The worker: runs {{project_name}}'s workflows and tasks until stopped.
import { Worker } from '@orcher/sdk';
import './workflows';
import { API_KEY, NAMESPACE, SERVER_URL, TASK_QUEUE } from './settings';

export function buildWorker(): Worker {
  return new Worker({
    serverUrl: SERVER_URL,
    namespace: NAMESPACE,
    taskQueue: TASK_QUEUE,
    apiKey: API_KEY,
  });
}

if (require.main === module) {
  const worker = buildWorker();
  console.log(`worker polling ${TASK_QUEUE} on ${SERVER_URL}`);
  worker.run().catch((err) => {
    console.error(err);
    process.exit(1);
  });
}
