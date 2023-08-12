use multi_type_task_queue_builder::build_multi_type_task_queue;
use task::TaskOnce;

#[test]
fn test() {
    pub struct DecrementTask {}
    impl TaskOnce<&mut usize, ()> for DecrementTask {
        fn run_once(self, arguments: &mut usize) -> () {
            *arguments -= 1;
        }
    }

    pub struct IncrementTask {}
    impl TaskOnce<&mut usize, ()> for IncrementTask {
        fn run_once(self, arguments: &mut usize) -> () {
            *arguments += 1;
        }
    }
    // enum TaskType {
    //     DecrementTask,
    //     IncrementTask,
    // }
    // pub struct SizedTaskQueue {
    //     type_enum_queue: VecDeque<TaskType>,
    //     queue_0: VecDeque<DecrementTask>,
    //     queue_1: VecDeque<IncrementTask>,
    // }
    // impl SizedTaskQueue {
    //     pub fn new() -> Self {
    //         Self {
    //             type_enum_queue: Default::default(),
    //             queue_0: Default::default(),
    //             queue_1: Default::default(),
    //         }
    //     }
    // }
    // impl PopFrontTaskAndRun<&mut usize, ()> for SizedTaskQueue {
    //     fn pop_front_task_and_run<ProcessedOutput>(
    //         &mut self,
    //         arguments: &mut usize,
    //         process_output: impl FnOnce(()) -> ProcessedOutput,
    //     ) -> Option<ProcessedOutput> {
    //         let task_type_option = self.type_enum_queue.pop_front();
    //         match task_type_option {
    //             None => {
    //                 return None;
    //             }
    //             Some(type_enum) => match type_enum {
    //                 TaskType::DecrementTask => match self.queue_0.pop_front() {
    //                     None => unsafe {
    //                         hint::unreachable_unchecked();
    //                     },
    //                     Some(task) => return Some(process_output(task.run(arguments))),
    //                 },
    //                 TaskType::IncrementTask => match self.queue_1.pop_front() {
    //                     None => unsafe {
    //                         hint::unreachable_unchecked();
    //                     },
    //                     Some(task) => return Some(process_output(task.run(arguments))),
    //                 },
    //             },
    //         }
    //     }
    // }
    // impl PushBackTask<&mut usize, (), DecrementTask> for SizedTaskQueue {
    //     fn push_back_task(&mut self, task: DecrementTask) {
    //         self.type_enum_queue.push_back(TaskType::DecrementTask);
    //         self.queue_0.push_back(task);
    //     }
    // }
    // impl PushBackTask<&mut usize, (), IncrementTask> for SizedTaskQueue {
    //     fn push_back_task(&mut self, task: IncrementTask) {
    //         self.type_enum_queue.push_back(TaskType::IncrementTask);
    //         self.queue_1.push_back(task);
    //     }
    // }
    //

    struct MultiTypeTaskQueue {}
    build_multi_type_task_queue!(
        MultiTypeTaskQueue,
        &mut usize,
        (),
        DecrementTask,
        IncrementTask,
    );

    // #[derive(BuildMultiTypeTaskQueue, tasks(IncrementTask, DecrementTask))]
    // let mut queue = MyTaskQueue::new();
    // queue.push_back_task(IncrementTask {});
    // queue.push_back_task(DecrementTask {});
    // let mut x = 0;
    // queue.pop_front_task_and_run(&mut x, |()| ());
    // queue.pop_front_task_and_run(&mut x, |()| ());
    // println!("{}", x); // prints -1
}
