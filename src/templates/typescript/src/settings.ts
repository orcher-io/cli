// Where the engine is. `orcher` sets these for `orcher test`; export them
// yourself to point the worker and the starter elsewhere.
export const SERVER_URL = process.env.ORCHER_SERVER ?? 'http://localhost:50051';
export const NAMESPACE = process.env.ORCHER_NAMESPACE ?? 'default';
export const API_KEY = process.env.ORCHER_API_KEY || undefined;
export const TASK_QUEUE = '{{task_queue}}';
