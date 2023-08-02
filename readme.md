# `ShardedQueue`
`ShardedQueue` is needed for some schedulers and `NonBlockingMutex` as a highly specialized for their use case concurrent queue

`ShardedQueue` is a light-weight concurrent queue, which uses spin locking and fights lock contention with sharding

Notice that while it may seem that FIFO order is guaranteed, it is not, because there can be a situation, when multiple producers triggered long resize of very large shards, all but last, then passed enough time for resize to finish, then 1 producer triggers long resize of last shard, and all other threads start to consume or produce, and eventually start spinning on last shard, without guarantee which will acquire spin lock first, so we can't even guarantee that `ShardedQueue::pop_front_or_spin` will acquire lock before `ShardedQueue::push_back` on first attempt

Notice that this queue doesn't track length, since length's increment/decrement logic may change depending on use case, as well as logic when it goes from 1 to 0 or reverse(in some cases, like `NonBlockingMutex`, we don't even add action to queue when count reaches 1, but run it immediately in same thread), or even negative(to optimize some hot paths, like in some schedulers, since it is cheaper to restore count to correct state than to enforce that it can not go negative in some schedulers)

# Examples
```rust
use sharded_queue::ShardedQueue;
use std::cell::UnsafeCell;
use std::fmt::{Debug, Display, Formatter};
use std::marker::PhantomData;
use std::ops::{Deref, DerefMut};
use std::sync::atomic::{AtomicUsize, Ordering};

pub struct NonBlockingMutex<'captured_variables, State: ?Sized> {
    task_count: AtomicUsize,
    task_queue: ShardedQueue<Box<dyn FnOnce(MutexGuard<State>) + Send + 'captured_variables>>,
    unsafe_state: UnsafeCell<State>,
}

/// [NonBlockingMutex] is needed to run actions atomically without thread blocking, or context
/// switch, or spin lock contention, or rescheduling on some scheduler
///
/// Notice that it uses [ShardedQueue] which doesn't guarantee order of retrieval, hence
/// [NonBlockingMutex] doesn't guarantee order of execution too, even of already added
/// items
impl<'captured_variables, State> NonBlockingMutex<'captured_variables, State> {
    #[inline]
    pub fn new(max_concurrent_thread_count: usize, state: State) -> Self {
        Self {
            task_count: Default::default(),
            task_queue: ShardedQueue::new(max_concurrent_thread_count),
            unsafe_state: UnsafeCell::new(state),
        }
    }

    /// Please don't forget that order of execution is not guaranteed. Atomicity of operations is guaranteed,
    /// but order can be random
    #[inline]
    pub fn run_if_first_or_schedule_on_first(
        &self,
        run_with_state: impl FnOnce(MutexGuard<State>) + Send + 'captured_variables,
    ) {
        if self.task_count.fetch_add(1, Ordering::Acquire) != 0 {
            self.task_queue.push_back(Box::new(run_with_state));
        } else {
            // If we acquired first lock, run should be executed immediately and run loop started
            run_with_state(unsafe { MutexGuard::new(self) });
            /// Note that if [`fetch_sub`] != 1
            /// => some thread entered first if block in method
            /// => [ShardedQueue::push_back] is guaranteed to be called
            /// => [ShardedQueue::pop_front_or_spin] will not deadlock while spins until it gets item
            ///
            /// Notice that we run action first, and only then decrement count
            /// with releasing(pushing) memory changes, even if it looks otherwise
            while self.task_count.fetch_sub(1, Ordering::Release) != 1 {
                self.task_queue.pop_front_or_spin()(unsafe { MutexGuard::new(self) });
            }
        }
    }
}

/// [Send], [Sync], and [MutexGuard] logic was taken from [std::sync::Mutex]
/// and [std::sync::MutexGuard]
///
/// these are the only places where `T: Send` matters; all other
/// functionality works fine on a single thread.
unsafe impl<'captured_variables, State: Send> Send
    for NonBlockingMutex<'captured_variables, State>
{
}
unsafe impl<'captured_variables, State: Send> Sync
    for NonBlockingMutex<'captured_variables, State>
{
}

/// Code was mostly taken from [std::sync::MutexGuard], it is expected to protect [State]
/// from moving out of synchronized loop
pub struct MutexGuard<
    'captured_variables,
    'non_blocking_mutex_ref,
    State: ?Sized + 'non_blocking_mutex_ref,
> {
    non_blocking_mutex: &'non_blocking_mutex_ref NonBlockingMutex<'captured_variables, State>,
    /// Adding it to ensure that [MutexGuard] implements [Send] and [Sync] in same cases
    /// as [std::sync::MutexGuard] and protects [State] from going out of synchronized
    /// execution loop
    ///
    /// todo remove when this error is no longer actual
    ///  negative trait bounds are not yet fully implemented; use marker types for now [E0658]
    _phantom_unsend: PhantomData<std::sync::MutexGuard<'non_blocking_mutex_ref, State>>,
}

// todo uncomment when this error is no longer actual
//  negative trait bounds are not yet fully implemented; use marker types for now [E0658]
// impl<'captured_variables, 'non_blocking_mutex_ref, State: ?Sized> !Send
//     for MutexGuard<'captured_variables, 'non_blocking_mutex_ref, State>
// {
// }
unsafe impl<'captured_variables, 'non_blocking_mutex_ref, State: ?Sized + Sync> Sync
    for MutexGuard<'captured_variables, 'non_blocking_mutex_ref, State>
{
}

impl<'captured_variables, 'non_blocking_mutex_ref, State: ?Sized>
    MutexGuard<'captured_variables, 'non_blocking_mutex_ref, State>
{
    unsafe fn new(
        non_blocking_mutex: &'non_blocking_mutex_ref NonBlockingMutex<'captured_variables, State>,
    ) -> Self {
        Self {
            non_blocking_mutex,
            _phantom_unsend: PhantomData,
        }
    }
}

impl<'captured_variables, 'non_blocking_mutex_ref, State: ?Sized> Deref
    for MutexGuard<'captured_variables, 'non_blocking_mutex_ref, State>
{
    type Target = State;

    #[inline]
    fn deref(&self) -> &State {
        unsafe { &*self.non_blocking_mutex.unsafe_state.get() }
    }
}

impl<'captured_variables, 'non_blocking_mutex_ref, State: ?Sized> DerefMut
    for MutexGuard<'captured_variables, 'non_blocking_mutex_ref, State>
{
    #[inline]
    fn deref_mut(&mut self) -> &mut State {
        unsafe { &mut *self.non_blocking_mutex.unsafe_state.get() }
    }
}

impl<'captured_variables, 'non_blocking_mutex_ref, State: ?Sized + Debug> Debug
    for MutexGuard<'captured_variables, 'non_blocking_mutex_ref, State>
{
    #[inline]
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        Debug::fmt(&**self, f)
    }
}

impl<'captured_variables, 'non_blocking_mutex_ref, State: ?Sized + Display> Display
    for MutexGuard<'captured_variables, 'non_blocking_mutex_ref, State>
{
    #[inline]
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        (**self).fmt(f)
    }
}
```

# Benchmarks
| benchmark_name                            | operation_count_per_thread | concurrent_thread_count | average_time |
|-------------------------------------------|---------------------------:|------------------------:|-------------:|
| sharded_queue_push_and_pop_concurrently   |                      1_000 |                      24 |    3.1980 ms |
| crossbeam_queue_push_and_pop_concurrently |                      1_000 |                      24 |    5.3154 ms |
| queue_mutex_push_and_pop_concurrently     |                      1_000 |                      24 |    6.4846 ms |
| sharded_queue_push_and_pop_concurrently   |                     10_000 |                      24 |    37.245 ms |
| crossbeam_queue_push_and_pop_concurrently |                     10_000 |                      24 |    49.234 ms |
| queue_mutex_push_and_pop_concurrently     |                     10_000 |                      24 |    69.207 ms |
| sharded_queue_push_and_pop_concurrently   |                    100_000 |                      24 |    395.12 ms |
| crossbeam_queue_push_and_pop_concurrently |                    100_000 |                      24 |    434.00 ms |
| queue_mutex_push_and_pop_concurrently     |                    100_000 |                      24 |    476.59 ms |

