use criterion::{black_box, criterion_group, criterion_main, Criterion};
use nameof::name_of;
use std::collections::VecDeque;
use std::sync::Mutex;
use std::thread::{available_parallelism, scope};

use crossbeam_queue::SegQueue;
use sharded_queue::ShardedQueue;

fn crossbeam_queue_push_and_pop_concurrently(
    operation_count: usize,
    max_concurrent_thread_count: usize,
) {
    let queue = SegQueue::new();
    scope(|scope| {
        for _ in 0..max_concurrent_thread_count {
            scope.spawn(|| {
                for i in 0..operation_count {
                    queue.push(i);
                    queue.pop();
                }
            });
        }
    });
}

fn sharded_queue_push_and_pop_concurrently(
    operation_count: usize,
    max_concurrent_thread_count: usize,
) {
    let sharded_queue = ShardedQueue::<usize>::new(max_concurrent_thread_count);

    scope(|scope| {
        for _ in 0..max_concurrent_thread_count {
            scope.spawn(|| {
                for i in 0..operation_count {
                    sharded_queue.push_back(i);
                    sharded_queue.pop_front_or_spin();
                }
            });
        }
    });
}

fn queue_mutex_push_and_pop_concurrently(
    operation_count: usize,
    max_concurrent_thread_count: usize,
) {
    let queue_mutex = Mutex::new(VecDeque::new());

    scope(|scope| {
        for _ in 0..max_concurrent_thread_count {
            scope.spawn(|| {
                for i in 0..operation_count {
                    {
                        let mut queue = queue_mutex.lock().unwrap();
                        queue.push_back(i);
                    }

                    {
                        let mut queue = queue_mutex.lock().unwrap();
                        queue.pop_front();
                    }
                }
            });
        }
    });
}

fn criterion_benchmark(criterion: &mut Criterion) {
    struct BenchFnAndName {
        bench_fn: fn(usize, usize),
        bench_fn_name: String,
    }

    let bench_fn_and_name_list = [
        BenchFnAndName {
            bench_fn: sharded_queue_push_and_pop_concurrently,
            bench_fn_name: String::from(name_of!(sharded_queue_push_and_pop_concurrently)),
        },
        BenchFnAndName {
            bench_fn: crossbeam_queue_push_and_pop_concurrently,
            bench_fn_name: String::from(name_of!(crossbeam_queue_push_and_pop_concurrently)),
        },
        BenchFnAndName {
            bench_fn: queue_mutex_push_and_pop_concurrently,
            bench_fn_name: String::from(name_of!(queue_mutex_push_and_pop_concurrently)),
        },
    ];

    let operation_count_per_thread_list = [1e3 as usize, 1e4 as usize, 1e5 as usize];

    let max_concurrent_thread_count = available_parallelism().unwrap().get();

    for operation_count_per_thread in operation_count_per_thread_list {
        let name_of_operation_count_per_thread = String::from(name_of!(operation_count_per_thread));

        for bench_fn_and_name in bench_fn_and_name_list.iter() {
            let (bench_fn, bench_fn_name) =
                (bench_fn_and_name.bench_fn, &bench_fn_and_name.bench_fn_name);
            criterion.bench_function(
                &format!("{bench_fn_name}|{name_of_operation_count}={operation_count}|concurrent_thread_count={max_concurrent_thread_count}"),
                |bencher| {
                    bencher.iter(|| {
                        bench_fn(
                            black_box(operation_count_per_thread),
                            black_box(max_concurrent_thread_count),
                        )
                    })
                },
            );
        }
    }
}

criterion_group!(benches, criterion_benchmark);
criterion_main!(benches);
