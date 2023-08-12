use task::{Task, TaskMut, TaskOnce};

pub trait PopFrontTaskAndRun<Arguments, Output> {
    fn pop_front_task_and_run(&mut self, arguments: Arguments) -> Option<Output>;
}

pub trait PushBackTaskOnce<Arguments, Output, TTaskOnce: TaskOnce<Arguments, Output>> {
    fn push_back_task_once(&mut self, task: TTaskOnce);
}

pub trait PushBackTaskMut<Arguments, Output, TTaskMut: TaskMut<Arguments, Output>> {
    fn push_back_task_mut(&mut self, task: TTaskMut);
}

pub trait PushBackTask<Arguments, Output, TTask: Task<Arguments, Output>> {
    fn push_back_task(&mut self, task: TTask);
}

impl<
        Arguments,
        Output,
        TTaskMut: TaskMut<Arguments, Output>,
        TPushBackTaskOnce: PushBackTaskOnce<Arguments, Output, TTaskMut>,
    > PushBackTaskMut<Arguments, Output, TTaskMut> for TPushBackTaskOnce
{
    fn push_back_task_mut(&mut self, task: TTaskMut) {
        self.push_back_task_once(task)
    }
}

impl<
        Arguments,
        Output,
        TTask: Task<Arguments, Output>,
        TPushBackTaskMut: PushBackTaskMut<Arguments, Output, TTask>,
    > PushBackTask<Arguments, Output, TTask> for TPushBackTaskMut
{
    fn push_back_task(&mut self, task: TTask) {
        self.push_back_task_mut(task)
    }
}
