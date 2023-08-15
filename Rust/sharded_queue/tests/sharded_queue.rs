use std::thread::{available_parallelism, scope};

use sharded_queue::ShardedQueue;

#[test]
fn push_and_pop_concurrently() {
    let max_concurrent_thread_count = available_parallelism().unwrap().get();
    let sharded_queue = ShardedQueue::new(max_concurrent_thread_count);
    let operation_count = 1e4 as usize;
    let mut joined_operation_result_list =
        Vec::with_capacity(operation_count * max_concurrent_thread_count);

    scope(|scope| {
        // Make a vector to hold the children which are spawned.
        let mut join_handle_list = vec![];

        for _ in 0..max_concurrent_thread_count {
            let join_handle = scope.spawn(|| {
                let mut operation_result_list = Vec::with_capacity(operation_count);
                for i in 0..operation_count {
                    sharded_queue.push_back(i);
                    operation_result_list.push(sharded_queue.pop_front_or_spin_wait_item());
                }

                operation_result_list
            });

            join_handle_list.push(join_handle);
        }

        for join_handle in join_handle_list {
            joined_operation_result_list.append(&mut join_handle.join().unwrap())
        }
    });

    joined_operation_result_list.sort();

    let mut expected_joined_operation_result_list =
        Vec::with_capacity(operation_count * max_concurrent_thread_count);
    for operation_index in 0..operation_count {
        for _thread_index in 0..max_concurrent_thread_count {
            expected_joined_operation_result_list.push(operation_index);
        }
    }

    assert_eq!(
        expected_joined_operation_result_list,
        joined_operation_result_list,
    );
}

#[test]
fn push_concurrently_then_pop_concurrently() {
    let max_concurrent_thread_count = available_parallelism().unwrap().get();
    let sharded_queue = ShardedQueue::new(max_concurrent_thread_count);
    let operation_count = 1e4 as usize;

    scope(|scope| {
        for _ in 0..max_concurrent_thread_count {
            scope.spawn(|| {
                for i in 0..operation_count {
                    sharded_queue.push_back(i);
                }
            });
        }
    });

    let mut joined_operation_result_list =
        Vec::with_capacity(operation_count * max_concurrent_thread_count);

    scope(|scope| {
        // Make a vector to hold the children which are spawned.
        let mut pop_join_handle_list = vec![];

        for _ in 0..max_concurrent_thread_count {
            pop_join_handle_list.push(scope.spawn(|| {
                let mut operation_result_list = Vec::with_capacity(operation_count);
                for _i in 0..operation_count {
                    operation_result_list.push(sharded_queue.pop_front_or_spin_wait_item());
                }

                operation_result_list
            }));
        }

        for join_handle in pop_join_handle_list {
            let ref mut operation_result_list = join_handle.join().unwrap();

            joined_operation_result_list.append(operation_result_list)
        }
    });

    joined_operation_result_list.sort();

    let mut expected_joined_operation_result_list =
        Vec::with_capacity(operation_count * max_concurrent_thread_count);
    for operation_index in 0..operation_count {
        for _thread_index in 0..max_concurrent_thread_count {
            expected_joined_operation_result_list.push(operation_index);
        }
    }

    assert_eq!(
        expected_joined_operation_result_list,
        joined_operation_result_list,
    );
}
