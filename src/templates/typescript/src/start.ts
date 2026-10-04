// The starter: start the hello workflow and wait for its result.
//
//   npm run start-workflow -- Ada
import { randomUUID } from 'node:crypto';
import { Client } from '@orcher/sdk';
import { API_KEY, NAMESPACE, SERVER_URL, TASK_QUEUE } from './settings';

export async function runHello(name: string): Promise<string> {
  const client = new Client({ serverUrl: SERVER_URL, namespace: NAMESPACE, apiKey: API_KEY });
  await client.connect();
  try {
    const handle = await client.startWorkflow<string>({
      workflowType: 'hello',
      taskQueue: TASK_QUEUE,
      workflowId: `hello-${randomUUID().slice(0, 8)}`,
      args: [name],
    });
    return await handle.result();
  } finally {
    client.close();
  }
}

if (require.main === module) {
  runHello(process.argv[2] ?? 'world')
    .then((result) => console.log(result))
    .catch((err) => {
      console.error(err);
      process.exit(1);
    });
}
