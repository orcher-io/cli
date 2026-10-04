// The workflows and tasks of {{project_name}}.
//
// A workflow decides what happens and in what order; it must be
// deterministic, so anything that touches the outside world (an API, a
// database, the clock) goes in a task.
import {
  Task, Tasks, Workflow, createTaskRefs,
  type TaskContext, type WorkflowContext,
} from '@orcher/sdk';

@Tasks()
export class GreetingTasks {
  // Tasks are where side effects go: call an API or write to a database here.
  @Task()
  async greet(_ctx: TaskContext, name: string): Promise<string> {
    return `Hello, ${name}!`;
  }
}

export const greetingTasks = createTaskRefs(GreetingTasks);

@Workflow({ name: 'hello' })
export class Hello {
  async run(ctx: WorkflowContext, name: string): Promise<string> {
    return ctx.executeTask(greetingTasks.greet, name);
  }
}
