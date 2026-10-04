// Runs the hello workflow end to end against the engine: starts a worker in
// this process, starts the workflow, and checks its result.
//
//   orcher test        # or: npm test
import { runHello } from './start';
import { buildWorker } from './worker';

async function main(): Promise<number> {
  const worker = buildWorker();
  const running = worker.run();
  let result: string;
  try {
    result = await runHello('test');
  } finally {
    await worker.shutdown();
    await running.catch(() => undefined);
  }
  if (result !== 'Hello, test!') {
    console.error(`FAIL: hello returned ${JSON.stringify(result)}`);
    return 1;
  }
  console.log("ok: hello returned 'Hello, test!'");
  return 0;
}

main()
  .then((code) => process.exit(code))
  .catch((err) => {
    console.error(err);
    process.exit(1);
  });
